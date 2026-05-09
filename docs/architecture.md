# Architecture

## Problem Statement

ML inference pipelines produce predictions with scores. A downstream action system has fixed capacity — it can handle N actions per unit time. The prediction source is bursty and its score distribution shifts over time. A fixed threshold either overloads the downstream system during bursts or wastes capacity during quiet periods.

Flowgate solves this by dynamically controlling the emission rate — ensuring downstream always receives a predictable, tunable number of predictions per second.

## System Overview

```
                                NATS KV (flowgate-config)
                                    ↕ watch
[ML Source] → JetStream (flowgate.in.>) → [Flowgate Service] → JetStream (flowgate.out) → [Downstream]
                                              ↕
                                      Prometheus /metrics
```

Flowgate is a stateless-ish service (its only state is the PID integral term and the message buffer, neither of which needs persistence). It can be restarted at any time — it reconnects, reloads config from KV, and resumes processing with a brief cold-start period.

## Message Flow

### Inbound
Messages arrive on any subject matching `flowgate.in.>` via JetStream. Flowgate reads the `Flowgate-Score` header to get the prediction score. The message body is never inspected — it flows through as opaque bytes.

### Decision
The `ThresholdController` determines what happens to each message:

- **PID/Fixed mode**: score is compared to the current threshold. Above → emit. Below → reject.
- **Buffered mode**: every message goes into the buffer regardless of score. The buffer drainer decides what to emit.

### Outbound
Emitted messages are published to the configured output subject (default: `flowgate.out`) with the original body and headers intact, plus two additional headers:
- `Flowgate-Threshold`: the threshold that was in effect
- `Flowgate-State`: the controller state (pid, fixed, cold_start, warmup)

## Four Algorithm Modes

### `pid` — Adaptive Threshold
The default mode. A PID controller adjusts a score threshold to maintain a target emission rate.

**How it works**: Every `pid_interval_ms` (default 1s), the controller measures the actual emission rate over the last `measurement_window_secs`, computes the error vs `target_rate`, and adjusts the threshold using PID control. Messages arriving between ticks use the current threshold.

**When to use**: When you want rate control with minimal latency. Messages are emitted or rejected immediately — no buffering delay.

### `fixed` — Static Threshold
A simple fixed threshold. Messages above `fallback_threshold` pass, below are rejected. No rate adaptation.

**When to use**: As an emergency fallback, or when the score distribution is well-understood and stable.

### `buffered_batch` — Windowed Best-of-N
Accumulates messages for `max_buffer_duration_ms`, then emits the top N by score (where N = `target_rate × buffer_duration`), dropping the rest.

**When to use**: When you can tolerate periodic bursts of emissions at window boundaries and want maximum quality per window.

### `buffered_streaming` — Priority Queue Drain
Maintains a priority queue (max-heap by score). Every `drain_interval_ms`, pops and emits the single best message. Messages exceeding `max_buffer_duration_ms` without being picked are evicted.

**When to use**: When you want smooth output rate and higher quality than threshold mode, at the cost of latency (up to `max_buffer_duration_ms`).

## The Quality/Latency Tradeoff

This is the central insight of the system: **buffered modes produce higher-quality output at the cost of latency**.

The PID threshold mode makes instantaneous decisions — a message either passes or doesn't, right now. It has no memory of what came before and can't go back. During a burst, many high-scoring messages may pass the threshold even though slightly better ones are coming right behind them.

The buffered streaming mode holds messages and picks the best. During a burst, the buffer fills with a dense pool of candidates, and only the very best are emitted. **This effect is most visible during bursts** — the average emitted score jumps noticeably in buffered mode while the threshold mode just raises its threshold reactively.

In the demo, you can observe this by watching the "Avg Score" metric on both panels when a burst hits. The buffered instance's avg score climbs while the threshold instance's stays roughly constant.

### Burst Quality Insight

