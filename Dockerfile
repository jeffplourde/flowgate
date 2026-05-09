FROM rust:1.88-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/

RUN cargo build --release --workspace

FROM node:22-slim AS dashboard-builder

WORKDIR /build
COPY dashboard/package.json dashboard/package-lock.json ./
RUN npm ci
COPY dashboard/ ./
RUN npm run build

FROM debian:bookworm-slim AS flowgate-service

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/flowgate /usr/local/bin/flowgate

EXPOSE 9090
ENTRYPOINT ["flowgate"]

FROM debian:bookworm-slim AS flowgate-producer

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/flowgate-producer /usr/local/bin/flowgate-producer

ENTRYPOINT ["flowgate-producer"]

FROM debian:bookworm-slim AS flowgate-dashboard

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/flowgate-dashboard /usr/local/bin/flowgate-dashboard
COPY --from=dashboard-builder /build/dist /app/dashboard/dist

WORKDIR /app
EXPOSE 3000
ENTRYPOINT ["flowgate-dashboard", "--static-dir", "/app/dashboard/dist"]
