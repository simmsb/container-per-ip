FROM lukemathwalker/cargo-chef as planner
WORKDIR app
COPY . .
RUN cargo chef prepare  --recipe-path recipe.json

FROM lukemathwalker/cargo-chef as cacher
WORKDIR app
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM clux/muslrust as builder
WORKDIR /app
COPY . .
# Copy over the cached dependencies
COPY --from=cacher /app/target target
COPY --from=cacher $CARGO_HOME $CARGO_HOME
RUN cargo build --release --bin container-per-ip

FROM scratch as runtime
WORKDIR app
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/container-per-ip /container-per-ip
ENTRYPOINT ["/container-per-ip"]
