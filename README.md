# Flowgate

Adaptive threshold rate controller for ML prediction pipelines. Sits between a prediction source and a downstream action system, dynamically tuning its score threshold via one of four algorithms — PID, fixed, buffered batch, or buffered streaming — to emit a predictable, controllable number of predictions per unit time.

```
[ML Producer] → NATS JetStream (flowgate.in) → [Flowgate] → NATS JetStream (flowgate.out) → [Downstream]
                                                    ↕
                                              NATS KV (flowgate-config)
```

The project includes a multi-client batch producer, a React dashboard for side-by-side algorithm comparison, and replica support for horizontal scaling.

## Key Features

- **PID-controlled threshold** — automatically adjusts based on actual vs target emission rate
- **Buffered admission** — `buffered_batch` (windowed best-of-N) and `buffered_streaming` (priority-queue drain) modes trade latency for higher output quality
- **Opaque payload passthrough** — only reads the `score` field; payload is never parsed
- **Live-tunable config** — all parameters stored in NATS KV, changes take effect within one PID tick
- **Hybrid cold start** — uses fallback threshold if configured, otherwise runs a percentile warmup
- **Instant fallback** — set `algorithm=fixed` in KV to immediately switch to a static threshold
- **Backpressure-aware drain** — `min_quality_score` quality floor and `backpressure_threshold_ms` for downstream pressure detection
- **Multi-client batch producer** — 70 simulated clients sending batches at compressed time intervals, configurable via NATS KV
- **React dashboard** — side-by-side comparison of two flowgate instances with live metrics, WebSocket backend, REST API for config
- **Replica support** — `deploy.replicas: 2` in docker-compose with shared durable consumers and metrics aggregation across replicas
- **Full observability** — Prometheus metrics for threshold, rate, PID terms, controller state, ingestion rate, backpressure, and producer health

## Message Format

Flowgate treats the message body as **completely opaque bytes** — it never inspects or parses the payload. The prediction score is carried in a NATS header:

```
NATS Headers:
  Flowgate-Score: 0.87

Body: <your data in any format — protobuf, msgpack, JSON, raw bytes, anything>
```

Flowgate reads the `Flowgate-Score` header to make the threshold decision. If the message passes, it is republished to the output subject with the original body and headers intact, plus two additional headers:

```
Flowgate-Threshold: 0.650000    (the threshold that was applied)
Flowgate-State: pid             (controller state: cold_start, warmup, pid, fixed)
```

This design means Flowgate works with any serialization format — your pipeline doesn't need to change its wire format to use it.

## Quick Start (Docker Compose)

```bash
docker compose up --build
```

This starts:
- **NATS** with JetStream enabled (10GB max storage, 10m stream max-age)
- **nats-setup** — creates streams, KV buckets, seeds default config
- **flowgate-threshold** (x2 replicas) — PID mode instance (metrics on `:9090`)
- **flowgate-buffered** (x2 replicas) — buffered_streaming mode instance (metrics on `:9091`)
- **producer** — multi-client batch producer (70 simulated clients, metrics on `:9090`)
- **dashboard** — React UI at http://localhost:3000

To include Prometheus:
```bash
docker compose --profile monitoring up --build
```

## Live Configuration

All parameters are stored in the `flowgate-config` KV bucket. Change them at any time:

```bash
# Change target emission rate to 20/sec
nats kv put flowgate-config target_rate 20

# Switch to fixed threshold mode
nats kv put flowgate-config algorithm fixed
nats kv put flowgate-config fallback_threshold 0.75

# Switch back to PID
nats kv put flowgate-config algorithm pid

# Tune PID gains
nats kv put flowgate-config kp 0.02
nats kv put flowgate-config ki 0.002

# View current config
nats kv ls flowgate-config
```

