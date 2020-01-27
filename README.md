# container-per-ip
Some thing so you can spin up a container per client ip

This works by spawning a new container based on the host address of a connecting
client.

Concurrent tcp connections route to the same container, containers are removed
when a timeout has elapsed since the last connection closed.

```
container-per-ip 0.1.0
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
# build (if needed)
docker build -t container-per-ip .

# run
docker run --rm -v /var/run/docker.sock:/var/run/docker.sock --net=host -it container-per-ip --help
```
