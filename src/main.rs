#![recursion_limit = "256"]

use std::collections::HashSet;
use bollard::Docker;
use lazy_static::lazy_static;
use log::{debug, info, error};
use snafu::{ResultExt, Snafu};
use structopt::StructOpt;
use tokio::sync::oneshot;

// lol

#[macro_use]
mod utils;
mod connections;
mod container_mgmt;
mod listener;
mod single_consumer;

lazy_static! {
    static ref DOCKER: Docker = Docker::connect_with_local_defaults()
        .context(error::Docker)
        .unwrap();
    static ref OPTS: Opt = Opt::from_args();
}

mod error {
    use snafu::Snafu;
    use crate::listener;

    #[derive(Debug, Snafu)]
    #[snafu(visibility = "pub(crate)")]
    pub enum Error {
        #[snafu(display("An error occured with docker: {}", source))]
        Docker { source: bollard::errors::Error },
        #[snafu(display("An error occured creating a listener: {}", source))]
        ListenerSpawn { source: listener::Error },
    }
}

type Result<T, E = error::Error> = std::result::Result<T, E>;

#[derive(Debug, Snafu)]
enum ParsePortError {
    #[snafu(display("Invalid port range: {} (parsed as: {})", range, parse))]
    InvalidPortRange { range: String, parse: String },
    #[snafu(display("Failed to parse lhs of port range: {}", source))]
    LHSInvalid { source: std::num::ParseIntError },
    #[snafu(display("Failed to parse rhs of port range: {}", source))]
    RHSInvalid { source: std::num::ParseIntError },
}

fn parse_ports(src: &str) -> Result<Vec<u16>, ParsePortError> {
    if let Ok(x) = src.parse::<u16>() {
        return Ok(vec![x]);
    }

    let range: Vec<_> = src.split('-').collect();
    let (l, r) = match range.as_slice() {
        [l, r] => (l, r),
        x => {
            return InvalidPortRange {
                range: src.to_string(),
                parse: format!("{:?}", x),
            }
            .fail()
        }
    };

    let l = l.parse::<u16>().context(LHSInvalid)?;
    let r = r.parse::<u16>().context(RHSInvalid)?;

    Ok((l..r).collect())
}

#[derive(Debug, StructOpt)]
#[structopt(name = "container-per-ip", about = "Run a container per client ip", author = "Ben Simms <ben@bensimms.moe>")]
pub struct Opt {
    #[structopt()]
    /// The docker image to run for each ip
    pub image: String,

    #[structopt(long)]
    /// Should the containers be started with the `--privileged` flag
    pub privileged: bool,

    #[structopt(short, long, parse(try_from_str = parse_ports))]
    /// Ports to listen on (tcp only currently)
    pub ports: Vec<Vec<u16>>,

    #[structopt(short, long)]
    /// Volume bindings to provide to containers
    pub binds: Vec<String>,

    #[structopt(short, long)]
    /// Environment variables to set on the child container
    pub env: Vec<String>,

    #[structopt(short, long, default_value = "300")]
    /// Timeout (seconds) after an IPs last connection disconnects before
    /// killing the associated container
    pub timeout: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    flexi_logger::Logger::with_env_or_str("info, container_per_ip = debug")
        .duplicate_to_stderr(flexi_logger::Duplicate::All)
        .format_for_stderr(flexi_logger::colored_detailed_format)
        .start()
        .unwrap();

    let ports: HashSet<_> = OPTS.ports.iter().flatten().cloned().collect();

    let (listener_stop_tx, listener_stop_rx): (Vec<_>, Vec<_>) = ports
        .iter()
        .map(|_| oneshot::channel())
        .unzip();

    let (evt_tx, context) = connections::Context::new(listener_stop_tx);

    debug!("Setting C-c handler");

    ctrlc::set_handler({
        let evt_tx = evt_tx.clone();
        move || {
            error_on_error!(evt_tx.send(connections::ConnectionEvent::Stop));
        }
    })
    .unwrap();

    debug!("Starting listeners");

    let listener_handles_fut = ports
        .iter()
        .zip(listener_stop_rx)
        .map(|(port, stop_rx)| listener::listen_on(*port, evt_tx.clone(), stop_rx))
        .collect::<Vec<_>>();

    let mut listener_handles = Vec::with_capacity(listener_handles_fut.len());

    for fut in listener_handles_fut {
        match fut.await.context(error::ListenerSpawn) {
            Ok(handle) => listener_handles.push(handle),
            Err(e) => {
                error!("Failed spawning a listener, aborting");
                error!("Reason: {}", e);
                error_on_error!(evt_tx.send(connections::ConnectionEvent::Stop));
                break;
            }
        }
    };

    context.handle_events().await;

    for handle in listener_handles {
        error_on_error!(handle.await);
    }

    info!("Bye");

    Ok(())
}
