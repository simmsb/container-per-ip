use bollard::image::CreateImageOptions;
use bollard::{network::ConnectNetworkOptions, Docker};
use clap::Parser;
use futures::StreamExt;
use miette::IntoDiagnostic;
use once_cell::sync::Lazy;
use std::collections::HashSet;
use tokio::sync::oneshot;
use tracing::{debug, error, info};

mod connections;
mod container_mgmt;
mod listener;
mod single_consumer;
mod utils;

static DOCKER: Lazy<Docker> = Lazy::new(|| Docker::connect_with_local_defaults().unwrap());
static OPTS: Lazy<Opt> = Lazy::new(Opt::parse);

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum Error {
    #[error("An error occured with docker")]
    Docker {
        #[source]
        source: bollard::errors::Error,
    },
    #[error("An error occured creating a listener")]
    ListenerSpawn {
        #[source]
        #[diagnostic_source]
        source: listener::Error,
    },
    #[error("An error occured doing fs stuff")]
    IOError {
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
enum ParsePortError {
    #[error("Invalid port range: {} (parsed as: {})", range, parse)]
    InvalidRange { range: String, parse: String },
    #[error("Failed to parse number: {}", source)]
    NoParse {
        #[source]
        source: std::num::ParseIntError,
    },
}

fn parse_ports(src: &str) -> miette::Result<Vec<u16>> {
    if let Ok(x) = src.parse::<u16>() {
        return Ok(vec![x]);
    }

    let range: Vec<_> = src.split('-').collect();
    match range.as_slice() {
        [x] => {
            let x = x
                .parse::<u16>()
                .map_err(|err| ParsePortError::NoParse { source: err })?;
            Ok(vec![x])
        }
        [l, r] => {
            let l = l
                .parse::<u16>()
                .map_err(|err| ParsePortError::NoParse { source: err })?;
            let r = r
                .parse::<u16>()
                .map_err(|err| ParsePortError::NoParse { source: err })?;

            Ok((l..r).collect())
        }
        x => Err(ParsePortError::InvalidRange {
            range: src.to_string(),
            parse: format!("{:?}", x),
        }
        .into()),
    }
}

#[derive(Debug, clap::Parser)]
#[clap(about = "Run a container per client ip", author)]
pub struct Opt {
    /// The docker image to run for each ip
    pub image: String,

    #[clap(long)]
    /// Should the containers be started with the `--privileged` flag
    pub privileged: bool,

    #[clap(short, long, parse(try_from_str = parse_ports))]
    /// Ports to listen on (tcp only currently)
    pub ports: Vec<Vec<u16>>,

    #[clap(short, long)]
    /// Volume bindings to provide to containers
    pub binds: Vec<String>,

    #[clap(long)]
    pub network: Option<String>,

    #[clap(short, long)]
    /// Environment variables to set on the child container
    pub env: Vec<String>,

    #[clap(short, long, default_value = "300")]
    /// Timeout (seconds) after an IPs last connection disconnects before
    /// killing the associated container
    pub timeout: u16,
}

fn install_tracing() -> miette::Result<()> {
    use tracing_subscriber::fmt::format::FmtSpan;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_span_events(FmtSpan::CLOSE)
        .pretty();
    let filter_layer = tracing_subscriber::EnvFilter::from_default_env()
        .add_directive("container_per_ip=info".parse().into_diagnostic()?);

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    Ok(())
}

async fn pull_if_needed() -> Result<(), Error> {
    let image = OPTS.image.as_str();
    let need_to_pull = match DOCKER.inspect_image(image).await {
        Ok(_) => false,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => true,
        Err(e) => return Err(Error::Docker { source: e }),
    };

    if need_to_pull {
        info!(image, "Pulling image");

        let tag = image.rsplit_once(':').map(|(_, t)| t).unwrap_or("latest");

        let mut s = DOCKER.create_image(
            Some(CreateImageOptions {
                from_image: image,
                tag,
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(msg) = s.next().await {
            let progress = msg.map_err(|e| Error::Docker { source: e })?;
            info!(?progress, "Pulling image");
        }

        info!("Finished pulling image");
    } else {
        info!("Image {} already loaded", image);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    install_tracing()?;

    pull_if_needed().await?;

    let ports: HashSet<_> = OPTS.ports.iter().flatten().cloned().collect();

    let (listener_stop_tx, listener_stop_rx): (Vec<_>, Vec<_>) =
        ports.iter().map(|_| oneshot::channel()).unzip();

    let (evt_tx, context) = connections::Context::new(listener_stop_tx);

    debug!("Setting C-c handler");

    ctrlc::set_handler({
        let evt_tx = evt_tx.clone();
        move || {
            trace_error!(evt_tx.send(connections::ConnectionEvent::Stop));
        }
    })
    .unwrap();

    if in_container::in_container() {
        if let Some(network) = OPTS.network.as_ref() {
            debug!("Adding ourselves to {}", network);

            let hostname = std::fs::read_to_string("/etc/hostname")
                .map_err(|e| Error::IOError { source: e })?;

            let config = ConnectNetworkOptions {
                container: hostname.trim(),
                endpoint_config: Default::default(),
            };

            DOCKER
                .connect_network(network, config)
                .await
                .map_err(|e| Error::Docker { source: e })?;
        }
    }

    debug!("Starting listeners");

    let listener_handles_fut = ports
        .iter()
        .zip(listener_stop_rx)
        .map(|(port, stop_rx)| listener::listen_on(*port, evt_tx.clone(), stop_rx))
        .collect::<Vec<_>>();

    let mut listener_handles = Vec::with_capacity(listener_handles_fut.len());

    for fut in listener_handles_fut {
        match fut.await.map_err(|e| Error::ListenerSpawn { source: e }) {
            Ok(handle) => listener_handles.push(handle),
            Err(err) => {
                error!("Failed spawning a listener, aborting");
                return Err(err.into());
            }
        }
    }

    context.handle_events().await;

    for handle in listener_handles {
        trace_error!(handle.await);
    }

    info!("Bye");

    Ok(())
}
