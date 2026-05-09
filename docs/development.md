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
├── docker-compose.yml           # Full demo: 2 flowgate instances, producer, dashboard
├── .dockerignore
├── .gitignore
├── nats-server.conf             # JetStream-enabled NATS config
├── prometheus.yml               # Scrape config for both instances
├── scripts/
│   └── setup-nats.sh            # Creates streams, KV buckets, seeds config
├── crates/
│   ├── flowgate-service/        # Core service
│   │   └── src/
│   │       ├── main.rs          # CLI, wiring, shutdown
│   │       ├── config.rs        # Config struct, KV watcher
│   │       ├── pid.rs           # PID controller (pure math)
│   │       ├── threshold.rs     # State machine, CheckResult
│   │       ├── buffer.rs        # BinaryHeap message buffer
│   │       ├── pipeline.rs      # Consumer, drainer, ticker tasks
│   │       ├── envelope.rs      # NATS header extraction
│   │       └── metrics.rs       # Prometheus definitions
│   ├── flowgate-producer/       # Synthetic data generator
│   │   └── src/main.rs
│   └── flowgate-dashboard/      # Dashboard backend
│       └── src/
│           ├── main.rs          # Axum server setup
│           ├── ws.rs            # WebSocket + NATS subscribers + metrics scraper
│           └── api.rs           # REST API for config/metrics
├── dashboard/                   # React frontend
│   ├── package.json
│   ├── vite.config.ts
│   └── src/
│       ├── App.tsx              # Main layout
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
    ├── architecture.md          # System design
    ├── analytical-model.md      # PID math and simulation
    ├── tuning-guide.md          # Practical tuning
    ├── operator-runbook.md      # Operating procedures
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
| `DISTRIBUTION` | `beta` | Score distribution type |
| `RATE` | `100` | Messages per second |
| `BURST_INTERVAL` | `0` | Seconds between bursts (0 = disabled) |
| `BURST_MULTIPLIER` | `5` | Rate multiplier during burst |
| `BURST_DURATION` | `2` | Burst length in seconds |

### flowgate-dashboard
| Var | Default | Description |
|-----|---------|-------------|
| `NATS_URL` | `nats://localhost:4222` | NATS connection URL |
| `PORT` | `3000` | HTTP server port |
| `FLOWGATE_A_METRICS` | `http://localhost:9090` | Instance A metrics URL |
| `FLOWGATE_B_METRICS` | `http://localhost:9091` | Instance B metrics URL |
| `KV_BUCKET_A` | `flowgate-config-a` | KV bucket for instance A |
| `KV_BUCKET_B` | `flowgate-config-b` | KV bucket for instance B |
| `STATIC_DIR` | `./dashboard/dist` | React build output directory |

## Future Work

- Vite dev proxy config for live UI development against Docker backend
- Integration test harness (replay recorded distributions, assert rate stability)
- K8s manifests (Deployment, Service, HPA)
- Horizontal scaling with partitioned consumers
- OpenTelemetry tracing
- Configurable producer parameters via dashboard UI
