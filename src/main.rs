use snafu::Snafu;
use structopt::StructOpt;

use tokio::prelude::*;

#[derive(Debug, Snafu)]
enum Error {}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, StructOpt)]
#[structopt(name = "container-per-ip", about = "Run a container per client ip")]
struct Opt {
    #[structopt()]
    /// The docker image to run for each ip
    image: String,

    #[structopt(long)]
    /// Should the containers be started with the `--privileged` flag
    privileged: bool,

    #[structopt(short, long)]
    /// Ports to listen on (tcp only currently)
    ports: Vec<u64>,

    #[structopt(long)]
    /// Timeout (seconds) after an IPs last connection disconnects before
    /// killing the associated container
    timeout: Option<f64>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Opt::from_args();
    println!("{:?}", opt);

    Ok(())
}
