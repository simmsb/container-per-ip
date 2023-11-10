use std::time::Duration;

use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use udpflow::UdpStreamLocal;

use crate::container_mgmt::DeployedContainer;

pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}
impl<T: AsyncRead + AsyncWrite> AsyncReadWrite for T {}

pub type ClientConnection = Box<dyn CreateConnection + Send + Sync + Unpin + 'static>;
pub type PlainConnection = Box<dyn AsyncReadWrite + Send + Sync + Unpin + 'static>;

#[async_trait::async_trait]
pub trait CreateConnection: AsyncRead + AsyncWrite {
    async fn create_connection(
        &self,
        port: u16,
        container: &DeployedContainer,
    ) -> miette::Result<PlainConnection>;

    fn into_connection(self: Box<Self>) -> PlainConnection;
}

#[async_trait::async_trait]
impl CreateConnection for TcpStream {
    async fn create_connection(
        &self,
        port: u16,
        container: &DeployedContainer,
    ) -> miette::Result<PlainConnection> {
        let max_tries = 10;
        let mut tries = 0;

        loop {
            tracing::trace!("Trying to connect to {:?} on port {}", container.id, port);

            match container.connect_tcp(port).await {
                Ok(s) => return Ok(Box::new(s)),
                Err(e) if tries == max_tries => {
                    return Err(e.into());
                }
                _ => (),
            }

            tries += 1;

            tokio::time::sleep(Duration::from_millis(200 + 200 * tries)).await
        }
    }

    fn into_connection(self: Box<Self>) -> PlainConnection {
        self as _
    }
}

#[async_trait::async_trait]
impl CreateConnection for UdpStreamLocal {
    async fn create_connection(
        &self,
        port: u16,
        container: &DeployedContainer,
    ) -> miette::Result<PlainConnection> {
        let s = container.connect_udp(port).await?;

        // try to let the container boot
        tokio::time::sleep(Duration::from_secs(1)).await;

        Ok(Box::new(s))
    }

    fn into_connection(self: Box<Self>) -> PlainConnection {
        self as _
    }
}
