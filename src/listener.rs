use std::net::Ipv4Addr;

use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};

use log::{error, info};

use futures::future::FutureExt;
use futures::pin_mut;
use futures::select;

use crate::connections::ConnectionEvent;

pub async fn listen_on(
    port: u16,
    events: mpsc::UnboundedSender<ConnectionEvent>,
    stop: oneshot::Receiver<()>,
) {
    let mut listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await.unwrap();

    info!("Starting listener on port {}", port);

    let mut stop = FutureExt::fuse(stop);

    loop {
        let listener_accept = listener.accept().fuse();
        pin_mut!(listener_accept);

        select! {
            _ = stop => {
                info!("Stopping listener on port {}", port);
                return;
            },
            r = listener_accept => {
                match r {
                    Ok((socket, addr)) => {
                        info!("Incoming connection from {} on port {}", addr, port);
                        let _ = events.send(ConnectionEvent::ConnCreate(addr, port, socket));
                    }
                    Err(e) => error!("Failed connecting to client on port {}: {:?}", port, e),
                }
            }
        }
    }
}
