FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --workspace

FROM debian:bookworm-slim AS flowgate-service

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/flowgate /usr/local/bin/flowgate

EXPOSE 9090
ENTRYPOINT ["flowgate"]

FROM debian:bookworm-slim AS flowgate-producer

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/flowgate-producer /usr/local/bin/flowgate-producer

ENTRYPOINT ["flowgate-producer"]
