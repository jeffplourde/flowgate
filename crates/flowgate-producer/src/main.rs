use std::time::Duration;

use async_nats::jetstream;
use clap::Parser;
use futures::StreamExt;
use metrics::{counter, describe_counter, describe_gauge, gauge};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rand_distr::{Beta, Distribution, Normal, Uniform};
use tokio::sync::watch;
use tracing::{info, warn};

#[derive(Parser)]
#[command(
    name = "flowgate-producer",
    about = "Multi-client batch prediction simulator"
)]
struct Args {
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[arg(long, env = "SUBJECT", default_value = "flowgate.in.synthetic")]
    subject: String,

    #[arg(
        long,
        env = "NATS_KV_BUCKET",
        default_value = "flowgate-producer-config"
    )]
    kv_bucket: String,

    #[arg(long, env = "METRICS_PORT", default_value_t = 9090)]
    metrics_port: u16,
}

fn register_metrics() {
    describe_counter!(
        "producer_messages_published_total",
        "Messages successfully published"
    );
    describe_counter!(
        "producer_publish_errors_total",
        "Publish failures (backpressure/storage full)"
    );
    describe_counter!(
        "producer_batches_sent_total",
        "Batches sent across all clients"
    );
    describe_gauge!(
        "producer_active_clients",
        "Number of active simulated clients"
    );
    describe_gauge!(
        "producer_backpressure",
        "1 if recent publishes are failing, 0 if healthy"
    );
}

#[derive(Debug, Clone)]
struct ProducerConfig {
    num_clients: u64,
    time_compression: f64,
    min_batch_size: u64,
    max_batch_size: u64,
    distribution: DistributionType,
    distribution_variance: f64,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            num_clients: 70,
            time_compression: 60.0,
            min_batch_size: 100,
            max_batch_size: 5000,
            distribution: DistributionType::Beta,
            distribution_variance: 0.3,
        }
    }
}

impl ProducerConfig {
    fn apply_kv_entry(&mut self, key: &str, value: &str) {
        match key {
            "num_clients" => {
                if let Ok(v) = value.parse() {
                    self.num_clients = v;
                }
            }
            "time_compression" => {
                if let Ok(v) = value.parse() {
                    self.time_compression = v;
                }
            }
            "min_batch_size" => {
                if let Ok(v) = value.parse() {
                    self.min_batch_size = v;
                }
            }
            "max_batch_size" => {
                if let Ok(v) = value.parse() {
                    self.max_batch_size = v;
                }
            }
            "distribution" => {
                if let Ok(v) = value.parse() {
                    self.distribution = v;
                }
            }
            "distribution_variance" => {
                if let Ok(v) = value.parse() {
                    self.distribution_variance = v;
                }
            }
            _ => {}
        }
    }

    fn batch_interval(&self) -> Duration {
        Duration::from_secs_f64(3600.0 / self.time_compression)
    }
}

#[derive(Clone, Debug)]
enum DistributionType {
    Normal,
    Beta,
    Uniform,
    Bimodal,
}

impl std::str::FromStr for DistributionType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "normal" => Ok(Self::Normal),
            "beta" => Ok(Self::Beta),
            "uniform" => Ok(Self::Uniform),
            "bimodal" => Ok(Self::Bimodal),
            _ => Err(format!("unknown distribution: {s}")),
        }
    }
}

impl std::fmt::Display for DistributionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "normal"),
            Self::Beta => write!(f, "beta"),
            Self::Uniform => write!(f, "uniform"),
            Self::Bimodal => write!(f, "bimodal"),
        }
    }
}

fn sample_score(rng: &mut impl Rng, dist: &DistributionType, variance_shift: f64) -> f64 {
    let raw: f64 = match dist {
        DistributionType::Normal => {
            let mean = 0.5 + variance_shift * 0.2;
            Normal::new(mean.clamp(0.1, 0.9), 0.15).unwrap().sample(rng)
        }
        DistributionType::Beta => {
            let a = (2.0 + variance_shift * 2.0).max(0.5);
            let b = (5.0 - variance_shift * 2.0).max(0.5);
            Beta::new(a, b).unwrap().sample(rng)
        }
        DistributionType::Uniform => Uniform::new(0.0, 1.0).unwrap().sample(rng),
        DistributionType::Bimodal => {
            let low_mean = 0.3 + variance_shift * 0.1;
            let high_mean = 0.8 - variance_shift * 0.1;
            if rng.random_bool(0.5) {
                Normal::new(low_mean.clamp(0.1, 0.5), 0.1)
                    .unwrap()
                    .sample(rng)
            } else {
                Normal::new(high_mean.clamp(0.5, 0.95), 0.1)
                    .unwrap()
                    .sample(rng)
            }
        }
    };
    raw.clamp(0.0, 1.0)
}

