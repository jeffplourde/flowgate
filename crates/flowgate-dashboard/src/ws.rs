use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::AppState;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    #[serde(rename = "score")]
    Score {
        instance: String,
        score: f64,
        threshold: f64,
        state: String,
    },
    #[serde(rename = "metrics")]
    Metrics {
        instance: String,
        data: std::collections::HashMap<String, f64>,
    },
}

pub async fn handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.event_tx.subscribe();

    info!("WebSocket client connected");

    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&event) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    let recv_task =
        tokio::spawn(async move { while let Some(Ok(_msg)) = receiver.next().await {} });

    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    info!("WebSocket client disconnected");
}

pub async fn subscribe_output_stream(
    js: async_nats::jetstream::Context,
    stream_name: &str,
    instance: &str,
    tx: broadcast::Sender<Event>,
) -> Result<(), async_nats::Error> {
    let consumer_name = format!("dashboard-{instance}");

    let stream = js.get_stream(stream_name).await?;

    let consumer = stream
        .get_or_create_consumer(
            &consumer_name,
            async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(consumer_name.clone()),
                deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::New,
                ..Default::default()
            },
        )
        .await?;

    info!(stream_name, instance, "subscribing to output stream");

    let mut messages = consumer.messages().await?;

    while let Some(msg) = messages.next().await {
        match msg {
            Ok(msg) => {
                let score = msg
                    .headers
                    .as_ref()
                    .and_then(|h| h.get("Flowgate-Score"))
                    .and_then(|v| v.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0);

                let threshold = msg
                    .headers
                    .as_ref()
                    .and_then(|h| h.get("Flowgate-Threshold"))
                    .and_then(|v| v.as_str().parse::<f64>().ok())
                    .unwrap_or(0.0);

                let state = msg
                    .headers
                    .as_ref()
                    .and_then(|h| h.get("Flowgate-State"))
                    .map(|v| v.as_str().to_string())
                    .unwrap_or_default();

                let _ = tx.send(Event::Score {
                    instance: instance.to_string(),
                    score,
                    threshold,
                    state,
                });

                if let Err(e) = msg.ack().await {
                    warn!(error = %e, "failed to ack dashboard message");
                }
            }
            Err(e) => {
                warn!(error = %e, "error receiving dashboard message");
            }
        }
    }

    Ok(())
}

pub async fn scrape_metrics_loop(
    metrics_a: String,
    metrics_b: String,
    tx: broadcast::Sender<Event>,
) {
    let client = reqwest::Client::new();
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        for (instance, url) in [("a", &metrics_a), ("b", &metrics_b)] {
            let metrics_url = format!("{url}/metrics");
            match client.get(&metrics_url).send().await {
                Ok(resp) => {
                    if let Ok(body) = resp.text().await {
                        let data = parse_prometheus_metrics(&body);
                        let _ = tx.send(Event::Metrics {
                            instance: instance.to_string(),
                            data,
                        });
                    }
                }
                Err(e) => {
                    warn!(instance, error = %e, "failed to scrape metrics");
                }
            }
        }
    }
}

fn parse_prometheus_metrics(body: &str) -> std::collections::HashMap<String, f64> {
    let mut map = std::collections::HashMap::new();
    for line in body.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(' ') {
            if key.starts_with("flowgate_") {
                if let Ok(v) = value.parse::<f64>() {
                    map.insert(key.to_string(), v);
                }
            }
        }
    }
    map
}
