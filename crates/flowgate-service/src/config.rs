use async_nats::jetstream::kv;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub target_rate: f64,
    pub measurement_window_secs: f64,
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub fallback_threshold: Option<f64>,
    pub min_threshold: f64,
    pub max_threshold: f64,
    pub algorithm: Algorithm,
    pub warmup_samples: u64,
    pub anti_windup_limit: f64,
    pub pid_interval_ms: u64,
    pub max_buffer_duration_ms: u64,
    pub drain_interval_ms: u64,
    pub min_quality_score: f64,
    pub backpressure_threshold_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Algorithm {
    Pid,
    Fixed,
    BufferedBatch,
    BufferedStreaming,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_rate: 10.0,
            measurement_window_secs: 10.0,
            kp: 0.0004,
            ki: 0.00004,
            kd: 0.00001,
            fallback_threshold: None,
            min_threshold: 0.0,
            max_threshold: 1.0,
            algorithm: Algorithm::Pid,
            warmup_samples: 100,
            anti_windup_limit: 100.0,
            pid_interval_ms: 1000,
            max_buffer_duration_ms: 5000,
            drain_interval_ms: 100,
            min_quality_score: 0.0,
            backpressure_threshold_ms: 500,
        }
    }
}

impl Config {
    pub fn is_buffered(&self) -> bool {
        matches!(
            self.algorithm,
            Algorithm::BufferedBatch | Algorithm::BufferedStreaming
        )
    }

    fn apply_kv_entry(&mut self, key: &str, value: &str) {
        match key {
            "target_rate" => {
                if let Ok(v) = value.parse() {
                    self.target_rate = v;
                }
            }
            "measurement_window_secs" => {
                if let Ok(v) = value.parse() {
                    self.measurement_window_secs = v;
                }
            }
            "kp" => {
                if let Ok(v) = value.parse() {
                    self.kp = v;
                }
            }
            "ki" => {
                if let Ok(v) = value.parse() {
                    self.ki = v;
                }
            }
            "kd" => {
                if let Ok(v) = value.parse() {
                    self.kd = v;
                }
            }
            "fallback_threshold" => {
                if value.is_empty() || value == "none" {
                    self.fallback_threshold = None;
                } else if let Ok(v) = value.parse() {
                    self.fallback_threshold = Some(v);
                }
            }
            "min_threshold" => {
                if let Ok(v) = value.parse() {
                    self.min_threshold = v;
                }
            }
            "max_threshold" => {
                if let Ok(v) = value.parse() {
                    self.max_threshold = v;
                }
            }
            "algorithm" => match value {
                "pid" => self.algorithm = Algorithm::Pid,
                "fixed" => self.algorithm = Algorithm::Fixed,
                "buffered_batch" => self.algorithm = Algorithm::BufferedBatch,
                "buffered_streaming" => self.algorithm = Algorithm::BufferedStreaming,
                _ => warn!(key, value, "unknown algorithm value"),
            },
            "warmup_samples" => {
                if let Ok(v) = value.parse() {
                    self.warmup_samples = v;
                }
            }
            "anti_windup_limit" => {
                if let Ok(v) = value.parse() {
                    self.anti_windup_limit = v;
                }
            }
            "pid_interval_ms" => {
                if let Ok(v) = value.parse() {
                    self.pid_interval_ms = v;
                }
            }
            "max_buffer_duration_ms" => {
                if let Ok(v) = value.parse() {
                    self.max_buffer_duration_ms = v;
                }
            }
            "drain_interval_ms" => {
                if let Ok(v) = value.parse() {
                    self.drain_interval_ms = v;
                }
            }
            "min_quality_score" => {
                if let Ok(v) = value.parse() {
                    self.min_quality_score = v;
                }
            }
            "backpressure_threshold_ms" => {
                if let Ok(v) = value.parse() {
                    self.backpressure_threshold_ms = v;
                }
            }
            _ => {
                warn!(key, "unknown config key");
            }
        }
    }
}

pub async fn load_initial_config(store: &kv::Store) -> Config {
    let mut config = Config::default();

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
        "min_quality_score",
        "backpressure_threshold_ms",
    ];

    for key in keys {
        match store.get(key).await {
            Ok(Some(bytes)) => {
                if let Ok(value) = std::str::from_utf8(&bytes) {
                    config.apply_kv_entry(key, value);
                }
            }
            Ok(None) => {}
            Err(e) => {
                warn!(key, error = %e, "failed to read config key");
            }
        }
    }

    info!(?config, "loaded initial config");
    config
}

pub async fn watch_config(
    store: kv::Store,
    tx: watch::Sender<Config>,
) -> Result<(), async_nats::Error> {
    let mut watcher = store.watch_all().await?;
    info!("config watcher started");

    while let Some(entry) = watcher.next().await {
        match entry {
            Ok(entry) => {
                if let Ok(value) = std::str::from_utf8(&entry.value) {
                    let mut config = tx.borrow().clone();
                    config.apply_kv_entry(&entry.key, value);
                    info!(key = %entry.key, value, "config updated");
                    let _ = tx.send(config);
                }
            }
            Err(e) => {
                warn!(error = %e, "config watch error");
            }
        }
    }

    warn!("config watcher ended");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_known_keys() {
        let mut config = Config::default();
        config.apply_kv_entry("target_rate", "50.0");
        assert!((config.target_rate - 50.0).abs() < f64::EPSILON);

        config.apply_kv_entry("algorithm", "fixed");
        assert_eq!(config.algorithm, Algorithm::Fixed);

        config.apply_kv_entry("algorithm", "buffered_batch");
        assert_eq!(config.algorithm, Algorithm::BufferedBatch);

        config.apply_kv_entry("algorithm", "buffered_streaming");
        assert_eq!(config.algorithm, Algorithm::BufferedStreaming);

        config.apply_kv_entry("max_buffer_duration_ms", "3000");
        assert_eq!(config.max_buffer_duration_ms, 3000);

        config.apply_kv_entry("drain_interval_ms", "50");
        assert_eq!(config.drain_interval_ms, 50);

        config.apply_kv_entry("fallback_threshold", "0.75");
        assert_eq!(config.fallback_threshold, Some(0.75));

        config.apply_kv_entry("fallback_threshold", "none");
        assert_eq!(config.fallback_threshold, None);
    }

    #[test]
    fn invalid_values_ignored() {
        let mut config = Config::default();
        let orig_rate = config.target_rate;
        config.apply_kv_entry("target_rate", "not_a_number");
        assert!((config.target_rate - orig_rate).abs() < f64::EPSILON);
    }
}
