use std::collections::HashMap;
use std::time::Duration;

use clap::Parser;
use rand::Rng;
use rand_distr::{Beta, Distribution, Normal, Uniform};
use serde::Serialize;
use tokio::time::Instant;
use tracing::info;

#[derive(Parser)]
#[command(name = "flowgate-producer", about = "Synthetic prediction generator")]
struct Args {
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[arg(long, env = "SUBJECT", default_value = "flowgate.in.synthetic")]
    subject: String,

    #[arg(long, env = "DISTRIBUTION", default_value = "beta")]
    distribution: DistributionType,

    #[arg(long, env = "RATE", default_value_t = 100.0)]
    rate: f64,

    #[arg(long, env = "BURST_INTERVAL", default_value_t = 0.0)]
    burst_interval: f64,

    #[arg(long, env = "BURST_MULTIPLIER", default_value_t = 5.0)]
    burst_multiplier: f64,

    #[arg(long, env = "BURST_DURATION", default_value_t = 2.0)]
    burst_duration: f64,
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
            _ => Err(format!(
                "unknown distribution: {s} (expected: normal, beta, uniform, bimodal)"
            )),
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

#[derive(Serialize)]
struct Envelope<'a> {
    score: f64,
    metadata: HashMap<&'a str, serde_json::Value>,
    payload: serde_json::Value,
}

fn sample_score(rng: &mut impl Rng, dist: &DistributionType) -> f64 {
    let raw: f64 = match dist {
        DistributionType::Normal => Normal::new(0.5, 0.15).unwrap().sample(rng),
        DistributionType::Beta => Beta::new(2.0, 5.0).unwrap().sample(rng),
        DistributionType::Uniform => Uniform::new(0.0, 1.0).unwrap().sample(rng),
        DistributionType::Bimodal => {
            if rng.random_bool(0.5) {
                Normal::new(0.3, 0.1).unwrap().sample(rng)
            } else {
                Normal::new(0.8, 0.1).unwrap().sample(rng)
            }
        }
    };
    raw.clamp(0.0, 1.0)
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

    let client = async_nats::connect(&args.nats_url).await?;
    let js = async_nats::jetstream::new(client);
    info!(
        url = %args.nats_url,
        subject = %args.subject,
        distribution = %args.distribution,
        rate = args.rate,
        "producer starting"
    );

    let mut rng = rand::rng();
    let base_interval = Duration::from_secs_f64(1.0 / args.rate);
    let burst_interval = if args.burst_interval > 0.0 {
        Some(Duration::from_secs_f64(args.burst_interval))
    } else {
        None
    };
    let burst_duration = Duration::from_secs_f64(args.burst_duration);

    let mut seq: u64 = 0;
    let start = Instant::now();
    let mut next_burst = burst_interval.map(|bi| start + bi);
    let mut in_burst = false;
    let mut burst_end = start;

    loop {
        let now = Instant::now();

        if let Some(nb) = next_burst {
            if now >= nb && !in_burst {
                in_burst = true;
                burst_end = now + burst_duration;
                info!("burst started");
            }
        }
        if in_burst && now >= burst_end {
            in_burst = false;
            next_burst = burst_interval.map(|bi| now + bi);
            info!("burst ended");
        }

        let current_interval = if in_burst {
            Duration::from_secs_f64(1.0 / (args.rate * args.burst_multiplier))
        } else {
            base_interval
        };

        let score = sample_score(&mut rng, &args.distribution);
        seq += 1;

        let envelope = Envelope {
            score,
            metadata: HashMap::from([
                (
                    "source",
                    serde_json::Value::String("flowgate-producer".into()),
                ),
                (
                    "distribution",
                    serde_json::Value::String(args.distribution.to_string()),
                ),
                ("seq", serde_json::json!(seq)),
            ]),
            payload: serde_json::json!({
                "demo": true,
                "seq": seq,
                "elapsed_ms": start.elapsed().as_millis() as u64,
            }),
        };

        let data = serde_json::to_vec(&envelope)?;
        js.publish(args.subject.clone(), data.into()).await?;

        if seq.is_multiple_of(1000) {
            info!(seq, score, in_burst, "published 1000 messages");
        }

        tokio::time::sleep(current_interval).await;
    }
}
