# Operator Runbook

## Starting the System

### Docker Compose (Demo)
```bash
docker compose up --build
```

Services start in order: NATS → nats-setup (creates streams/KV) → flowgate instances + producer + dashboard.

### Verifying Health
```bash
# All services running
docker compose ps

# Flowgate is processing messages
curl -s http://localhost:9090/metrics | grep flowgate_messages_received_total

# Dashboard is serving
curl -s http://localhost:3000/ | head -1
```

### Key Ports
| Port | Service |
|------|---------|
| 3000 | Dashboard UI |
| 4222 | NATS client |
| 8222 | NATS monitoring |
| 9090 | Flowgate A (threshold) metrics |
| 9091 | Flowgate B (buffered) metrics |

## Emergency: Switch to Fixed Threshold

If the PID is misbehaving (oscillating, stuck, wrong rate), immediately switch to a fixed threshold:

```bash
# For instance A
nats kv put flowgate-config-a algorithm fixed
nats kv put flowgate-config-a fallback_threshold 0.5

# For instance B
nats kv put flowgate-config-b algorithm fixed
nats kv put flowgate-config-b fallback_threshold 0.5
```

This takes effect within one PID tick (1 second). To switch back:
```bash
nats kv put flowgate-config-a algorithm pid
```

## Reading the Metrics

### Key Metrics to Watch

| Metric | Healthy Range | Action if Out of Range |
|--------|--------------|----------------------|
| `flowgate_actual_rate` | Within ±20% of target | Check PID gains, input rate |
| `flowgate_current_threshold` | 0.1-0.9 | If at 0 or 1, gains are too aggressive |
| `flowgate_pid_error` | Oscillating near 0 | If growing, PID isn't converging |
| `flowgate_buffer_size` | < input_rate × buffer_duration | If growing unbounded, drain can't keep up |
| `flowgate_buffer_evicted_total` | Increasing steadily | Normal in buffered mode — messages age out |

### Understanding Controller State
| Value | State | Meaning |
|-------|-------|---------|
| 0 | cold_start | Using fallback threshold, gathering rate data |
| 1 | warmup | No fallback, collecting samples for percentile estimation |
| 2 | pid | PID active and adjusting threshold |
| 3 | fixed | Static threshold, no adaptation |

## Restart Behavior

Flowgate is designed for safe restarts:
1. Durable JetStream consumer resumes from last acked message — no data loss
2. Config reloads from KV — no drift from last state
3. PID state (integral, threshold) resets — brief transient as PID re-converges
4. Buffer is lost — messages in-flight at shutdown are dropped (they were already acked from the input stream)

Cold start behavior depends on `fallback_threshold`:
- **Set**: starts processing immediately at that threshold, PID takes over after one measurement window
- **Not set**: buffers messages until `warmup_samples` received, then computes initial threshold

## Scaling

### Single Instance
Handles 100-10K msg/s comfortably. The bottleneck is the `Arc<Mutex<ThresholdController>>` — each message acquires the lock briefly.

### Horizontal Scaling
Not yet implemented. Would require:
- Partitioned JetStream consumers (one per partition)
- Shared rate measurement (or per-partition rate targets)
- This is tracked as a future enhancement

## Troubleshooting

### Messages not being processed
1. Check consumer exists: `nats consumer info FLOWGATE_IN <consumer-name>`
2. Check ack pending count — if high, messages are being delivered but not acked
3. Check flowgate logs for errors: `docker compose logs flowgate-threshold`

### Threshold stuck at 1.0
The PID is driving the threshold up because actual_rate > target_rate. Either:
- Input rate is very high and PID gains are too aggressive
- Reduce Kp: `nats kv put flowgate-config-a kp 0.0001`
- Or switch to fixed: `nats kv put flowgate-config-a algorithm fixed`

### Buffer growing without bound
The drain rate is lower than the input rate. Either:
- Increase `target_rate`
- Decrease `max_buffer_duration_ms` (expired messages get evicted)
- Check that the drain_interval_ms is appropriate: should be ~`1000/target_rate`

### Dashboard shows "Disconnected"
- Check dashboard container is running: `docker compose ps dashboard`
- Check dashboard can reach NATS: `docker compose logs dashboard`
- Hard-refresh the browser (Ctrl+Shift+R) to clear cached JS
