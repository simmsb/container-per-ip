use std::fmt::Debug;
use std::net::Ipv4Addr;

use tokio::net::{TcpListener, UdpSocket};
use tokio::select;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use tracing::{error, info};
use udpflow::{UdpListener, UdpStreamLocal};

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

pub async fn listen_on_tcp(
    ext: u16,
    int: u16,
    events: mpsc::UnboundedSender<ConnectionEvent>,
    mut stop: oneshot::Receiver<()>,
) -> Result<JoinHandle<()>> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, ext))
        .await
        .map_err(|e| Error::BindError {
            port: ext,
            source: e,
        })?;

    Ok(tokio::spawn(async move {
        info!("Starting tcp listener on port {}:{}", ext, int);

        loop {
            select! {
                _ = &mut stop => {
                    info!("Stopping listener on port {}:{}", ext,int);
                    return;
                },
                r = listener.accept() => {
                    match r {
                        Ok((socket, addr)) => {
                            info!("Incoming tcp connection from {} on port {}:{}", addr, ext, int);
                            crate::trace_error!(events.send(ConnectionEvent::TcpConnCreate(addr, int, socket)));
                        }
                        Err(e) => error!("Failed connecting to client on port {}:{}: {}", ext, int, e),
                    }
                }
            }
        }
    }))
}

pub struct UdpStream(pub UdpStreamLocal);

impl Debug for UdpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UdpStream").finish()
    }
}

pub async fn listen_on_udp(
    ext: u16,
    int: u16,
    events: mpsc::UnboundedSender<ConnectionEvent>,
    mut stop: oneshot::Receiver<()>,
) -> Result<JoinHandle<()>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, ext))
        .await
        .map_err(|e| Error::BindError {
            port: ext,
            source: e,
        })?;
    let listener = UdpListener::new(socket);

    Ok(tokio::spawn(async move {
        info!("Starting udp listener on port {}:{}", ext, int);

        let mut buf = vec![0u8; 8192];

        loop {
            select! {
                _ = &mut stop => {
                    info!("Stopping listener on port {}:{}", ext,int);
                    return;
                },
                r = listener.accept(&mut buf) => {
                    match r {
                        Ok((socket, addr)) => {
                            info!("Incoming udp connection from {} on port {}:{}", addr, ext, int);
                            crate::trace_error!(events.send(ConnectionEvent::UdpConnCreate(addr, int, UdpStream(socket))));
                        }
                        Err(e) => error!("Failed connecting to client on port {}:{}: {}", ext, int, e),
                    }
                }
            }
        }
    }))
}
