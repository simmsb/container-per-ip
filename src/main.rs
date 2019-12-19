#![recursion_limit="256"]

use bollard::Docker;
use snafu::{ResultExt, Snafu};
use structopt::StructOpt;
use lazy_static::lazy_static;
use tokio::sync::oneshot;
use log::{info, debug};

mod connections;
mod container_mgmt;
mod single_consumer;
mod listener;

lazy_static! {
    static ref DOCKER: Docker =
        Docker::connect_with_local_defaults().context(DockerError).unwrap();

    static ref OPTS: Opt = Opt::from_args();
}

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("An error occured with docker: {}", source))]
    DockerError { source: bollard::errors::Error },
    #[snafu(display("An error occured joining tokio handles {}", source))]
    TokioJoinError { source: tokio::task::JoinError },
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, StructOpt)]
#[structopt(name = "container-per-ip", about = "Run a container per client ip")]
pub struct Opt {
    #[structopt()]
    /// The docker image to run for each ip
    pub image: String,

    #[structopt(long)]
    /// Should the containers be started with the `--privileged` flag
    pub privileged: bool,

    #[structopt(short, long)]
    /// Ports to listen on (tcp only currently)
    pub ports: Vec<u16>,

    #[structopt(short, long)]
    /// Volume bindings to provide to containers
    pub binds: Vec<String>,

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

    let version = DOCKER.version().await.context(DockerError)?;

    debug!("Docker version: {:?}", version);

    let (listener_stop_tx, listener_stop_rx): (Vec<_>, Vec<_>) =
        OPTS.ports.iter().map(|_| oneshot::channel()).unzip();

    let (evt_tx, context) = connections::Context::new(listener_stop_tx);

    let listener_handles = OPTS.ports.iter().zip(listener_stop_rx)
        .map(|(port, stop_rx)|
             tokio::spawn(listener::listen_on(*port, evt_tx.clone(), stop_rx)))
        .collect::<Vec<_>>();

    debug!("Started listeners");

    ctrlc::set_handler(move || {
        let evt_tx = evt_tx.clone();
        let _ = evt_tx.send(connections::ConnectionEvent::SIGINTSent);
    }).unwrap();

    debug!("Set C-c handler");

    context.handle_events().await;

    for handle in listener_handles {
        handle.await.context(TokioJoinError)?;
    }

    info!("Bye");

    Ok(())
}
