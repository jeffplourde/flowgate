#!/bin/sh
set -e

NATS_URL="${NATS_URL:-nats://nats:4222}"

echo "Waiting for NATS to be ready..."
until nats pub --server="$NATS_URL" flowgate.healthcheck "ready" 2>/dev/null; do
    sleep 0.5
done
echo "NATS is ready"

echo "Creating streams..."

nats stream add FLOWGATE_IN \
    --server="$NATS_URL" \
    --subjects="flowgate.in.>" \
    --retention=limits \
    --max-age=1h \
    --storage=file \
    --replicas=1 \
    --discard=old \
    --defaults 2>/dev/null || echo "FLOWGATE_IN already exists"

nats stream add FLOWGATE_OUT \
    --server="$NATS_URL" \
    --subjects="flowgate.out" \
    --retention=limits \
    --max-age=1h \
    --storage=file \
    --replicas=1 \
    --discard=old \
    --defaults 2>/dev/null || echo "FLOWGATE_OUT already exists"

echo "Creating KV bucket..."
nats kv add flowgate-config \
    --server="$NATS_URL" \
    --history=5 2>/dev/null || echo "flowgate-config already exists"

echo "Seeding default config..."
nats kv put flowgate-config target_rate "10" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config measurement_window_secs "10" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config kp "0.0004" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config ki "0.00004" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config kd "0.00001" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config fallback_threshold "0.5" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config min_threshold "0" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config max_threshold "1" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config algorithm "pid" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config warmup_samples "100" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config anti_windup_limit "100" --server="$NATS_URL" 2>/dev/null || true
nats kv put flowgate-config pid_interval_ms "1000" --server="$NATS_URL" 2>/dev/null || true

echo "NATS setup complete!"
nats kv ls flowgate-config --server="$NATS_URL" 2>/dev/null || true
