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
| 9090 | Flowgate A (threshold) metrics — each replica exposes this port internally |
| 9091 | Flowgate B (buffered) metrics — each replica exposes this port internally |
| 9090 | Producer metrics (separate container) |

Note: With replicas, ports 9090/9091 are per-container. The dashboard resolves all replica IPs via DNS and scrapes each one individually.

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
| `flowgate_ingestion_rate` | Matches expected input rate | If zero, producer may be down or NATS is unhealthy |
| `flowgate_drain_skipped_quality_total` | Low or zero | If high, `min_quality_score` is too aggressive for the input distribution |
| `flowgate_drain_skipped_backpressure_total` | Low or zero | If high, downstream is slow — investigate downstream or raise `backpressure_threshold_ms` |
| `flowgate_publish_latency_ms` | < 100ms | If consistently high, downstream or NATS is under pressure |

### Producer Metrics to Watch

| Metric | Healthy Range | Action if Out of Range |
|--------|--------------|----------------------|
| `producer_messages_published_total` | Increasing steadily | If flat, producer may be stuck or backpressured |
| `producer_publish_errors_total` | Zero or near-zero | If rising, NATS is rejecting publishes — check stream limits |
| `producer_batches_sent_total` | Increasing | If flat, check producer logs |
| `producer_active_clients` | Matches `num_clients` config | Should equal configured value |
| `producer_backpressure` | 0 | If 1, NATS is rejecting publishes — dashboard shows red banner |

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

### Replica Support (Current)

Each flowgate service in docker-compose runs with `deploy.replicas: 2`. Replicas share the same durable consumer name, so JetStream load-balances messages across them.

**Managing replicas:**
```bash
# Scale up
docker compose up -d --scale flowgate-threshold=4

# Scale down
docker compose up -d --scale flowgate-threshold=1

# Check replica count
docker compose ps flowgate-threshold
```

**Important considerations:**
- Each replica runs its own PID controller independently. Doubling replicas means each sees half the input rate, so the effective `target_rate` should be divided by replica count (or configure each replica's target rate accordingly).
- The dashboard automatically discovers all replica IPs via DNS and aggregates their metrics. No dashboard reconfiguration is needed when scaling.
- On scale-down, the durable consumer ensures messages are rebalanced to surviving replicas with no data loss.

### Horizontal Scaling (Future)
For true horizontal scaling beyond replicas, would require:
- Partitioned JetStream consumers (one per partition)
- Shared rate measurement (or per-partition rate targets)

## Handling Backpressure

### Dashboard Shows Red Backpressure Banner
The producer reports `producer_backpressure=1` when NATS publishes fail. This triggers a red banner in the dashboard.

**Immediate actions:**
1. Check NATS health: `nats server check connection`
2. Check stream storage: `nats stream info FLOWGATE_IN` — if storage is at max (10GB), old messages are being dropped
3. Reduce producer load: `nats kv put flowgate-producer-config num_clients 20`
4. Check consumer lag: `nats consumer info FLOWGATE_IN <consumer-name>` — high pending count means flowgate isn't keeping up

### Drain Skipping Due to Backpressure
If `flowgate_drain_skipped_backpressure_total` is rising:
1. Check `flowgate_publish_latency_ms` — what is the actual downstream latency?
2. If downstream is genuinely slow, this is working as intended — flowgate is protecting the downstream system
3. If latency is just above threshold, raise it: `nats kv put flowgate-config-b backpressure_threshold_ms 1000`
4. If downstream is healthy but NATS is slow, investigate NATS performance

### Drain Skipping Due to Quality Floor
If `flowgate_drain_skipped_quality_total` is rising:
1. The input distribution may have shifted — scores are lower than expected
2. Lower the quality floor: `nats kv put flowgate-config-b min_quality_score 0.3`
3. Or set to 0 to disable: `nats kv put flowgate-config-b min_quality_score 0.0`

## Producer Monitoring

### Checking Producer Health
```bash
# Producer metrics
curl -s http://producer:9090/metrics | grep producer_

# Producer config
nats kv ls flowgate-producer-config

# Adjust producer load
nats kv put flowgate-producer-config num_clients 30
nats kv put flowgate-producer-config time_compression 120  # faster compression = more load
```

### Changing Producer Behavior at Runtime
All producer parameters are live-tunable via the `flowgate-producer-config` KV bucket:

```bash
# Reduce client count to ease load
nats kv put flowgate-producer-config num_clients 20

# Increase batch sizes for burst testing
nats kv put flowgate-producer-config min_batch_size 1000
nats kv put flowgate-producer-config max_batch_size 10000

# Switch score distribution
nats kv put flowgate-producer-config distribution bimodal
nats kv put flowgate-producer-config distribution_variance 0.5
```

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