During bursts, buffered streaming shows notably higher average emitted scores compared to PID mode. This happens because the buffer accumulates a denser pool of candidates when a batch of client data arrives. The drainer continues popping the single best message at its steady interval, so it cherry-picks from a much richer set. The quality advantage is most visible in the dashboard when the multi-client producer fires a wave of batches — the buffered instance's average emitted score spikes upward while the threshold instance's average stays roughly flat (it just raises the threshold reactively, which filters more but doesn't select the best).

## Backpressure-Aware Drain

The buffer drain loop includes two safety mechanisms that prevent it from emitting low-value or harmful traffic:

### Quality Floor (`min_quality_score`)
Before emitting, the drainer peeks at the best score in the buffer. If it is below `min_quality_score`, the drain is skipped for that interval and `flowgate_drain_skipped_quality_total` is incremented. This prevents emitting predictions that are not worth acting on, even when the buffer is non-empty.

Default: `0.0` (disabled — all scores are acceptable).

### Downstream Backpressure (`backpressure_threshold_ms`)
After each publish, the drain loop records the round-trip latency as `flowgate_publish_latency_ms`. If the latency exceeds `backpressure_threshold_ms`, the next drain cycle is skipped and `flowgate_drain_skipped_backpressure_total` is incremented. This gives the downstream system time to recover before more messages are pushed.

Default: `500` ms.

## Component Details

### ThresholdController (`threshold.rs`)
A state machine with states: `ColdStart`, `Warmup`, `Pid`, `Fixed`.

- **ColdStart**: uses `fallback_threshold` while accumulating rate observations. Transitions to PID after one tick with data.
- **Warmup**: no fallback threshold available. Buffers scores until `warmup_samples` received, computes a percentile-based initial threshold, then transitions to PID.
- **Pid**: the PID controller is active, adjusting the threshold each tick.
- **Fixed**: static threshold, no adaptation.

For buffered modes, the controller simply returns `CheckResult::Buffer` for every message — the buffer drainer handles emission.

### PID Controller (`pid.rs`)
A pure computation unit with no I/O. Takes error (target - actual rate) and dt, returns the new threshold.

Key details:
- Positive error = emitting too few → lower the threshold
- Anti-windup clamps the integral term to prevent overshoot after quiet periods
- The controller subtracts the PID adjustment from the current threshold (because higher threshold = fewer emissions)

### MessageBuffer (`buffer.rs`)
A `BinaryHeap<BufferedMessage>` ordered by score (max-heap). Supports:
- `push`: add a message
- `drain_one`: pop the highest-scoring message
- `drain_top_n`: pop the top N
- `drain_batch`: evict expired, return top N, drop the rest
- `evict_expired`: remove messages older than `max_buffer_duration_ms`

### Pipeline (`pipeline.rs`)
Three concurrent tasks:
1. **Consumer**: pulls messages from JetStream, routes through the threshold controller, emits or buffers
2. **PID Ticker**: runs the PID update loop on a configurable interval
3. **Buffer Drainer**: drains the buffer on its own interval (streaming mode) or window (batch mode)

All three share the `ThresholdController` and `MessageBuffer` via `Arc<Mutex<>>`.

### Config (`config.rs`)
All parameters are read from a NATS KV bucket on startup and watched for changes. Changes are broadcast via `tokio::watch` to all tasks. The KV bucket name is configurable per-instance via CLI args.

## Multi-Instance Support

The service supports per-instance configuration via environment variables:
- `NATS_KV_BUCKET`: which KV bucket to use for config
- `OUTPUT_SUBJECT`: where to publish emitted messages
- `OUTPUT_STREAM`: JetStream stream name for the output
- `CONSUMER_NAME`: durable consumer name (defaults to `flowgate-{bucket}`)

This enables the side-by-side demo where two instances process the same input with different algorithms.

## Replica Support

Each flowgate service in docker-compose runs with `deploy.replicas: 2`. Replicas share the same durable consumer name, so JetStream load-balances messages across them. This means:

- Each replica processes a subset of the input stream
- PID state is per-replica (each replica runs its own PID loop)
- The effective throughput scales with replica count
- On restart, the durable consumer ensures no messages are lost

The dashboard handles replicas by resolving the Docker Compose service DNS name (e.g., `flowgate-threshold`) to all replica IPs via `tokio::net::lookup_host`, scraping Prometheus metrics from each, and aggregating: counters are summed, gauges are averaged. This gives a unified view of the service's behavior regardless of replica count.

## Multi-Client Batch Producer

The producer simulates a realistic ML inference pipeline rather than a simple steady-rate message generator:

- **70 simulated clients** (configurable via `num_clients`) each independently generate batches
- **Time compression**: real-world hourly patterns are compressed by a configurable factor (default 60x, so 1 hour of traffic in 1 minute)
- **Batch sizes**: each client sends between `min_batch_size` (100) and `max_batch_size` (5000) predictions per batch
- **Score distribution**: configurable (`beta`, `normal`, `uniform`, `bimodal`) with tunable variance

All producer parameters are stored in the `flowgate-producer-config` KV bucket and can be changed at runtime.

### Producer Metrics

The producer exposes Prometheus metrics on its own port:

| Metric | Type | Description |
|--------|------|-------------|
| `producer_messages_published_total` | counter | Total predictions published |
| `producer_publish_errors_total` | counter | Failed publish attempts |
| `producer_batches_sent_total` | counter | Total batches completed |
| `producer_active_clients` | gauge | Number of active simulated clients |
| `producer_backpressure` | gauge | 0 or 1 — set to 1 when publishes fail, indicating NATS backpressure |

The dashboard scrapes producer metrics and surfaces them alongside flowgate metrics. When `producer_backpressure` = 1, the dashboard shows a red banner alerting the operator.

## Dashboard Architecture

```
Browser ↔ WebSocket ↔ Dashboard Backend ↔ NATS
                          ↕
                    Prometheus scrape
```

The dashboard backend (`flowgate-dashboard`) is an axum server that:
1. Subscribes to both output JetStream streams, extracts score/threshold/state from headers, pushes events to connected WebSocket clients
2. Scrapes Prometheus `/metrics` from both flowgate instances and the producer every second, resolves Docker DNS to all replica IPs, aggregates metrics (sum counters, avg gauges), and pushes snapshots to WebSocket clients
3. Serves a REST API for reading/writing NATS KV config (both flowgate and producer KV buckets)
4. Serves the built React SPA as static files

The browser never talks to NATS directly.

### Metrics Aggregation

When scraping metrics, the dashboard resolves each service hostname (e.g., `flowgate-threshold`) to all container IPs via DNS lookup. It scrapes each IP individually, then aggregates:
- **Counters** (names containing `_total`): summed across replicas
- **Gauges** (everything else): averaged across replicas

This ensures the dashboard shows correct totals for counters (e.g., total messages received across all replicas) and representative values for gauges (e.g., average threshold).

### Ingestion Rate

The `flowgate_ingestion_rate` gauge tracks the current inbound message rate (messages/sec) as observed by each flowgate instance. The dashboard displays this metric to give operators visibility into the input load, which is especially useful for understanding PID behavior and buffer fill rates.
