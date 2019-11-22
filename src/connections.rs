use std::sync::Arc;
use std::net::SocketAddr;
use std::collections::HashMap;

use tokio::net::TcpStream;
use tokio::sync::oneshot;
use tokio::prelude::*;

use crate::timeout_queue::CancellableTimeoutQueue;
use crate::container_mgmt::{DeployedContainer, ContainerID};

#[derive(Debug)]
pub struct ActiveConnection {
    // might eventually become a message channel
    should_close: oneshot::Receiver<()>,
    container: DeployedContainer,
}

#[derive(Debug)]
pub struct InnerContext {
    active_connections: HashMap<SocketAddr, Arc<ActiveConnection>>,
    disconnected_containers: CancellableTimeoutQueue<ContainerID, DeployedContainer>,
}
