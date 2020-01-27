FROM clux/muslrust:latest AS build

RUN cargo install --git https://github.com/nitros12/container-per-ip

FROM docker:latest

COPY --from=build /root/.cargo/bin/container-per-ip /usr/local/bin/container-per-ip

ENTRYPOINT ["container-per-ip"]
