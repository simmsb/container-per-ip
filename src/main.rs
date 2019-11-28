use bollard::Docker;
use snafu::{ResultExt, Snafu};
use structopt::StructOpt;
use lazy_static::lazy_static;

mod connections;
mod container_mgmt;
mod single_consumer;

lazy_static! {
    static ref DOCKER: Docker =
        Docker::connect_with_local_defaults().context(DockerError).unwrap();

    static ref OPTS: Opt = Opt::from_args();
}

#[derive(Debug, Snafu)]
enum Error {
    #[snafu(display("An error occured with docker: {}", source))]
    DockerError { source: bollard::errors::Error },
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, StructOpt)]
#[structopt(name = "container-per-ip", about = "Run a container per client ip")]
pub struct Opt {
    #[structopt()]
    /// The docker image to run for each ip
    pub image: String,

    #[structopt(long)]
    /// Should the containers be started with the `--privileged` flag
    pub privileged: bool,

    #[structopt(short, long)]
    /// Ports to listen on (tcp only currently)
    pub ports: Vec<u16>,

    #[structopt(long)]
    /// Timeout (seconds) after an IPs last connection disconnects before
    /// killing the associated container
    pub timeout: Option<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("{:?}", *OPTS);

    let version = DOCKER.version().await.context(DockerError)?;

    println!("Docker version: {:?}", version);

    Ok(())
}
