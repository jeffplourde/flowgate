# Development Guide

## Prerequisites

- Rust 1.88+ (async-nats 0.48 requires it)
- Node.js 22+ and npm (for the React dashboard)
- Docker and Docker Compose (for the demo environment)

## Local Development

### Building
```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

### Running Locally (without Docker)
You need a NATS server with JetStream:
```bash
nats-server -js
```

Then run the setup script to create streams and KV:
```bash
NATS_URL=nats://localhost:4222 ./scripts/setup-nats.sh
```

Run the service:
```bash
cargo run --bin flowgate -- --nats-url nats://localhost:4222
```

Run the producer:
```bash
cargo run --bin flowgate-producer -- --nats-url nats://localhost:4222 --rate 100
```

### Dashboard Development

For fast UI iteration, run the Vite dev server instead of rebuilding Docker:

```bash
cd dashboard
npm install
npm run dev
```

The dev server runs on http://localhost:5173. You'll need the dashboard backend running separately for the WebSocket and API endpoints. You can either run it from Docker or locally:

```bash
cargo run --bin flowgate-dashboard -- \
  --nats-url nats://localhost:4222 \
  --static-dir dashboard/dist
```

## Project Structure

```
flowgate/
├── CLAUDE.md                    # Project context for Claude Code sessions
├── Cargo.toml                   # Workspace root
├── Cargo.lock
├── Dockerfile                   # Multi-stage: builder, dashboard-builder, 3 runtime targets
├── docker-compose.yml           # Full demo: 2 flowgate instances (2 replicas each), producer, dashboard
├── .dockerignore
├── .gitignore
├── nats-server.conf             # JetStream-enabled NATS config
├── prometheus.yml               # Scrape config for both instances + producer
├── scripts/
│   └── setup-nats.sh            # Creates streams (10m max-age), KV buckets, seeds config
├── crates/
│   ├── flowgate-service/        # Core service
│   │   └── src/
│   │       ├── main.rs          # CLI, wiring, shutdown
│   │       ├── config.rs        # Config struct, KV watcher (incl. min_quality_score, backpressure_threshold_ms)
│   │       ├── pid.rs           # PID controller (pure math)
│   │       ├── threshold.rs     # State machine, CheckResult, backpressure-aware drain
│   │       ├── buffer.rs        # BinaryHeap message buffer
│   │       ├── pipeline.rs      # Consumer, drainer (quality floor + backpressure), ticker tasks
│   │       ├── envelope.rs      # NATS header extraction
│   │       └── metrics.rs       # Prometheus definitions (incl. ingestion_rate, drain_skipped_*, publish_latency)
│   ├── flowgate-producer/       # Multi-client batch producer
│   │   └── src/main.rs          # 70 simulated clients, KV-configurable, Prometheus metrics
│   └── flowgate-dashboard/      # Dashboard backend
│       └── src/
│           ├── main.rs          # Axum server setup (incl. PRODUCER_METRICS env)
│           ├── ws.rs            # WebSocket + NATS subscribers + metrics scraper + replica DNS aggregation
│           └── api.rs           # REST API for config/metrics (both flowgate and producer KV)
├── dashboard/                   # React frontend
│   ├── package.json
│   ├── vite.config.ts
│   └── src/
│       ├── App.tsx              # Main layout with backpressure banner
│       ├── types.ts             # TypeScript interfaces
│       ├── hooks/
│       │   └── useFlowgateSocket.ts  # WebSocket state management
│       └── components/
│           ├── InstancePanel.tsx      # Per-instance display
│           ├── TimeSeriesChart.tsx    # Recharts line chart
│           ├── ScoreHistogram.tsx     # Score distribution bar chart
│           ├── MetricGauge.tsx        # Big number display
│           └── ControlPanel.tsx       # Config controls
└── docs/
    ├── architecture.md          # System design, replicas, producer, backpressure
    ├── analytical-model.md      # PID math and simulation
    ├── tuning-guide.md          # Practical tuning, quality floor, backpressure tuning
    ├── operator-runbook.md      # Operating procedures, producer monitoring, replica management
    └── development.md           # This file
