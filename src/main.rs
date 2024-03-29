use bollard::image::CreateImageOptions;
use bollard::service::ContainerInspectResponse;
use bollard::{network::ConnectNetworkOptions, Docker};
use clap::Parser;
use coerce::actor::system::ActorSystem;
use coerce::actor::{IntoActor, IntoActorId};
use futures::StreamExt;
use itertools::Itertools;
use miette::IntoDiagnostic;
use once_cell::sync::Lazy;
use tracing::{debug, error, info};

use crate::coordinator::Coordinator;
use crate::port::PortMode;

mod container;
mod container_connection;
mod container_mgmt;
pub mod coordinator;
mod create_connection;
mod listener;
pub mod port;
mod utils;

static DOCKER: Lazy<Docker> = Lazy::new(|| Docker::connect_with_local_defaults().unwrap());
static OPTS: Lazy<Opts> = Lazy::new(Opts::parse);

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

#[derive(Debug, clap::Parser)]
#[clap(about = "Run a container per client ip", author)]
pub struct Opts {
    /// The docker image to run for each ip
    pub image: String,

    #[clap(long)]
    /// Should the containers be started with the `--privileged` flag
    pub privileged: bool,

    #[clap(short, long)]
    /// Ports to listen on
    ///
    /// The supported syntax for ports is: udp:53, tcp:8080:80 (outside:inside),
    /// tcp:5000-5100 (range), tcp:5000-5100:6000-6100 (outside range - inside range)
    pub ports: Vec<String>,

    #[clap(short, long)]
    /// Volume bindings to provide to containers
    pub binds: Vec<String>,

    #[clap(short, long)]
    /// Set the docker network containers should be started in
    pub network: Option<String>,

    #[clap(short, long)]
    /// Environment variables to set on the child container
    pub env: Vec<String>,

    #[clap(short, long, default_value = "300")]
    /// Timeout (seconds) after an IPs last connection disconnects before
    /// killing the associated container
    pub timeout: u16,

    /// Specifies the unique id set in the container-per-ip.parent tag of
    /// spawned containers.
    ///
    /// By default, containers will be tagged with `container-per-ip.parent = <uuid>`
    /// If specified, containers will be tagged with `container-per-ip.parent = <container_tag_suffix>`
    #[clap(long)]
    pub parent_id: Option<String>,

    /// Always pull the image on start
    #[clap(long)]
    pub force_pull: bool,
}

fn install_tracing() -> miette::Result<()> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let fmt_layer = tracing_subscriber::fmt::layer().compact();
    let filter_layer = tracing_subscriber::EnvFilter::builder()
        .with_default_directive("container_per_ip=info".parse().into_diagnostic()?)
        .from_env()
        .into_diagnostic()?;

    tracing_subscriber::registry()
        .with(tracing_error::ErrorLayer::default())
        .with(filter_layer)
        .with(fmt_layer)
        .init();

    Ok(())
}

async fn pull_if_needed(force: bool) -> Result<(), Error> {
    let image = OPTS.image.as_str();

    let need_to_pull = if force {
        true
    } else {
        match DOCKER.inspect_image(image).await {
            Ok(_) => false,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => true,
            Err(e) => return Err(Error::Docker { source: e }),
        }
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

    pull_if_needed(OPTS.force_pull).await?;

    let ports: Vec<_> = OPTS
        .ports
        .iter()
        .map(|p| port::parse_port_mapping(p.as_str()))
        .try_collect()?;
    let ports = ports.into_iter().flat_map(|c| c.all_ports()).collect_vec();

    if in_container::in_container() {
        if let Some(network) = OPTS.network.as_ref() {
            debug!("Adding ourselves to {}", network);

            let hostname = std::fs::read_to_string("/etc/hostname")
                .map_err(|e| Error::IOError { source: e })?;

            let container_name = hostname.trim();

            let insp = DOCKER
                .inspect_container(container_name, None)
                .await
                .map_err(|e| Error::Docker { source: e })?;

            fn in_network(name: &str, insp: ContainerInspectResponse) -> Option<bool> {
                let networks = insp.network_settings?.networks?;

                Some(networks.contains_key(name))
            }

            let already_joined = in_network(network.as_str(), insp).unwrap_or(false);

            if already_joined {
                debug!("Already in network");
            } else {
                let config = ConnectNetworkOptions {
                    container: container_name,
                    endpoint_config: Default::default(),
                };

                DOCKER
                    .connect_network(network, config)
                    .await
                    .map_err(|e| Error::Docker { source: e })?;
            }
        }
    }

    debug!("Starting coordinator");

    let coordinator = Coordinator::new()
        .into_actor(
            Some("coordinator".into_actor_id()),
            &ActorSystem::global_system(),
        )
        .await
        .unwrap();

    debug!("Starting listeners");

    for (mode, (ext, int)) in ports {
        match mode {
            PortMode::Tcp => listener::listen_on_tcp(ext, int, coordinator.clone()).await?,
            PortMode::Udp => listener::listen_on_udp(ext, int, coordinator.clone()).await?,
        };
    }

    tokio::signal::ctrl_c().await.unwrap();

    container_mgmt::cleanup_containers().await?;

    info!("Bye");

    Ok(())
}
