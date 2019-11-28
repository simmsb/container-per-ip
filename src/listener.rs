use tokio::net::TcpListener;
use tokio::sync::mpsc;

use log::error;

use crate::connections::ConnectionEvent;

pub async fn listen_on(port: u16, mut events: mpsc::UnboundedSender<ConnectionEvent>) {
    let mut listener = TcpListener::bind(("0.0.0.0", port)).await.unwrap();

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                let _ = events.try_send(ConnectionEvent::ConnCreate(addr, port, socket));
            }
            Err(e) => error!("Failed connecting to client on port {}: {:?}", port, e),
        }
    }
}