```

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `async-nats` 0.48 | NATS client with JetStream and KV |
| `tokio` | Async runtime |
| `metrics` + `metrics-exporter-prometheus` 0.18 | Prometheus metrics |
| `tracing` + `tracing-subscriber` | Structured logging |
| `clap` | CLI argument parsing |
| `axum` 0.8 | HTTP + WebSocket for dashboard |
| `reqwest` | Metrics scraping in dashboard |
| `recharts` | React charting library |

## Testing

### Unit Tests
```bash
cargo test --workspace
```

Tests cover:
- PID controller behavior (convergence, anti-windup, bounds)
- Message buffer operations (drain, evict, ordering)
- Config parsing and KV entry application
- Threshold state machine transitions
- NATS header extraction

### Integration Testing
No automated integration tests yet. Manual verification via Docker Compose:
1. `docker compose up --build`
2. Check metrics endpoints
3. Verify PID convergence via dashboard
4. Test live config changes via KV

### Docker Build
```bash
docker compose build
```

Multi-stage Dockerfile:
1. `builder` — Rust compilation (all three binaries)
2. `dashboard-builder` — Node.js, npm ci, Vite build
3. `flowgate-service` — lean Debian + flowgate binary
4. `flowgate-producer` — lean Debian + producer binary
5. `flowgate-dashboard` — lean Debian + dashboard binary + React dist

## Environment Variables

### flowgate-service
| Var | Default | Description |
|-----|---------|-------------|
| `NATS_URL` | `nats://localhost:4222` | NATS connection URL |
| `METRICS_PORT` | `9090` | Prometheus metrics port |
| `NATS_KV_BUCKET` | `flowgate-config` | KV bucket for config |
| `OUTPUT_SUBJECT` | `flowgate.out` | Where to publish emitted messages |
| `OUTPUT_STREAM` | `FLOWGATE_OUT` | JetStream stream for output |
| `CONSUMER_NAME` | `flowgate-{bucket}` | Durable consumer name |

### flowgate-producer
| Var | Default | Description |
|-----|---------|-------------|
| `NATS_URL` | `nats://localhost:4222` | NATS connection URL |
| `SUBJECT` | `flowgate.in.synthetic` | Publish subject |
| `NATS_KV_BUCKET` | `flowgate-producer-config` | KV bucket for live-tunable producer config |
| `METRICS_PORT` | `9090` | Prometheus metrics port |

Producer behavior is configured via the `flowgate-producer-config` KV bucket (not env vars):

| KV Key | Default | Description |
|--------|---------|-------------|
| `num_clients` | `70` | Number of simulated clients sending batches |
| `time_compression` | `60` | Time compression factor (60 = 1h of traffic in 1m) |
| `min_batch_size` | `100` | Minimum predictions per batch |
| `max_batch_size` | `5000` | Maximum predictions per batch |
| `distribution` | `beta` | Score distribution: `normal`, `beta`, `uniform`, `bimodal` |
| `distribution_variance` | `0.3` | Controls spread of the score distribution |

### flowgate-dashboard
| Var | Default | Description |
|-----|---------|-------------|
| `NATS_URL` | `nats://localhost:4222` | NATS connection URL |
| `PORT` | `3000` | HTTP server port |
| `FLOWGATE_A_METRICS` | `http://localhost:9090` | Instance A metrics URL (dashboard resolves DNS to all replica IPs) |
| `FLOWGATE_B_METRICS` | `http://localhost:9091` | Instance B metrics URL (dashboard resolves DNS to all replica IPs) |
| `PRODUCER_METRICS` | `http://producer:9090` | Producer metrics URL |
| `KV_BUCKET_A` | `flowgate-config-a` | KV bucket for instance A |
| `KV_BUCKET_B` | `flowgate-config-b` | KV bucket for instance B |
| `KV_BUCKET_PRODUCER` | `flowgate-producer-config` | KV bucket for producer config |
| `STATIC_DIR` | `./dashboard/dist` | React build output directory |

## JetStream Configuration

The `setup-nats.sh` script creates streams with:
- **Max age**: 10 minutes — messages older than 10m are automatically discarded
- Streams: `FLOWGATE_IN`, `FLOWGATE_OUT_A`, `FLOWGATE_OUT_B`
- KV buckets: `flowgate-config-a`, `flowgate-config-b`, `flowgate-producer-config`

## Replica Behavior in Development

When running `docker compose up`, each flowgate service starts with 2 replicas. The replicas share a durable consumer name so messages are load-balanced by JetStream.

For local development without Docker, you typically run a single instance. The replica behavior is only relevant in the Docker Compose environment.

To test with different replica counts:
```bash
docker compose up -d --scale flowgate-threshold=4 --scale flowgate-buffered=4
```

The dashboard automatically discovers all replicas via DNS and aggregates their metrics.

## Future Work

- Vite dev proxy config for live UI development against Docker backend
- Integration test harness (replay recorded distributions, assert rate stability)
- K8s manifests (Deployment, Service, HPA)
- Horizontal scaling with partitioned consumers
- OpenTelemetry tracing
