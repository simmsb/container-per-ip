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
    #[snafu(display("Container wasn't running when it should be"))]
    NotRunning,
    #[snafu(display("Failed to get a container's ip address"))]
    ContainerIP,
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerID(String);

impl ContainerID {
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

fn get_container_ip(c: &bollard::container::Container) -> Result<IpAddr> {
    if let Ok(ip) = c.network_settings.ip_address.parse() {
        return Ok(ip);
    };

    for net in c.network_settings.networks.values() {
        if let Ok(ip) = dbg!(net).ip_address.parse() {
            return Ok(ip);
        }
    }

    return Err(Error::ContainerIP);
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

    if !container.state.running {
        return Err(Error::NotRunning);
    }

    Ok(DeployedContainer {
        id: ContainerID(id),
        ip_address: get_container_ip(&container)?,
    })
}

/// `deploy_container` but uses global values for docker and opts
pub async fn new_container() -> Result<DeployedContainer> {
    deploy_container(&DOCKER, &OPTS).await
}

pub async fn remove_container(id: &ContainerID) {
    use bollard::container::{StopContainerOptions, RemoveContainerOptions};

    error_on_error!(
        DOCKER
            .stop_container(id.as_str(), Some(StopContainerOptions { t: 5 }))
            .await
    );

    error_on_error!(
        DOCKER
            .remove_container(
                id.as_str(),
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                })
            )
            .await
    );
}
