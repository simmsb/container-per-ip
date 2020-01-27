FROM clux/muslrust:latest AS build

WORKDIR /opt/container-per-ip/

COPY Cargo.toml .
COPY Cargo.lock .
RUN mkdir .cargo
RUN cargo vendor > .cargo/config

COPY src src

RUN cargo build --release
RUN cargo install --path . --verbose

FROM docker:latest

COPY --from=build /root/.cargo/bin/container-per-ip /usr/local/bin/container-per-ip

ENTRYPOINT ["container-per-ip"]
