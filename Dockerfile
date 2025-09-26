FROM rust:1.89-alpine3.21 AS builder
WORKDIR /usr/src/myapp
RUN apk add --no-cache openssl-dev openssl-libs-static pkgconfig musl-dev
COPY server/src server/src
COPY server/Cargo.toml server/Cargo.toml
COPY server/Cargo.lock server/Cargo.lock
COPY interface interface
RUN cargo install --path server

FROM alpine:3.21
RUN apk add --no-cache tzdata
ENV TZ=Europe/Vienna
ENV RUST_LOG=info
COPY server/config.toml /data/config.toml
COPY --from=builder /usr/local/cargo/bin/public-transport-server /usr/local/bin/server
ENTRYPOINT ["server", "/data/config.toml"]