async fn load_config(store: &async_nats::jetstream::kv::Store) -> ProducerConfig {
    let mut config = ProducerConfig::default();
    let keys = [
        "num_clients",
        "time_compression",
        "min_batch_size",
        "max_batch_size",
        "distribution",
        "distribution_variance",
    ];
    for key in keys {
        if let Ok(Some(bytes)) = store.get(key).await {
            if let Ok(value) = std::str::from_utf8(&bytes) {
                config.apply_kv_entry(key, value);
            }
        }
    }
    config
}

async fn watch_config(store: async_nats::jetstream::kv::Store, tx: watch::Sender<ProducerConfig>) {
    let Ok(mut watcher) = store.watch_all().await else {
        warn!("failed to start producer config watcher");
        return;
    };

    while let Some(entry) = watcher.next().await {
        if let Ok(entry) = entry {
            if let Ok(value) = std::str::from_utf8(&entry.value) {
                let mut config = tx.borrow().clone();
                config.apply_kv_entry(&entry.key, value);
                info!(key = %entry.key, value, "producer config updated");
                let _ = tx.send(config);
            }
        }
    }
}

async fn client_loop(
    client_id: u64,
    js: jetstream::Context,
    subject: String,
    config_rx: watch::Receiver<ProducerConfig>,
    mut shutdown: tokio::sync::broadcast::Receiver<()>,
    phase_offset: f64,
    variance_shift: f64,
) {
    let mut rng = StdRng::seed_from_u64(client_id);

    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs_f64(phase_offset)) => {}
        _ = shutdown.recv() => { return; }
    }

    loop {
        let config = config_rx.borrow().clone();
        let batch_size = rng.random_range(config.min_batch_size..=config.max_batch_size);
        let batch_id = uuid::Uuid::new_v4().to_string();

        info!(
            client_id,
            batch_size,
            batch_id = %batch_id,
            "sending batch"
        );

        for seq in 0..batch_size {
            let score = sample_score(&mut rng, &config.distribution, variance_shift);

            let mut headers = async_nats::HeaderMap::new();
            headers.insert("Flowgate-Score", format!("{score:.6}").as_str());
            headers.insert("Flowgate-Client-Id", client_id.to_string().as_str());
            headers.insert("Flowgate-Batch-Id", batch_id.as_str());
            headers.insert("Flowgate-Batch-Seq", seq.to_string().as_str());
            headers.insert("Flowgate-Batch-Size", batch_size.to_string().as_str());

            let payload = format!(r#"{{"client":{client_id},"seq":{seq}}}"#);

            match js
                .publish_with_headers(subject.clone(), headers, payload.into())
                .await
            {
                Ok(_) => {
                    counter!("producer_messages_published_total").increment(1);
                    gauge!("producer_backpressure").set(0.0);
                }
                Err(e) => {
                    counter!("producer_publish_errors_total").increment(1);
                    gauge!("producer_backpressure").set(1.0);
                    warn!(client_id, error = %e, "publish failed (backpressure?)");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }

        counter!("producer_batches_sent_total").increment(1);

        let interval = config.batch_interval();
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = shutdown.recv() => { return; }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), async_nats::Error> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "flowgate_producer=info".into()),
        )
        .init();

    let args = Args::parse();

    metrics_exporter_prometheus::PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], args.metrics_port))
        .install()
        .expect("failed to install prometheus exporter");
    register_metrics();

    let client = async_nats::connect(&args.nats_url).await?;
    let js = async_nats::jetstream::new(client);
    info!(url = %args.nats_url, "connected to NATS");

    let kv_store = js
        .create_key_value(async_nats::jetstream::kv::Config {
            bucket: args.kv_bucket.clone(),
            history: 5,
            ..Default::default()
        })
        .await?;

    let initial_config = load_config(&kv_store).await;
    let (config_tx, config_rx) = watch::channel(initial_config.clone());

    info!(
        num_clients = initial_config.num_clients,
        time_compression = initial_config.time_compression,
        batch_interval_secs = initial_config.batch_interval().as_secs_f64(),
        "producer starting"
    );

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    tokio::spawn({
        let store = kv_store.clone();
        async move {
            watch_config(store, config_tx).await;
        }
    });

    let mut rng = rand::rng();
    let mut client_handles = Vec::new();
    for id in 0..initial_config.num_clients {
        let js = js.clone();
        let subject = args.subject.clone();
        let config_rx = config_rx.clone();
        let shutdown_rx = shutdown_tx.subscribe();
        let phase_offset = rng.random_range(0.0..initial_config.batch_interval().as_secs_f64());
        let variance_shift =
            (rng.random::<f64>() - 0.5) * 2.0 * initial_config.distribution_variance;
        client_handles.push(tokio::spawn(async move {
            client_loop(
                id,
                js,
                subject,
                config_rx,
                shutdown_rx,
                phase_offset,
                variance_shift,
            )
            .await;
        }));
    }

    gauge!("producer_active_clients").set(initial_config.num_clients as f64);
    info!(
        num_clients = initial_config.num_clients,
        "all clients spawned"
    );

    tokio::signal::ctrl_c().await?;
    info!("shutting down");
    let _ = shutdown_tx.send(());

    for handle in client_handles {
        let _ = handle.await;
    }

    Ok(())
}
