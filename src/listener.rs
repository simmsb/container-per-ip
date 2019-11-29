use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use log::{error, info};

use futures::future::FutureExt;
use futures::select;

use crate::connections::ConnectionEvent;

pub async fn listen_on(
    port: u16,
    mut events: mpsc::UnboundedSender<ConnectionEvent>,
    stop: oneshot::Receiver<()>,
) {
    let mut listener = TcpListener::bind(("0.0.0.0", port)).await.unwrap();

    info!("Starting listener on port {}", port);

    let mut stop = FutureExt::fuse(stop);

    loop {
        select! {
            _ = stop => {
                info!("Stopping listener on port {}", port);
                return;
            },
            r = FutureExt::fuse(listener.accept()) => {
                match r {
                    Ok((socket, addr)) => {
                        info!("Incoming connection from {} on port {}", addr, port);
                        let _ = events.try_send(ConnectionEvent::ConnCreate(addr, port, socket));
                    }
                    Err(e) => error!("Failed connecting to client on port {}: {:?}", port, e),
                }
            }
        }
    }
}
