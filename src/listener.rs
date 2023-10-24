use std::fmt::Debug;
use std::net::Ipv4Addr;

use coerce::actor::LocalActorRef;
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;

use tracing::{error, info};
use udpflow::UdpListener;

use crate::coordinator::{Connect, Coordinator};

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
    coordinator: LocalActorRef<Coordinator>,
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
            match listener.accept().await {
                Ok((socket, addr)) => {
                    info!(
                        "Incoming tcp connection from {} on port {}:{}",
                        addr, ext, int
                    );
                    coordinator
                        .send(Connect {
                            client_addr: addr,
                            port: int,
                            connection: Box::new(socket),
                        })
                        .await
                        .unwrap();
                }
                Err(e) => error!("Failed connecting to client on port {}:{}: {}", ext, int, e),
            }
        }
    }))
}

pub async fn listen_on_udp(
    ext: u16,
    int: u16,
    coordinator: LocalActorRef<Coordinator>,
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
            match listener.accept(&mut buf).await {
                Ok((socket, addr)) => {
                    info!(
                        "Incoming udp connection from {} on port {}:{}",
                        addr, ext, int
                    );
                    coordinator
                        .send(Connect {
                            client_addr: addr,
                            port: int,
                            connection: Box::new(socket),
                        })
                        .await
                        .unwrap();
                }
                Err(e) => error!("Failed connecting to client on port {}:{}: {}", ext, int, e),
            }
        }
    }))
}