### Configuration Reference

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `target_rate` | float | 10.0 | Target emissions per second |
| `measurement_window_secs` | float | 10.0 | Window for measuring actual rate |
| `kp` | float | 0.0004 | PID proportional gain |
| `ki` | float | 0.00004 | PID integral gain |
| `kd` | float | 0.00001 | PID derivative gain |
| `fallback_threshold` | float | none | Fixed threshold for cold start and fallback |
| `min_threshold` | float | 0.0 | Lower bound for threshold |
| `max_threshold` | float | 1.0 | Upper bound for threshold |
| `algorithm` | string | pid | `pid`, `fixed`, `buffered_batch`, or `buffered_streaming` |
| `warmup_samples` | int | 100 | Samples needed for percentile warmup |
| `anti_windup_limit` | float | 100.0 | PID integral term clamp |
| `pid_interval_ms` | int | 1000 | PID tick interval in milliseconds |
| `max_buffer_duration_ms` | int | 5000 | Max time a message can sit in the buffer before eviction |
| `drain_interval_ms` | int | 100 | How often the buffer drainer pops the best message (streaming mode) |
| `min_quality_score` | float | 0.0 | Quality floor — drain skips if best buffered score is below this |
| `backpressure_threshold_ms` | int | 500 | Publish latency above which drain pauses to relieve downstream pressure |

## How the PID Controller Works

The controller maintains a target emission rate and adjusts the score threshold to achieve it:

- **Error** = `target_rate - actual_rate` (positive = emitting too few)
- **Proportional**: reacts to current error magnitude
- **Integral**: corrects for persistent bias (distribution drift)
- **Derivative**: dampens oscillation from bursty input

When error is positive (under-emitting), the threshold is lowered to allow more messages through. When negative (over-emitting), the threshold is raised.

### Cold Start Behavior

1. **With `fallback_threshold` set**: starts processing immediately using that threshold, transitions to PID after one measurement window
2. **Without `fallback_threshold`**: buffers messages, computes a percentile-based initial threshold after `warmup_samples` received, then transitions to PID

## Observability

Prometheus metrics are exposed on `:9090/metrics`:

### Flowgate Service Metrics

| Metric | Type |
|--------|------|
| `flowgate_messages_received_total` | counter |
| `flowgate_messages_emitted_total` | counter |
| `flowgate_messages_rejected_total` | counter |
| `flowgate_current_threshold` | gauge |
| `flowgate_target_rate` | gauge |
| `flowgate_actual_rate` | gauge |
| `flowgate_pid_error` | gauge |
| `flowgate_pid_p_term` | gauge |
| `flowgate_pid_i_term` | gauge |
| `flowgate_pid_d_term` | gauge |
| `flowgate_controller_state` | gauge (0=cold_start, 1=warmup, 2=pid, 3=fixed) |
| `flowgate_ingestion_rate` | gauge — current inbound message rate |
| `flowgate_drain_skipped_quality_total` | counter — drains skipped due to `min_quality_score` floor |
| `flowgate_drain_skipped_backpressure_total` | counter — drains skipped due to downstream backpressure |
| `flowgate_publish_latency_ms` | gauge — last publish round-trip time |

### Producer Metrics

| Metric | Type |
|--------|------|
| `producer_messages_published_total` | counter |
| `producer_publish_errors_total` | counter |
| `producer_batches_sent_total` | counter |
| `producer_active_clients` | gauge |
| `producer_backpressure` | gauge (0 or 1) |

## Producer

The producer simulates a realistic ML inference pipeline: 70 clients sending prediction batches at compressed time intervals. Each client generates a batch of 100-5000 predictions with scores drawn from a configurable distribution, then publishes them to NATS JetStream. All parameters are live-tunable via the `flowgate-producer-config` KV bucket.

### Producer Config (KV bucket: `flowgate-producer-config`)

| Key | Default | Description |
|-----|---------|-------------|
| `num_clients` | 70 | Number of simulated clients sending batches |
| `time_compression` | 60 | Time compression factor (60 = 1 hour of real traffic in 1 minute) |
| `min_batch_size` | 100 | Minimum predictions per batch |
| `max_batch_size` | 5000 | Maximum predictions per batch |
| `distribution` | beta | Score distribution: `normal`, `beta`, `uniform`, `bimodal` |
| `distribution_variance` | 0.3 | Controls spread of the score distribution |

The producer exposes Prometheus metrics on its own `:9090` endpoint and reports backpressure status (surfaced in the dashboard with a red banner when `producer_backpressure` = 1).

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## License

MIT
