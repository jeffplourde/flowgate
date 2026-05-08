use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream;
use async_nats::jetstream::consumer::pull::Config as ConsumerConfig;
use async_nats::jetstream::stream::Config as StreamConfig;
use bytes::Bytes;
use futures::StreamExt;
use tokio::sync::{watch, Mutex};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::envelope::Envelope;
use crate::metrics as m;
use crate::threshold::ThresholdController;

pub const STREAM_IN: &str = "FLOWGATE_IN";
pub const STREAM_OUT: &str = "FLOWGATE_OUT";
pub const SUBJECT_IN: &str = "flowgate.in.>";
pub const SUBJECT_OUT: &str = "flowgate.out";
pub const CONSUMER_NAME: &str = "flowgate-service";

pub async fn setup_streams(js: &jetstream::Context) -> Result<(), async_nats::Error> {
    js.get_or_create_stream(StreamConfig {
        name: STREAM_IN.to_string(),
        subjects: vec![SUBJECT_IN.to_string()],
        ..Default::default()
    })
    .await?;

    js.get_or_create_stream(StreamConfig {
        name: STREAM_OUT.to_string(),
        subjects: vec![SUBJECT_OUT.to_string()],
        ..Default::default()
    })
    .await?;

    info!("JetStream streams ready");
    Ok(())
}

pub async fn run_consumer(
    js: jetstream::Context,
    controller: Arc<Mutex<ThresholdController>>,
    _config_rx: watch::Receiver<Config>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), async_nats::Error> {
    let stream = js.get_stream(STREAM_IN).await?;
    let consumer = stream
        .get_or_create_consumer(
            CONSUMER_NAME,
            ConsumerConfig {
                durable_name: Some(CONSUMER_NAME.to_string()),
                ..Default::default()
            },
        )
        .await?;

    info!("consumer ready, starting message loop");

    let mut messages = consumer.messages().await?;

    loop {
        tokio::select! {
            msg = messages.next() => {
                let Some(msg) = msg else {
                    warn!("message stream ended");
                    break;
                };
                match msg {
                    Ok(msg) => {
                        process_message(&js, &controller, &msg).await;
                        if let Err(e) = msg.ack().await {
                            error!(error = %e, "failed to ack message");
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "error receiving message");
                    }
                }
            }
            _ = shutdown.recv() => {
                info!("consumer shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn process_message(
    js: &jetstream::Context,
    controller: &Arc<Mutex<ThresholdController>>,
    msg: &async_nats::jetstream::Message,
) {
    m::record_received();

    let payload = &msg.payload;
    let envelope: Envelope = match serde_json::from_slice(payload) {
        Ok(env) => env,
        Err(e) => {
            warn!(error = %e, "failed to parse envelope, skipping");
            m::record_rejected();
            return;
        }
    };

    let passes = {
        let mut ctrl = controller.lock().await;
        ctrl.check(envelope.score)
    };

    if passes {
        m::record_emitted();

        let mut headers = async_nats::HeaderMap::new();
        let ctrl = controller.lock().await;
        headers.insert(
            "Flowgate-Threshold",
            format!("{:.6}", ctrl.threshold()).as_str(),
        );
        headers.insert("Flowgate-State", ctrl.state_name());
        drop(ctrl);

        if let Err(e) = js
            .publish_with_headers(
                SUBJECT_OUT.to_string(),
                headers,
                Bytes::copy_from_slice(payload),
            )
            .await
        {
            error!(error = %e, "failed to publish to output stream");
        }
    } else {
        m::record_rejected();
        debug!(score = envelope.score, "message rejected");
    }
}

pub async fn run_pid_ticker(
    controller: Arc<Mutex<ThresholdController>>,
    mut config_rx: watch::Receiver<Config>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    let config = config_rx.borrow_and_update().clone();
    let mut interval = tokio::time::interval(Duration::from_millis(config.pid_interval_ms));

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let mut ctrl = controller.lock().await;
                ctrl.tick();
            }
            result = config_rx.changed() => {
                if result.is_err() {
                    break;
                }
                let config = config_rx.borrow_and_update().clone();
                interval = tokio::time::interval(Duration::from_millis(config.pid_interval_ms));
                let mut ctrl = controller.lock().await;
                ctrl.update_config(&config);
                info!(state = ctrl.state_name(), threshold = ctrl.threshold(), "config updated");
            }
            _ = shutdown.recv() => {
                info!("PID ticker shutting down");
                break;
            }
        }
    }
}
