# container-per-ip

![Rust](https://github.com/nitros12/container-per-ip/workflows/Rust/badge.svg)

Some thing so you can spin up a container per client ip

This works by spawning a new container based on the host address of a connecting
client.

Concurrent tcp connections route to the same container, containers are removed
when a timeout has elapsed since the last connection closed.

```
container-per-ip 0.1.2
Ben Simms <ben@bensimms.moe>
Run a container per client ip

USAGE:
    container-per-ip [FLAGS] [OPTIONS] <image>

FLAGS:
    -h, --help          Prints help information
        --privileged    Should the containers be started with the `--privileged` flag
    -V, --version       Prints version information

OPTIONS:
    -b, --binds <binds>...     Volume bindings to provide to containers
    -p, --ports <ports>...     Ports to listen on (tcp only currently)
    -t, --timeout <timeout>    Timeout (seconds) after an IPs last connection disconnects before killing the associated
                               container [default: 300]

ARGS:
    <image>    The docker image to run for each ip
```

## Running in docker

```
docker run --rm -v /var/run/docker.sock:/var/run/docker.sock --net=host -it nitros12/container-per-ip --help
```

## Building the docker image

``` shell
env DOCKER_BUILDKIT=1 docker build -f Cargo.toml -t nitros12/container-per-ip:latest .
```
