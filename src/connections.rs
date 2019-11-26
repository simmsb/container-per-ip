use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use snafu::{ResultExt, Snafu};

use log::{debug, error};

use futures::select;
use futures::future::FutureExt;

use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::sync::{mpsc, oneshot};

use crate::container_mgmt::{ContainerID, DeployedContainer};
use crate::timeout_queue::TimeoutQueue;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Something went wrong managing a container: {}", source))]
    ContainerMgmt {
        source: crate::container_mgmt::Error,
    },
}

type Result<T, E = Error> = std::result::Result<T, E>;

enum ConnectionEvent {
    ConnClosed(SocketAddr),
}

#[derive(Debug)]
struct ActiveConnection {
    // might eventually become a message channel
    should_close: oneshot::Sender<()>,
    container: DeployedContainer,
}

impl ActiveConnection {
    fn new(container: DeployedContainer) -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();

        let conn = ActiveConnection {
            should_close: tx,
            container,
        };

        (conn, rx)
    }
}

// // queue of containers that are disconnected, item type is a oneshot receiver to
// // allow for containers to be removed from the queue once they are reconnected
// containers_to_reap: TimeoutQueue<oneshot::Receiver<DeployedContainer>>,

async fn container_reaper(
    timeout: Duration,
    mut receiver: mpsc::UnboundedReceiver<oneshot::Receiver<DeployedContainer>>,
) {
    enum Action {
        Add(oneshot::Receiver<DeployedContainer>),
        Reap(oneshot::Receiver<DeployedContainer>),
    }

    let timeout_queue = TimeoutQueue::new();
    // let timeout_queue = Arc::new(TimeoutQueue::new());
    // let (canc_send, canc_recv) = mpsc::unbounded_channel();

    // tokio::spawn(async move {
    //     while let Some(to_cancel) = timeout_queue.pop().await {
    //         if canc_send.try_send(to_cancel).is_err() {
    //             break;
    //         }
    //     }
    // });

    loop {
        select! {
            to_add = FutureExt::fuse(receiver.recv()) => {
                timeout_queue.push(to_add, timeout).await;
            },
            to_reap = FutureExt::fuse(timeout_queue.pop()) => {
                if let Some(to_reap) = to_reap {
                    // if this returns None, it means the reaping was cancelled
                    if let Ok(container) = to_reap.await {
                        // TODO: stop and remove container
                    }
                }
            }
        }
    }
}

async fn rx_tx_loop(addr: SocketAddr,
                    mut lhs: TcpStream, mut rhs: TcpStream,
                    close: oneshot::Receiver<()>,
                    mut events: mpsc::UnboundedSender<ConnectionEvent>) {
    let mut read_write_buf = [0; 1024];
    let mut write_read_buf = [0; 1024];

    let (mut lhs_r, mut lhs_w) = lhs.split();
    let (mut rhs_r, mut rhs_w) = rhs.split();

    select! {
        _ = FutureExt::fuse(close) => {
            debug!("Stopping transmission ({:?} <--> {:?}) commanded to stop.", lhs, rhs);
        },
        _ = FutureExt::fuse(lhs_r.copy(&mut rhs_w)) => {
            debug!("Stopping transmission ({:?} <--> {:?}) rhs_w or lhs_r closed.", lhs, rhs);
            let _ = events.try_send(ConnectionEvent::ConnClosed(addr));
        },
        _ = FutureExt::fuse(rhs_r.copy(&mut lhs_w)) => {
            debug!("Stopping transmission ({:?} <--> {:?}) lhs_w or rhs_r closed.", lhs, rhs);
            let _ = events.try_send(ConnectionEvent::ConnClosed(addr));
        },
    }

}

#[derive(Debug)]
pub struct InnerContext {
    event_chan: (mpsc::UnboundedSender<ConnectionEvent>, mpsc::UnboundedReceiver<ConnectionEvent>),

    // ongoing active connections
    active_connections: HashMap<SocketAddr, ActiveConnection>,

    active_containers: HashMap<ContainerID, (usize, DeployedContainer)>,

    // sends containers to a worker queue to reap deployed containers after a timeout
    containers_to_reap: mpsc::UnboundedSender<oneshot::Receiver<DeployedContainer>>,

    // disconnected containers, contains the same receivers of disconnected containers
    // as `containers_to_ream` and is used to bring disconnected containers back alive
    disconnected_containers: HashMap<ContainerID, oneshot::Receiver<DeployedContainer>>,
}

impl InnerContext {
    async fn create_connection(&mut self,
                               client_addr: SocketAddr,
                               port: u16,
                               client_stream: TcpStream,
                               container: DeployedContainer) -> Result<()> {
        let container_stream = container.connect(port).await.context(ContainerMgmt)?;

        // TODO: pull container out of reaper

        let (connection, close_send) = ActiveConnection::new(container.clone());
        // let connection = Arc::new(connection);

        self.active_connections.insert(client_addr, connection);
        self.active_containers
            .entry(container.id.clone())
            .and_modify(|(num_conn, cont)| { *num_conn += 1; })
            .or_insert_with(|| (0, container.clone()));

        tokio::spawn(rx_tx_loop(client_addr,
                                client_stream,
                                container_stream,
                                close_send,
                                self.event_chan.0.clone(),
        ));

        Ok(())
    }

    async fn close_connection(&mut self, client_addr: SocketAddr) {
        let active_connection = self
            .active_connections
            .remove(&client_addr)
            .expect("Connection closed but already removed from map?");

        // we don't care if the receiver is closed or not.
        let _ = active_connection.should_close.send(());

        let active_container =
            self.active_containers
                .get_mut(&active_connection.container.id)
                .expect("Container didn't exist but was being removed?");

        *active_container.0 -= 1;

        if *active_container == 0 {
            // no more entries, move the con
            let active_container = self
                .active_containers
                .remove(&active_connection.container.id)
                .unwrap();

            // TODO: Single Taker thingy
            let (mut tx, mut rx) = oneshot::channel();
            tx.send(active_container.1).unwrap();

            self.containers_to_reap.try_send(rx).unwrap();

        }

    }
}
