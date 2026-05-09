use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::AppState;

pub fn router(_state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/config/{instance}", axum::routing::get(get_config))
        .route("/config/{instance}/{key}", axum::routing::put(put_config))
        .route("/metrics/{instance}", axum::routing::get(get_metrics))
}

fn bucket_name(state: &AppState, instance: &str) -> Option<String> {
    match instance {
        "a" => Some(state.kv_bucket_a.clone()),
        "b" => Some(state.kv_bucket_b.clone()),
        _ => None,
    }
}

fn metrics_url(state: &AppState, instance: &str) -> Option<String> {
    match instance {
        "a" => Some(format!("{}/metrics", state.flowgate_a_metrics)),
        "b" => Some(format!("{}/metrics", state.flowgate_b_metrics)),
        _ => None,
    }
}

#[derive(Serialize)]
struct ConfigEntry {
    key: String,
    value: String,
}

async fn get_config(
    State(state): State<Arc<AppState>>,
    Path(instance): Path<String>,
) -> impl IntoResponse {
    let Some(bucket) = bucket_name(&state, &instance) else {
        return (StatusCode::NOT_FOUND, "unknown instance").into_response();
    };

    let store = match state.js.get_key_value(&bucket).await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "failed to open KV bucket");
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    let keys = [
        "target_rate",
        "measurement_window_secs",
        "kp",
        "ki",
        "kd",
        "fallback_threshold",
        "min_threshold",
        "max_threshold",
        "algorithm",
        "warmup_samples",
        "anti_windup_limit",
        "pid_interval_ms",
        "max_buffer_duration_ms",
        "drain_interval_ms",
    ];

    let mut entries = Vec::new();
    for key in keys {
        if let Ok(Some(bytes)) = store.get(key).await {
            if let Ok(value) = std::str::from_utf8(&bytes) {
                entries.push(ConfigEntry {
                    key: key.to_string(),
                    value: value.to_string(),
                });
            }
        }
    }

    Json(entries).into_response()
}

#[derive(Deserialize)]
struct PutBody {
    value: String,
}

async fn put_config(
    State(state): State<Arc<AppState>>,
    Path((instance, key)): Path<(String, String)>,
    Json(body): Json<PutBody>,
) -> impl IntoResponse {
    let Some(bucket) = bucket_name(&state, &instance) else {
        return (StatusCode::NOT_FOUND, "unknown instance").into_response();
    };

    let store = match state.js.get_key_value(&bucket).await {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };

    match store
        .put(&key, Bytes::from(body.value.clone()))
        .await
    {
        Ok(_) => {
            info!(instance, key, value = %body.value, "config updated via API");
            (StatusCode::OK, "ok").into_response()
        }
        Err(e) => {
            warn!(error = %e, "failed to put config");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn get_metrics(
    State(state): State<Arc<AppState>>,
    Path(instance): Path<String>,
) -> impl IntoResponse {
    let Some(url) = metrics_url(&state, &instance) else {
        return (StatusCode::NOT_FOUND, "unknown instance".to_string()).into_response();
    };

    let client = reqwest::Client::new();
    match client.get(&url).send().await {
        Ok(resp) => match resp.text().await {
            Ok(body) => (StatusCode::OK, body).into_response(),
            Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
        },
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}
