mod api;
mod ws;

use std::sync::Arc;

use axum::Router;
use clap::Parser;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

#[derive(Parser)]
#[command(name = "flowgate-dashboard")]
struct Args {
    #[arg(long, env = "NATS_URL", default_value = "nats://localhost:4222")]
    nats_url: String,

    #[arg(long, env = "PORT", default_value_t = 3000)]
    port: u16,

    #[arg(
        long,
        env = "FLOWGATE_A_METRICS",
        default_value = "http://localhost:9090"
    )]
    flowgate_a_metrics: String,

    #[arg(
        long,
        env = "FLOWGATE_B_METRICS",
        default_value = "http://localhost:9091"
    )]
    flowgate_b_metrics: String,

    #[arg(long, env = "KV_BUCKET_A", default_value = "flowgate-config-a")]
    kv_bucket_a: String,

    #[arg(long, env = "KV_BUCKET_B", default_value = "flowgate-config-b")]
    kv_bucket_b: String,

    #[arg(long, env = "STATIC_DIR", default_value = "./dashboard/dist")]
    static_dir: String,
}

pub struct AppState {
    pub js: async_nats::jetstream::Context,
    pub kv_bucket_a: String,
    pub kv_bucket_b: String,
    pub flowgate_a_metrics: String,
    pub flowgate_b_metrics: String,
    pub event_tx: broadcast::Sender<ws::Event>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "flowgate_dashboard=info".into()),
        )
        .init();

    let args = Args::parse();

    let client = async_nats::connect(&args.nats_url).await?;
    info!(url = %args.nats_url, "connected to NATS");

    let js = async_nats::jetstream::new(client.clone());

    let (event_tx, _) = broadcast::channel::<ws::Event>(4096);

    let state = Arc::new(AppState {
        js,
        kv_bucket_a: args.kv_bucket_a,
        kv_bucket_b: args.kv_bucket_b,
        flowgate_a_metrics: args.flowgate_a_metrics.clone(),
        flowgate_b_metrics: args.flowgate_b_metrics.clone(),
        event_tx: event_tx.clone(),
    });

    // Spawn NATS subscribers that feed events to WebSocket clients
    let js_clone = state.js.clone();
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        if let Err(e) =
            ws::subscribe_output_stream(js_clone, "flowgate.out.threshold", "a", tx_clone).await
        {
            tracing::error!(error = %e, "output stream subscriber A failed");
        }
    });

    let js_clone = state.js.clone();
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        if let Err(e) =
            ws::subscribe_output_stream(js_clone, "flowgate.out.buffered", "b", tx_clone).await
        {
            tracing::error!(error = %e, "output stream subscriber B failed");
        }
    });

    // Spawn metrics scraper
    let metrics_a = args.flowgate_a_metrics.clone();
    let metrics_b = args.flowgate_b_metrics.clone();
    let tx_clone = event_tx.clone();
    tokio::spawn(async move {
        ws::scrape_metrics_loop(metrics_a, metrics_b, tx_clone).await;
    });

    let app = Router::new()
        .nest("/api", api::router(state.clone()))
        .route("/ws", axum::routing::get(ws::handler))
        .fallback_service(ServeDir::new(&args.static_dir))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", args.port)).await?;
    info!(port = args.port, "dashboard server starting");

    axum::serve(listener, app).await?;

    Ok(())
}
