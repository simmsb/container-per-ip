use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use snafu::{ResultExt, Snafu};

use multi_map::MultiMap;

use log::{debug, error, info};

use pin_utils::unsafe_pinned;

use tokio::select;
use tokio::io;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::DelayQueue;
use tokio::stream::StreamExt;

use crate::container_mgmt::{new_container, kill_container, ContainerID, DeployedContainer};
use crate::single_consumer::SingleConsumer;
use crate::OPTS;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Something went wrong managing a container: {}", source))]
    ContainerMgmt {
        source: crate::container_mgmt::Error,
    },
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub enum ConnectionEvent {
    ConnCreate(SocketAddr, u16, TcpStream),
    ConnClosed(SocketAddr),
    ContainerClosed(ContainerID),
    Stop,
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

struct UnfuseFut<Fut> {
    future: Fut,
}

impl<Fut> UnfuseFut<Fut> {
    fn new(future: Fut) -> Self {
        UnfuseFut {
            future
        }
    }

    unsafe_pinned!(future: Fut);
}

impl<Fut: Unpin> Unpin for UnfuseFut<Fut> {}

impl<Fut: Future<Output = Option<T>>, T> Future for UnfuseFut<Fut> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut futures::task::Context<'_>) -> Poll<Self::Output> {
        match self.as_mut().future().poll(cx) {
            Poll::Ready(None) => Poll::Pending,
            Poll::Ready(Some(x)) => Poll::Ready(x),
            Poll::Pending => Poll::Pending,
        }
    }
}

async fn container_reaper(
    timeout: Duration,
    events: mpsc::UnboundedSender<ConnectionEvent>,
    mut receiver: mpsc::UnboundedReceiver<SingleConsumer<DeployedContainer>>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut delay_queue = DelayQueue::new();
    info!("Starting reaper");

    loop {
        select! {
            _ = &mut stop => {
                info!("Stopping reaper");
                return;
            },
            to_add = receiver.recv() => {
                if let Some(to_add) = to_add {
                    delay_queue.insert(to_add, timeout);
                }
            },
            to_reap = UnfuseFut::new(delay_queue.next()) => {
                if let Ok(to_reap) = to_reap {
                    // if this is None, it means the reaping was cancelled
                    if let Some(container) = to_reap.into_inner().take() {
                        info!("Shutting down container {:?}", container.id);

                        kill_container(&container.id).await;

                        let _ = events.send(ConnectionEvent::ContainerClosed(container.id));
                    }
                }
            }
        }
    }
}

async fn rx_tx_loop(
    addr: SocketAddr,
    mut lhs: TcpStream,
    mut rhs: TcpStream,
    close: oneshot::Receiver<()>,
    events: mpsc::UnboundedSender<ConnectionEvent>,
) {
    let (mut lhs_r, mut lhs_w) = lhs.split();
    let (mut rhs_r, mut rhs_w) = rhs.split();

    select! {
        _ = close => {
            debug!("Stopping transmission ({:?} <--> {:?}) commanded to stop.", lhs, rhs);
        },
        _ = io::copy(&mut lhs_r, &mut rhs_w) => {
            debug!("Stopping transmission ({:?} <--> {:?}) rhs_w or lhs_r closed.", lhs, rhs);
            let _ = events.send(ConnectionEvent::ConnClosed(addr));
        },
        _ = io::copy(&mut rhs_r, &mut lhs_w) => {
            debug!("Stopping transmission ({:?} <--> {:?}) lhs_w or rhs_r closed.", lhs, rhs);
            let _ = events.send(ConnectionEvent::ConnClosed(addr));
        },
    }
}

#[derive(Debug)]
pub struct Context {
    event_chan: (
        mpsc::UnboundedSender<ConnectionEvent>,
        mpsc::UnboundedReceiver<ConnectionEvent>,
    ),

    listener_stop_txs: Vec<oneshot::Sender<()>>,

    reaper_stop_tx: oneshot::Sender<()>,

    // ongoing active connections
    active_connections: HashMap<SocketAddr, ActiveConnection>,

    active_containers: HashMap<IpAddr, (usize, DeployedContainer)>,

    // sends containers to a worker queue to reap deployed containers after a timeout
    containers_to_reap: mpsc::UnboundedSender<SingleConsumer<DeployedContainer>>,

    // disconnected containers, contains the same receivers of disconnected containers
    // as `containers_to_reap` and is used to bring disconnected containers back alive
    disconnected_containers: MultiMap<IpAddr, ContainerID, SingleConsumer<DeployedContainer>>,
}

