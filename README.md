# Flowgate

Adaptive threshold rate controller for ML prediction pipelines. Sits between a prediction source and a downstream action system, dynamically tuning its score threshold via a PID controller to emit a predictable, controllable number of predictions per unit time.

```
[ML Producer] → NATS JetStream (flowgate.in) → [Flowgate] → NATS JetStream (flowgate.out) → [Downstream]
                                                    ↕
                                              NATS KV (flowgate-config)
```

## Key Features

- **PID-controlled threshold** — automatically adjusts based on actual vs target emission rate
- **Opaque payload passthrough** — only reads the `score` field; payload is never parsed
- **Live-tunable config** — all parameters stored in NATS KV, changes take effect within one PID tick
- **Hybrid cold start** — uses fallback threshold if configured, otherwise runs a percentile warmup
- **Instant fallback** — set `algorithm=fixed` in KV to immediately switch to a static threshold
- **Full observability** — Prometheus metrics for threshold, rate, PID terms, and controller state

## Message Format

```json
{
  "score": 0.87,
  "metadata": {
    "source_id": "model-v2",
    "category": "fraud"
  },
  "payload": { "your": "data", "here": true }
}
```

Flowgate reads `score` to make the threshold decision. `metadata` and `payload` are passed through untouched. Output messages are republished as-is with `Flowgate-Threshold` and `Flowgate-State` headers added.

## Quick Start (Docker Compose)

```bash
docker compose up --build
```

This starts:
- **NATS** with JetStream enabled
- **nats-setup** — creates streams, KV bucket, seeds default config
- **flowgate** — the adaptive threshold service (metrics on `:9090`)
- **producer** — generates synthetic predictions at 500/s with bursts

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
| `kp` | float | 0.01 | PID proportional gain |
| `ki` | float | 0.001 | PID integral gain |
| `kd` | float | 0.0005 | PID derivative gain |
| `fallback_threshold` | float | none | Fixed threshold for cold start and fallback |
| `min_threshold` | float | 0.0 | Lower bound for threshold |
| `max_threshold` | float | 1.0 | Upper bound for threshold |
| `algorithm` | string | pid | `pid` or `fixed` |
| `warmup_samples` | int | 100 | Samples needed for percentile warmup |
| `anti_windup_limit` | float | 100.0 | PID integral term clamp |
| `pid_interval_ms` | int | 1000 | PID tick interval in milliseconds |

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

## Producer

The included producer generates synthetic predictions with configurable distributions:

| Env Var | Default | Description |
|---------|---------|-------------|
| `DISTRIBUTION` | beta | `normal`, `beta`, `uniform`, `bimodal` |
| `RATE` | 100 | Base messages per second |
| `BURST_INTERVAL` | 0 | Seconds between bursts (0 = no bursts) |
| `BURST_MULTIPLIER` | 5 | Rate multiplier during burst |
| `BURST_DURATION` | 2 | Burst length in seconds |

## Development

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## License

MIT
