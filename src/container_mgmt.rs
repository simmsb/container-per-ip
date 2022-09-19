use bollard::{
    container::{Config, CreateContainerOptions, InspectContainerOptions, StartContainerOptions},
    service::{ContainerCreateResponse, HostConfig},
    Docker,
};
use std::net::IpAddr;
use tokio::net::TcpStream;
use tracing::{info, warn};

use crate::{Opt, DOCKER, OPTS};

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum Error {
    #[error("Failed to deploy container")]
    DeployContainer {
        #[source]
        source: bollard::errors::Error,
    },
    #[error("Failed to connect to container on port ({:?}:{})", ip, port)]
    ConnectContainer {
        ip: IpAddr,
        port: u16,

        #[source]
        source: tokio::io::Error,
    },
    #[error("Container {:?} wasn't running when it should be", id)]
    NotRunning { id: ContainerID },
    #[error("Failed to get the state of container {:?}", id)]
    NoState { id: ContainerID },
    #[error("Failed to get the ip address of container {:?}", id)]
    ContainerIP { id: ContainerID },
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
            .map_err(|e| Error::ConnectContainer {
                ip: self.ip_address,
                port,
                source: e,
            })
    }

    pub async fn check_up(&self) -> bool {
        let inspection = match DOCKER.inspect_container(self.id.as_str(), None).await {
            Ok(x) => x,
            Err(err) => {
                tracing::error!(
                    ?err,
                    "Failed to get container info (maybe it was deleted under our feet)"
                );
                return false;
            }
        };

        inspection
            .state
            .map(|s| s.running.unwrap_or(false))
            .unwrap_or(false)
    }
}

fn get_container_ip(
    id: &ContainerID,
    c: &bollard::models::ContainerInspectResponse,
) -> Result<IpAddr> {
    for net in c
        .network_settings
        .as_ref()
        .and_then(|s| Some(s.networks.as_ref()?.values()))
        .ok_or_else(|| Error::ContainerIP { id: id.to_owned() })?
    {
        if let Some(ip) = net.ip_address.as_ref().and_then(|ip| ip.parse().ok()) {
            return Ok(ip);
        }
    }

    Err(Error::ContainerIP { id: id.to_owned() })
}

pub async fn deploy_container(docker: &Docker, opts: &Opt) -> Result<DeployedContainer> {
    info!("Creating container: {}", opts.image);

    let config = Config {
        image: Some(opts.image.clone()),
        host_config: Some(HostConfig {
            privileged: if opts.privileged { Some(true) } else { None },
            binds: Some(opts.binds.clone()),
            network_mode: opts.network.clone(),
            ..Default::default()
        }),
        env: Some(opts.env.clone()),
        ..Default::default()
    };

    let ContainerCreateResponse { id, warnings } = docker
        .create_container(None::<CreateContainerOptions<String>>, config)
        .await
        .map_err(|e| Error::DeployContainer { source: e })?;

    for warning in warnings {
        warn!("Creating container resulted in error: \"{}\"", warning);
    }

    info!("Starting container: {}", opts.image);

    docker
        .start_container(&id, None::<StartContainerOptions<String>>)
        .await
        .map_err(|e| Error::DeployContainer { source: e })?;

    let container = docker
        .inspect_container(&id, None::<InspectContainerOptions>)
        .await
        .map_err(|e| Error::DeployContainer { source: e })?;

    if !container
        .state
        .as_ref()
        .ok_or_else(|| Error::NoState {
            id: ContainerID(id.to_owned()),
        })?
        .running
        .unwrap_or(false)
    {
        return Err(Error::NotRunning {
            id: ContainerID(id.to_owned()),
        });
    }

    let id = ContainerID(id);
    let ip_address = get_container_ip(&id, &container)?;
    Ok(DeployedContainer { id, ip_address })
}

/// `deploy_container` but uses global values for docker and opts
pub async fn new_container() -> Result<DeployedContainer> {
    deploy_container(&DOCKER, &OPTS).await
}

pub async fn remove_container(id: &ContainerID) {
    use bollard::container::{RemoveContainerOptions, StopContainerOptions};

    crate::trace_error!(
        DOCKER
            .stop_container(id.as_str(), Some(StopContainerOptions { t: 5 }))
            .await
    );

    crate::trace_error!(
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
