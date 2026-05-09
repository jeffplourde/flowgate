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

nats stream add FLOWGATE_OUT_A \
    --server="$NATS_URL" \
    --subjects="flowgate.out.threshold" \
    --retention=limits \
    --max-age=1h \
    --storage=file \
    --replicas=1 \
    --discard=old \
    --defaults 2>/dev/null || echo "FLOWGATE_OUT_A already exists"

nats stream add FLOWGATE_OUT_B \
    --server="$NATS_URL" \
    --subjects="flowgate.out.buffered" \
    --retention=limits \
    --max-age=1h \
    --storage=file \
    --replicas=1 \
    --discard=old \
    --defaults 2>/dev/null || echo "FLOWGATE_OUT_B already exists"

seed_bucket() {
    local BUCKET=$1
    local ALGORITHM=$2

    echo "Creating KV bucket: $BUCKET (algorithm: $ALGORITHM)..."
    nats kv add "$BUCKET" --server="$NATS_URL" --history=5 2>/dev/null || echo "$BUCKET already exists"

    nats kv put "$BUCKET" target_rate "10" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" measurement_window_secs "10" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" kp "0.0004" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" ki "0.00004" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" kd "0.00001" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" fallback_threshold "0.5" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" min_threshold "0" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" max_threshold "1" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" algorithm "$ALGORITHM" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" warmup_samples "100" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" anti_windup_limit "100" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" pid_interval_ms "1000" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" max_buffer_duration_ms "5000" --server="$NATS_URL" 2>/dev/null || true
    nats kv put "$BUCKET" drain_interval_ms "100" --server="$NATS_URL" 2>/dev/null || true
}

seed_bucket "flowgate-config-a" "pid"
seed_bucket "flowgate-config-b" "buffered_streaming"

echo "NATS setup complete!"
