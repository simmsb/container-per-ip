# container-per-ip

![Rust](https://github.com/nitros12/container-per-ip/workflows/Rust/badge.svg)

Some thing so you can spin up a container per client ip

This works by spawning a new container based on the host address of a connecting
client.

Concurrent tcp connections route to the same container, containers are removed
when a timeout has elapsed since the last connection closed.

```
container-per-ip 
Ben Simms <ben@bensimms.moe>
Run a container per client ip

USAGE:
    container-per-ip [OPTIONS] <IMAGE>

ARGS:
    <IMAGE>    The docker image to run for each ip

OPTIONS:
    -b, --binds <BINDS>        Volume bindings to provide to containers
    -e, --env <ENV>            Environment variables to set on the child container
    -h, --help                 Print help information
        --network <NETWORK>    
    -p, --ports <PORTS>        Ports to listen on (tcp only currently)
        --privileged           Should the containers be started with the `--privileged` flag
    -t, --timeout <TIMEOUT>    Timeout (seconds) after an IPs last connection disconnects before
                               killing the associated container [default: 300]
```

## Running in docker

```
docker run --rm -v /var/run/docker.sock:/var/run/docker.sock --net=host -it nitros12/container-per-ip --help
```
