mod config;
mod envelope;
mod metrics;
mod pid;
mod pipeline;
mod threshold;

use std::sync::Arc;

use clap::Parser;
use tokio::sync::{broadcast, watch, Mutex};
use tracing::info;

#[derive(Parser)]
#[command(name = "flowgate", about = "Adaptive threshold rate controller")]
struct Args {
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[arg(long, env = "METRICS_PORT", default_value_t = 9090)]
    metrics_port: u16,
}

#[tokio::main]
async fn main() -> Result<(), async_nats::Error> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "flowgate=info".into()),
        )
        .init();

    let args = Args::parse();

    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    builder
        .with_http_listener(([0, 0, 0, 0], args.metrics_port))
        .install()
        .expect("failed to install prometheus exporter");

    metrics::register();
    info!(
        metrics_port = args.metrics_port,
        "prometheus metrics server started"
    );

    let client = async_nats::connect(&args.nats_url).await?;
    info!(url = %args.nats_url, "connected to NATS");

    let js = async_nats::jetstream::new(client.clone());

    pipeline::setup_streams(&js).await?;

    let kv_store = js
        .create_key_value(async_nats::jetstream::kv::Config {
            bucket: config::KV_BUCKET.to_string(),
            history: 5,
            ..Default::default()
        })
        .await?;
    info!("KV bucket ready");

    let initial_config = config::load_initial_config(&kv_store).await;
    let (config_tx, config_rx) = watch::channel(initial_config.clone());

    let controller = Arc::new(Mutex::new(threshold::ThresholdController::new(
        &initial_config,
    )));

    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    let config_watcher = {
        let store = kv_store.clone();
        tokio::spawn(async move {
            if let Err(e) = config::watch_config(store, config_tx).await {
                tracing::error!(error = %e, "config watcher failed");
            }
        })
    };

    let pid_ticker = {
        let controller = controller.clone();
        let config_rx = config_rx.clone();
        let shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            pipeline::run_pid_ticker(controller, config_rx, shutdown_rx).await;
        })
    };

    let consumer = {
        let controller = controller.clone();
        let shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            if let Err(e) = pipeline::run_consumer(js, controller, config_rx, shutdown_rx).await {
                tracing::error!(error = %e, "consumer failed");
            }
        })
    };

    info!("flowgate running");

    tokio::signal::ctrl_c().await?;
    info!("shutdown signal received");

    let _ = shutdown_tx.send(());

    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        futures::future::join_all([consumer, pid_ticker]),
    )
    .await;

    config_watcher.abort();

    info!("flowgate stopped");
    Ok(())
}
