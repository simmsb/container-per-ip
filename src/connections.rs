use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::sync::oneshot;

use crate::container_mgmt::{ContainerID, DeployedContainer};
use crate::timeout_queue::CancellableTimeoutQueue;

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