impl Context {
    pub fn new(listener_stop_txs: Vec<oneshot::Sender<()>>) -> (mpsc::UnboundedSender<ConnectionEvent>, Context) {
        let (evt_tx, evt_rx) = mpsc::unbounded_channel();
        let (reap_tx, reap_rx) = mpsc::unbounded_channel();
        let (reaper_stop_tx, reaper_stop_rx) = oneshot::channel();

        tokio::spawn(container_reaper(
            Duration::from_secs(OPTS.timeout as u64),
            evt_tx.clone(),
            reap_rx,
            reaper_stop_rx,
        ));

        let ctx = Context {
            event_chan: (evt_tx.clone(), evt_rx),
            listener_stop_txs,
            reaper_stop_tx,
            active_connections: HashMap::new(),
            active_containers: HashMap::new(),
            containers_to_reap: reap_tx,
            disconnected_containers: MultiMap::new(),
        };

        (evt_tx, ctx)
    }

    pub async fn handle_events(mut self) {
        while let Some(evt) = self.event_chan.1.recv().await {
            match evt {
                ConnectionEvent::ConnCreate(addr, port, socket) => {
                    if let Err(e) = self.create_connection_for(addr, port, socket).await {
                        error!(
                            "Failed creating connection for container from {} on port {}: {:?}",
                            addr, port, e
                        );
                    }
                }
                ConnectionEvent::ConnClosed(addr) => {
                    self.close_connection(addr).await;
                }
                ConnectionEvent::ContainerClosed(container_id) => {
                    self.disconnected_containers.remove_alt(&container_id);
                },
                ConnectionEvent::Stop => {
                    // oopsie
                    info!("Stopping, killing all connections and containers");

                    let _ = self.reaper_stop_tx.send(());

                    for closer in self.listener_stop_txs {
                        let _ = closer.send(());
                    }

                    for (_, conn) in self.active_connections.drain() {
                        let _ = conn.should_close.send(());
                    }

                    for (_, container) in self.active_containers.values() {
                        kill_container(&container.id).await;
                    }

                    for (_, (_, container_c)) in self.disconnected_containers.iter() {
                        if let Some(container) = container_c.take() {
                            kill_container(&container.id).await;
                        }
                    }

                    return;
                }
            }
        }
    }

    async fn create_connection_for(
        &mut self,
        client_addr: SocketAddr,
        port: u16,
        client_stream: TcpStream,
    ) -> Result<()> {
        if let Some(container) = self.disconnected_containers.remove(&client_addr.ip()) {
            if let Some(container) = container.take() {
                // removal of this container has now been cancelled, we can use it
                info!(
                    "Container for {} that was on the reap queue moved back to being alive.",
                    client_addr.ip()
                );
                self.active_containers
                    .insert(client_addr.ip(), (1, container.clone()));
                return self
                    .create_connection(client_addr, port, client_stream, container)
                    .await;
            }

            debug!(
                "Container for {} evicted before we could cancel eviction.",
                client_addr.ip()
            );
            // otherwise fall through to below and create a new container
        }

        let container =
            if let Some((count, container)) = self.active_containers.get_mut(&client_addr.ip()) {
                *count += 1;
                container.clone()
            } else {
                info!(
                    "Incoming initial connection from {}, creating new container.",
                    client_addr.ip()
                );
                let container = new_container().await.context(ContainerMgmt)?;
                self.active_containers
                    .insert(client_addr.ip(), (1, container.clone()));
                container
            };

        self.create_connection(client_addr, port, client_stream, container)
            .await
    }

    async fn create_connection(
        &mut self,
        client_addr: SocketAddr,
        port: u16,
        client_stream: TcpStream,
        container: DeployedContainer,
    ) -> Result<()> {
        let container_stream = container.connect(port).await.context(ContainerMgmt)?;

        let (connection, close_send) = ActiveConnection::new(container.clone());

        self.active_connections.insert(client_addr, connection);

        tokio::spawn(rx_tx_loop(
            client_addr,
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

        let active_container = self
            .active_containers
            .get_mut(&client_addr.ip())
            .expect("Container didn't exist but was being removed?");

        active_container.0 -= 1;

        if active_container.0 == 0 {
            // no more entries, move the container to the reap queue

            info!("Last connection to container {:?} gone, moving to reap queue", active_container.1.id);

            let active_container = self.active_containers.remove(&client_addr.ip()).unwrap();
            let wrapped_container = SingleConsumer::new(active_container.1.clone());

            self.containers_to_reap
                .send(wrapped_container.clone())
                .unwrap();
            self.disconnected_containers.insert(
                client_addr.ip(),
                active_container.1.id,
                wrapped_container,
            );
        }
    }
}
