use std::net::Ipv4Addr;

use tokio::net::TcpListener;
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use tracing::{error, info};

use crate::connections::ConnectionEvent;

#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum Error {
    #[error("Couldn't bind to the port: {}", port)]
    BindError {
        port: u16,

        #[source]
        source: tokio::io::Error,
    },
}

type Result<T, E = Error> = std::result::Result<T, E>;

pub async fn listen_on(
    port: u16,
    events: mpsc::UnboundedSender<ConnectionEvent>,
    mut stop: oneshot::Receiver<()>,
) -> Result<JoinHandle<()>> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))
        .await
        .map_err(|e| Error::BindError { port, source: e })?;

    Ok(tokio::spawn(async move {
        info!("Starting listener on port {}", port);

        loop {
            select! {
                _ = &mut stop => {
                    info!("Stopping listener on port {}", port);
                    return;
                },
                r = listener.accept() => {
                    match r {
                        Ok((socket, addr)) => {
                            info!("Incoming connection from {} on port {}", addr, port);
                            crate::trace_error!(events.send(ConnectionEvent::ConnCreate(addr, port, socket)));
                        }
                        Err(e) => error!("Failed connecting to client on port {}: {}", port, e),
                    }
                }
            }
        }
    }))
}
