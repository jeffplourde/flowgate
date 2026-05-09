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

## Dashboard Architecture

```
Browser ↔ WebSocket ↔ Dashboard Backend ↔ NATS
                          ↕
                    Prometheus scrape
```

The dashboard backend (`flowgate-dashboard`) is an axum server that:
1. Subscribes to both output JetStream streams, extracts score/threshold/state from headers, pushes events to connected WebSocket clients
2. Scrapes Prometheus `/metrics` from both flowgate instances every second, parses the text format, pushes metric snapshots to WebSocket clients
3. Serves a REST API for reading/writing NATS KV config
4. Serves the built React SPA as static files

The browser never talks to NATS directly.
