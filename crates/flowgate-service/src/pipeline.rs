use std::sync::Arc;
use std::time::{Duration, Instant};

use async_nats::jetstream;
use async_nats::jetstream::consumer::pull::Config as ConsumerConfig;
use async_nats::jetstream::stream::Config as StreamConfig;
use futures::StreamExt;
use tokio::sync::{watch, Mutex};
use tracing::{debug, error, info, warn};

use crate::buffer::{BufferedMessage, MessageBuffer};
use crate::config::{Algorithm, Config};
use crate::envelope::{self, STATE_HEADER, THRESHOLD_HEADER};
use crate::metrics as m;
use crate::threshold::{CheckResult, ThresholdController};

pub const STREAM_IN: &str = "FLOWGATE_IN";
pub const SUBJECT_IN: &str = "flowgate.in.>";

pub struct PipelineConfig {
    pub output_subject: String,
    pub output_stream: String,
    pub consumer_name: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            output_subject: "flowgate.out".to_string(),
            output_stream: "FLOWGATE_OUT".to_string(),
            consumer_name: "flowgate-service".to_string(),
        }
    }
}

pub async fn setup_streams(
    js: &jetstream::Context,
    pipeline_config: &PipelineConfig,
) -> Result<(), async_nats::Error> {
    js.get_or_create_stream(StreamConfig {
        name: STREAM_IN.to_string(),
        subjects: vec![SUBJECT_IN.to_string()],
        ..Default::default()
    })
    .await?;

    js.get_or_create_stream(StreamConfig {
        name: pipeline_config.output_stream.clone(),
        subjects: vec![pipeline_config.output_subject.clone()],
        ..Default::default()
    })
    .await?;

    info!("JetStream streams ready");
    Ok(())
}

pub async fn run_consumer(
    js: jetstream::Context,
    controller: Arc<Mutex<ThresholdController>>,
    buffer: Arc<Mutex<MessageBuffer>>,
    pipeline_config: Arc<PipelineConfig>,
    _config_rx: watch::Receiver<Config>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) -> Result<(), async_nats::Error> {
    let stream = js.get_stream(STREAM_IN).await?;
    let consumer = stream
        .get_or_create_consumer(
            &pipeline_config.consumer_name,
            ConsumerConfig {
                durable_name: Some(pipeline_config.consumer_name.clone()),
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
                        process_message(
                            &js, &controller, &buffer, &pipeline_config, &msg,
                        ).await;
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
    buffer: &Arc<Mutex<MessageBuffer>>,
    pipeline_config: &PipelineConfig,
    msg: &async_nats::jetstream::Message,
) {
    m::record_received();

    let score = match envelope::extract_score(msg.headers.as_ref()) {
        Some(s) => s,
        None => {
            warn!("message missing or invalid Flowgate-Score header, skipping");
            m::record_rejected();
            return;
        }
    };

    let result = {
        let mut ctrl = controller.lock().await;
        ctrl.check(score)
    };

    match result {
        CheckResult::Emit => {
            emit_message(
                js,
                controller,
                pipeline_config,
                score,
                &msg.payload,
                msg.headers.clone(),
            )
            .await;
        }
        CheckResult::Reject => {
            m::record_rejected();
            debug!(score, "message rejected");
        }
        CheckResult::Buffer => {
            let mut buf = buffer.lock().await;
            buf.push(BufferedMessage {
                score,
                arrived: Instant::now(),
                payload: msg.payload.clone(),
                headers: msg.headers.clone().unwrap_or_default(),
            });
            m::set_buffer_size(buf.len());
        }
    }
}

async fn emit_message(
    js: &jetstream::Context,
    controller: &Arc<Mutex<ThresholdController>>,
    pipeline_config: &PipelineConfig,
    score: f64,
    payload: &[u8],
    original_headers: Option<async_nats::HeaderMap>,
) {
    m::record_emitted();
    update_score_ema(score);

    let mut headers = original_headers.unwrap_or_default();
    let ctrl = controller.lock().await;
    headers.insert(
        THRESHOLD_HEADER,
        format!("{:.6}", ctrl.threshold()).as_str(),
    );
    headers.insert(STATE_HEADER, ctrl.state_name());
    drop(ctrl);

    if let Err(e) = js
        .publish_with_headers(
            pipeline_config.output_subject.clone(),
            headers,
            bytes::Bytes::copy_from_slice(payload),
        )
        .await
    {
        error!(error = %e, "failed to publish to output stream");
    }
}

fn update_score_ema(score: f64) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static EMA: AtomicU64 = AtomicU64::new(0);
    const ALPHA: f64 = 0.01;

    let current = f64::from_bits(EMA.load(Ordering::Relaxed));
    let new = if current == 0.0 {
        score
    } else {
        ALPHA * score + (1.0 - ALPHA) * current
    };
    EMA.store(new.to_bits(), Ordering::Relaxed);
    m::set_avg_emitted_score(new);
}

fn update_latency_ema(latency_ms: f64) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static EMA: AtomicU64 = AtomicU64::new(0);
    const ALPHA: f64 = 0.01;

    let current = f64::from_bits(EMA.load(Ordering::Relaxed));
    let new = if current == 0.0 {
        latency_ms
    } else {
        ALPHA * latency_ms + (1.0 - ALPHA) * current
    };
    EMA.store(new.to_bits(), Ordering::Relaxed);
    m::set_avg_latency_ms(new);
}

