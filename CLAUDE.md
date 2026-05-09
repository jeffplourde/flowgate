# Flowgate

Adaptive threshold rate controller for ML prediction pipelines, built in Rust with NATS JetStream.

## Project Overview

Flowgate sits between an ML prediction source and a downstream action system. It controls the rate of emitted predictions using one of four algorithms (PID, fixed, buffered_batch, buffered_streaming), all dynamically configurable via NATS KV at runtime.

The project now includes:
- **Multi-client batch producer** — 70 simulated clients sending batches at compressed time intervals, configurable via `flowgate-producer-config` KV bucket
- **React dashboard** — side-by-side comparison of two flowgate instances with live WebSocket streaming, Prometheus metrics scraping, and REST API for config changes
- **Replica support** — `deploy.replicas: 2` in docker-compose; dashboard resolves all replica IPs and aggregates metrics (sum for counters, avg for gauges)
- **Backpressure-aware drain** — `min_quality_score` quality floor prevents emitting low-quality predictions; `backpressure_threshold_ms` pauses drain when downstream publish latency is high
- **Producer metrics** — Prometheus endpoint with published/errors/batches/backpressure gauges, surfaced in dashboard with a red backpressure banner
- **Ingestion rate tracking** — `flowgate_ingestion_rate` gauge displayed in the dashboard

## Architecture

- **flowgate-service** (`crates/flowgate-service/`) — the core service binary. Consumes from `FLOWGATE_IN` JetStream, applies threshold/buffer logic, publishes to a configurable output subject.
- **flowgate-producer** (`crates/flowgate-producer/`) — synthetic prediction generator for demos. Configurable distribution, rate, and burst patterns.
- **flowgate-dashboard** (`crates/flowgate-dashboard/`) — axum-based backend serving a React UI. Subscribes to output streams, scrapes Prometheus metrics, proxies KV config changes.
- **dashboard/** — React + TypeScript + Vite frontend. Side-by-side comparison of two flowgate instances.

## Key Design Decisions

- **Opaque payloads**: Flowgate never parses message bodies. The prediction score is carried in the `Flowgate-Score` NATS header. This lets any wire format flow through unchanged.
- **PID controller for threshold mode**: a control-theory approach to adaptive thresholding. The plant gain `λ·f(θ*)` varies with input rate and score distribution, so PID gains must be tuned per-deployment.
- **Buffered admission**: two modes (batch and streaming) that trade latency for quality by holding messages and emitting only the best. During bursts, buffered streaming shows notably higher average emitted scores because the buffer has a denser pool of candidates.
- **Backpressure-aware drain**: the drain loop checks publish latency against `backpressure_threshold_ms` and skips emission when downstream is slow. A separate `min_quality_score` floor prevents emitting predictions below a quality threshold.
- **All config in NATS KV**: every parameter is live-tunable. The service watches for changes and applies them within one tick. Per-instance KV bucket, output subject, and consumer name are set via env vars.
- **Metrics aggregation**: the dashboard resolves Docker/Compose service DNS names to all replica IPs and aggregates their Prometheus metrics (sum counters, average gauges) for a unified view.

## Build & Test

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Running the Demo

```bash
docker compose up --build    # starts nats, two flowgate instances, producer, dashboard
# UI at http://localhost:3000
# Metrics at :9090 (threshold instance) and :9091 (buffered instance)
```

## Project Structure

```
crates/flowgate-service/src/
  main.rs       — CLI args, wiring, graceful shutdown
  config.rs     — Config struct, KV watcher, live updates via tokio::watch
  pid.rs        — Pure PID controller (no I/O)
  threshold.rs  — ThresholdController state machine, CheckResult enum
  buffer.rs     — MessageBuffer (BinaryHeap) with drain/evict logic
  pipeline.rs   — JetStream consumer, buffer drainer, PID ticker
  envelope.rs   — NATS header extraction (Flowgate-Score)
  metrics.rs    — Prometheus metric definitions and helpers
```

## Conventions

- Score is always in the `Flowgate-Score` NATS header, never in the message body
- Config keys in NATS KV are snake_case strings with string values
- The service supports per-instance KV bucket, output subject, and consumer name via env vars
- Two instances run side-by-side in the demo: `flowgate-config-a` (PID mode) and `flowgate-config-b` (buffered_streaming mode)
- Each flowgate service runs with `deploy.replicas: 2`; consumers share a durable name so messages are load-balanced
- Producer config lives in the `flowgate-producer-config` KV bucket (keys: `num_clients`, `time_compression`, `min_batch_size`, `max_batch_size`, `distribution`, `distribution_variance`)
- JetStream streams use 10m max-age to keep storage bounded

## Documentation

See `docs/` for detailed guides:
- `architecture.md` — system design, data flow, component details
- `analytical-model.md` — PID math, stability analysis, simulation
- `tuning-guide.md` — practical PID tuning, algorithm selection
- `operator-runbook.md` — operating procedures, emergency fallback
- `development.md` — dev setup, project structure, contribution workflow
