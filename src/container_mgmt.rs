use bollard::{
    container::{
        Config, CreateContainerOptions, CreateContainerResults, HostConfig,
        InspectContainerOptions, StartContainerOptions,
    },
    Docker,
};
use log::{info, warn};
use snafu::{ResultExt, Snafu};
use std::net::IpAddr;
use tokio::net::TcpStream;

use crate::{Opt, DOCKER, OPTS};

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to deploy container: {}", source))]
    DeployContainer { source: bollard::errors::Error },
    #[snafu(display(
        "Failed to connect to container on port ({:?}:{}): {}",
        ip,
        port,
        source
    ))]
    ConnectContainer {
        ip: IpAddr,
        port: u16,
        source: tokio::io::Error,
    },
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerID(String);

impl ContainerID {
    pub fn into_inner(self) -> String {
        self.0
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeployedContainer {
    pub(crate) id: ContainerID,
    pub(crate) ip_address: IpAddr,
}

impl DeployedContainer {
    pub async fn connect(&self, port: u16) -> Result<TcpStream> {
        TcpStream::connect((self.ip_address, port))
            .await
            .context(ConnectContainer {
                ip: self.ip_address,
                port,
            })
    }
}

pub async fn deploy_container(docker: &Docker, opts: &Opt) -> Result<DeployedContainer> {
    info!("Creating container: {}", opts.image);

    let config = Config {
        image: Some(opts.image.as_str()),
        host_config: Some(HostConfig {
            privileged: if opts.privileged { Some(true) } else { None },
            binds: Some(opts.binds.iter().map(|x| x.as_str()).collect()),
            ..Default::default()
        }),
        ..Default::default()
    };

    let CreateContainerResults { id, warnings } = docker
        .create_container(None::<CreateContainerOptions<String>>, config)
        .await
        .context(DeployContainer)?;

    if let Some(warnings) = warnings {
        for warning in warnings {
            warn!("Creating container resulted in error: \"{}\"", warning);
        }
    }

    info!("Starting container: {}", opts.image);

    docker
        .start_container(&id, None::<StartContainerOptions<String>>)
        .await
        .context(DeployContainer)?;

    let container = docker
        .inspect_container(&id, None::<InspectContainerOptions>)
        .await
        .context(DeployContainer)?;

    Ok(DeployedContainer {
        id: ContainerID(id),
        ip_address: container.network_settings.ip_address.parse().unwrap(),
    })
}

/// `deploy_container` but uses global values for docker and opts
pub async fn new_container() -> Result<DeployedContainer> {
    deploy_container(&DOCKER, &OPTS).await
}

pub async fn kill_container(id: &ContainerID) {
    use bollard::container::KillContainerOptions;

    let _ = DOCKER
        .kill_container(id.as_str(), None::<KillContainerOptions<String>>)
        .await;
}