pub async fn run_buffer_drainer(
    js: jetstream::Context,
    controller: Arc<Mutex<ThresholdController>>,
    buffer: Arc<Mutex<MessageBuffer>>,
    pipeline_config: Arc<PipelineConfig>,
    mut config_rx: watch::Receiver<Config>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
) {
    let config = config_rx.borrow_and_update().clone();
    let mut interval = tokio::time::interval(Duration::from_millis(config.drain_interval_ms));
    let mut batch_interval =
        tokio::time::interval(Duration::from_millis(config.max_buffer_duration_ms));
    let mut current_algorithm = config.algorithm;

    loop {
        tokio::select! {
            _ = interval.tick(), if current_algorithm == Algorithm::BufferedStreaming => {
                let mut buf = buffer.lock().await;
                let evicted = buf.evict_expired(Instant::now());
                if evicted > 0 {
                    m::record_evicted(evicted);
                }
                if let Some(msg) = buf.drain_one() {
                    let latency = msg.arrived.elapsed().as_millis() as f64;
                    m::set_buffer_size(buf.len());
                    drop(buf);

                    update_latency_ema(latency);

                    let mut ctrl = controller.lock().await;
                    ctrl.record_emission();
                    drop(ctrl);

                    emit_message(
                        &js, &controller, &pipeline_config,
                        msg.score, &msg.payload, Some(msg.headers),
                    ).await;
                } else {
                    m::set_buffer_size(0);
                }
            }
            _ = batch_interval.tick(), if current_algorithm == Algorithm::BufferedBatch => {
                let config = config_rx.borrow().clone();
                let n = (config.target_rate * config.max_buffer_duration_ms as f64 / 1000.0)
                    .round() as usize;
                let mut buf = buffer.lock().await;
                let batch = buf.drain_batch(n, Instant::now());
                let evicted_remaining = buf.len();
                m::set_buffer_size(0);
                drop(buf);

                if evicted_remaining > 0 {
                    m::record_evicted(evicted_remaining);
                }

                for msg in batch {
                    let latency = msg.arrived.elapsed().as_millis() as f64;
                    update_latency_ema(latency);

                    let mut ctrl = controller.lock().await;
                    ctrl.record_emission();
                    drop(ctrl);

                    emit_message(
                        &js, &controller, &pipeline_config,
                        msg.score, &msg.payload, Some(msg.headers),
                    ).await;
                }
            }
            result = config_rx.changed() => {
                if result.is_err() {
                    break;
                }
                let config = config_rx.borrow_and_update().clone();
                current_algorithm = config.algorithm;
                interval = tokio::time::interval(Duration::from_millis(config.drain_interval_ms));
                batch_interval = tokio::time::interval(Duration::from_millis(
                    config.max_buffer_duration_ms,
                ));
                let mut buf = buffer.lock().await;
                buf.set_max_duration(config.max_buffer_duration_ms);
                info!(algorithm = ?current_algorithm, "buffer drainer config updated");
            }
            _ = shutdown.recv() => {
                info!("buffer drainer shutting down");
                break;
            }
        }
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
