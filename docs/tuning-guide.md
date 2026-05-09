# Tuning Guide

## Choosing an Algorithm

| Mode | Latency | Quality | Rate Stability | Use When |
|------|---------|---------|----------------|----------|
| `pid` | ~0ms | Moderate | Good (converges) | Low latency matters, quality is acceptable |
| `fixed` | ~0ms | Fixed | None (static) | Emergency fallback, known-stable distributions |
| `buffered_batch` | Up to buffer duration | High | Bursty (window edges) | Quality matters, periodic bursts are OK |
| `buffered_streaming` | Up to buffer duration | High | Excellent (smooth) | Quality + smooth rate, latency acceptable |

## PID Tuning

### The Plant Gain Problem

The PID controller's effective behavior depends on the **plant gain**: `λ · f(θ*)`, where:
- `λ` = input message rate (messages/sec)
- `f(θ*)` = the score distribution's PDF value at the equilibrium threshold

This means:
- **Higher input rates need lower PID gains** (the system is more sensitive)
- **Denser score regions need lower gains** (small threshold changes cause big rate swings)
- **Tail regions need higher gains** (large threshold changes needed for any effect)

### Computing the Equilibrium Threshold

The threshold the PID will converge to:
```
θ* = F⁻¹(1 - target_rate / input_rate)
```

For Beta(2,5) at 500 msg/s targeting 10/s:
```
θ* = F⁻¹(1 - 10/500) = F⁻¹(0.98) ≈ 0.658
```

### Starting Point for Gains

Compute the plant gain, then set conservative proportional gain:

```
plant_gain = input_rate × PDF(θ*)
Kp ≈ 0.05 / plant_gain
Ki ≈ Kp / 10
Kd ≈ Kp / 40
```

For our demo (Beta(2,5), 500 msg/s): plant_gain ≈ 135, so Kp ≈ 0.0004.

### Signs of Mis-Tuning

| Symptom | Cause | Fix |
|---------|-------|-----|
| Threshold oscillates wildly | Gains too high for plant gain | Reduce Kp by 2-5x |
| Slow convergence (>30s) | Gains too low | Increase Kp by 2x |
| Steady-state offset (rate never reaches target) | Ki too low or zero | Increase Ki |
| Overshoot after burst then ringing | Kd too low relative to Kp | Increase Kd or reduce Kp |
| Threshold stuck at 0 or 1 | Anti-windup too high, integral wound up | Reduce anti_windup_limit |

### Tuning Live

All gains can be changed at runtime via NATS KV:
```bash
nats kv put flowgate-config-a kp 0.001
nats kv put flowgate-config-a ki 0.0001
```

Watch the Prometheus metrics or the dashboard to see the effect immediately.

## Burst Behavior

### PID Mode During Bursts
When input rate spikes (e.g., 5x burst), the effective plant gain also jumps by 5x. If your PID gains are tuned for steady state, the system temporarily over-reacts:

1. Burst starts → emission rate jumps above target
2. PID raises threshold aggressively
3. Burst ends → emission rate drops below target
4. PID lowers threshold → overshoots downward
5. Eventually converges back

The transient depends on burst duration vs measurement window. Longer bursts give the PID time to settle during the burst itself.

### Buffered Mode During Bursts
Bursts are where buffered mode really shines. The buffer fills with a much denser pool of high-quality candidates. The drainer continues picking the best at its steady rate, so:
- **Average emitted score increases during bursts** — more candidates to choose from
- **Emission rate stays perfectly smooth** — buffer absorbs the burst
- **Buffer size grows then shrinks** — visible in the dashboard

This is the clearest demonstration of the quality/latency tradeoff.

## Buffer Tuning

### `max_buffer_duration_ms`
How long a message can sit in the buffer before being evicted.

- **Too short** (< 1s): buffer doesn't accumulate enough candidates, quality improvement is minimal
- **Too long** (> 30s): high latency, large memory usage, stale predictions
- **Sweet spot**: 3-10x the drain interval for streaming mode

### `drain_interval_ms` (streaming mode)
How often the drainer pops the best message.

- Should be approximately `1000 / target_rate` for smooth output
- Example: target_rate=10/s → drain_interval_ms=100

### Buffer Memory
Each buffered message holds the full payload bytes plus headers. At 500 msg/s with 5s buffer duration, you'll have ~2500 messages in the buffer. Size depends on payload size.

## Measurement Window

The `measurement_window_secs` parameter controls the sliding window for computing actual emission rate.

- **Longer window** (15-30s): smoother rate estimate, but slower to react to changes
- **Shorter window** (3-5s): faster reaction, but noisier rate signal causes threshold jitter
- **Rule of thumb**: 5-10x the PID tick interval

## Target Rate

Set `target_rate` to the downstream system's sustainable throughput. If downstream can handle 50 actions/sec, set target_rate=50.

If you're unsure, start conservative (lower than capacity) and increase gradually while monitoring downstream health.
