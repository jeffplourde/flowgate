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

const COUNTER_METRICS: &[&str] = &[
    "flowgate_messages_received_total",
    "flowgate_messages_emitted_total",
    "flowgate_messages_rejected_total",
    "flowgate_buffer_evicted_total",
    "producer_messages_published_total",
    "producer_publish_errors_total",
    "producer_batches_sent_total",
];

pub async fn scrape_metrics_loop(
    metrics_a: String,
    metrics_b: String,
    producer_metrics: String,
    tx: broadcast::Sender<Event>,
) {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_default();
    let mut interval = tokio::time::interval(Duration::from_secs(1));

    loop {
        interval.tick().await;

        for (instance, base_url) in [
            ("a", &metrics_a),
            ("b", &metrics_b),
            ("producer", &producer_metrics),
        ] {
            let data = scrape_and_aggregate(&client, base_url).await;
            if !data.is_empty() {
                let _ = tx.send(Event::Metrics {
                    instance: instance.to_string(),
                    data,
                });
            }
        }
    }
}

async fn scrape_and_aggregate(
    client: &reqwest::Client,
    base_url: &str,
) -> std::collections::HashMap<String, f64> {
    let host_port = base_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let addrs: Vec<_> = match tokio::net::lookup_host(host_port).await {
        Ok(addrs) => addrs.collect(),
        Err(_) => {
            if let Ok(resp) = client.get(format!("{base_url}/metrics")).send().await {
                if let Ok(body) = resp.text().await {
                    return parse_prometheus_metrics(&body);
                }
            }
            return std::collections::HashMap::new();
        }
    };

    let mut all_results = Vec::new();
    for addr in &addrs {
        let url = format!("http://{addr}/metrics");
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(body) = resp.text().await {
                all_results.push(parse_prometheus_metrics(&body));
            }
        }
    }

    if all_results.is_empty() {
        return std::collections::HashMap::new();
    }
    if all_results.len() == 1 {
        return all_results.into_iter().next().unwrap();
    }

    aggregate_metrics(all_results)
}

fn aggregate_metrics(
    results: Vec<std::collections::HashMap<String, f64>>,
) -> std::collections::HashMap<String, f64> {
    let mut aggregated = std::collections::HashMap::new();
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for result in &results {
        for (key, value) in result {
            let entry = aggregated.entry(key.clone()).or_insert(0.0);
            if COUNTER_METRICS.contains(&key.as_str()) {
                *entry += value;
            } else {
                *entry += value;
                *counts.entry(key.clone()).or_insert(0) += 1;
            }
        }
    }

    for (key, value) in &mut aggregated {
        if !COUNTER_METRICS.contains(&key.as_str()) {
            if let Some(&count) = counts.get(key) {
                if count > 1 {
                    *value /= count as f64;
                }
            }
        }
    }

    aggregated
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
