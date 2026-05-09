use std::collections::VecDeque;
use std::time::Instant;

use crate::config::{Algorithm, Config};
use crate::metrics as m;
use crate::pid::{PidController, PidParams};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    Emit,
    Reject,
    Buffer,
}

#[derive(Debug)]
enum State {
    ColdStart,
    Warmup { scores: Vec<f64> },
    Pid,
    Fixed,
}

#[derive(Debug)]
pub struct ThresholdController {
    state: State,
    pid: PidController,
    threshold: f64,
    emission_timestamps: VecDeque<Instant>,
    last_tick: Instant,
    config: Config,
}

impl ThresholdController {
    pub fn new(config: &Config) -> Self {
        let pid_params = pid_params_from_config(config);
        let initial_threshold = config
            .fallback_threshold
            .unwrap_or((config.min_threshold + config.max_threshold) / 2.0);

        let initial_state = match (config.algorithm, config.fallback_threshold) {
            (Algorithm::Fixed, _)
            | (Algorithm::BufferedBatch, _)
            | (Algorithm::BufferedStreaming, _) => {
                m::set_controller_state(m::ControllerState::Fixed);
                State::Fixed
            }
            (Algorithm::Pid, Some(_)) => {
                m::set_controller_state(m::ControllerState::ColdStart);
                State::ColdStart
            }
            (Algorithm::Pid, None) => {
                m::set_controller_state(m::ControllerState::Warmup);
                State::Warmup { scores: Vec::new() }
            }
        };

        Self {
            state: initial_state,
            pid: PidController::new(pid_params, initial_threshold),
            threshold: initial_threshold,
            emission_timestamps: VecDeque::new(),
            last_tick: Instant::now(),
            config: config.clone(),
        }
    }

    pub fn threshold(&self) -> f64 {
        self.threshold
    }

    pub fn check(&mut self, score: f64) -> CheckResult {
        if self.config.is_buffered() {
            return CheckResult::Buffer;
        }

        match &mut self.state {
            State::Warmup { scores } => {
                scores.push(score);
                m::set_warmup_samples(scores.len() as u64);

                if scores.len() as u64 >= self.config.warmup_samples {
                    let threshold = compute_percentile_threshold(
                        scores,
                        self.config.target_rate,
                        self.config.measurement_window_secs,
                    );
                    self.threshold =
                        threshold.clamp(self.config.min_threshold, self.config.max_threshold);
                    self.pid.set_output(self.threshold);
                    self.state = State::Pid;
                    self.last_tick = Instant::now();
                    m::set_controller_state(m::ControllerState::Pid);
                    m::set_threshold(self.threshold);
                    tracing::info!(
                        threshold = self.threshold,
                        "warmup complete, transitioning to PID"
                    );
                }

                if score >= self.threshold {
                    CheckResult::Emit
                } else {
                    CheckResult::Reject
                }
            }
            State::ColdStart => {
                let passes = score >= self.threshold;
                if passes {
                    self.emission_timestamps.push_back(Instant::now());
                }
                if passes {
                    CheckResult::Emit
                } else {
                    CheckResult::Reject
                }
            }
            State::Pid | State::Fixed => {
                let passes = score >= self.threshold;
                if passes {
                    self.emission_timestamps.push_back(Instant::now());
                }
                if passes {
                    CheckResult::Emit
                } else {
                    CheckResult::Reject
                }
            }
        }
    }

    /// Record that a message was emitted (called by the buffer drainer
    /// in buffered modes to keep the emission rate tracking accurate).
    pub fn record_emission(&mut self) {
        self.emission_timestamps.push_back(Instant::now());
    }

    /// Called on the PID tick interval. Updates the threshold based on emission rate.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;

        // Prune old emission timestamps outside the measurement window
        let window_start =
            now - std::time::Duration::from_secs_f64(self.config.measurement_window_secs);
        while self
            .emission_timestamps
            .front()
            .is_some_and(|t| *t < window_start)
        {
            self.emission_timestamps.pop_front();
        }

        let actual_rate =
            self.emission_timestamps.len() as f64 / self.config.measurement_window_secs;
        m::set_actual_rate(actual_rate);
        m::set_target_rate(self.config.target_rate);

        match &self.state {
            State::ColdStart => {
                // Transition to PID after first tick with some data
                if !self.emission_timestamps.is_empty() {
                    self.state = State::Pid;
                    m::set_controller_state(m::ControllerState::Pid);
                    tracing::info!("cold start complete, transitioning to PID");
                }
            }
            State::Pid => {
                let error = self.config.target_rate - actual_rate;
                m::set_pid_error(error);

                self.threshold = self.pid.update(error, dt);

                let p_term = self.config.kp * error;
                let i_term = self.config.ki * self.pid.integral();
                let d_term = self.config.kd * (error - self.pid.prev_error());
                m::set_pid_terms(p_term, i_term, d_term);
                m::set_threshold(self.threshold);
            }
            State::Warmup { .. } | State::Fixed => {}
        }
    }

    /// Update the controller when config changes.
    pub fn update_config(&mut self, config: &Config) {
        let old_algorithm = self.config.algorithm;
        self.config = config.clone();

        self.pid.update_params(pid_params_from_config(config));

        match (old_algorithm, config.algorithm) {
            (_, Algorithm::Fixed)
            | (_, Algorithm::BufferedBatch)
            | (_, Algorithm::BufferedStreaming) => {
                if let Some(t) = config.fallback_threshold {
                    self.threshold = t;
                    m::set_threshold(t);
                }
                self.state = State::Fixed;
                m::set_controller_state(m::ControllerState::Fixed);
            }
            (_, Algorithm::Pid) if old_algorithm != Algorithm::Pid => {
                self.pid.set_output(self.threshold);
                self.pid.reset();
                self.state = State::Pid;
                m::set_controller_state(m::ControllerState::Pid);
            }
            _ => {}
        }
    }

    #[allow(dead_code)]
    pub fn is_buffering(&self) -> bool {
        matches!(self.state, State::Warmup { .. })
    }

    pub fn state_name(&self) -> &'static str {
        match self.state {
            State::ColdStart => "cold_start",
            State::Warmup { .. } => "warmup",
            State::Pid => "pid",
            State::Fixed => "fixed",
        }
    }
}

fn pid_params_from_config(config: &Config) -> PidParams {
    PidParams {
        kp: config.kp,
        ki: config.ki,
        kd: config.kd,
        min_output: config.min_threshold,
        max_output: config.max_threshold,
        anti_windup_limit: config.anti_windup_limit,
    }
}

fn compute_percentile_threshold(scores: &mut [f64], target_rate: f64, window_secs: f64) -> f64 {
    if scores.is_empty() {
        return 0.5;
    }

    scores.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let total = scores.len() as f64;
    let expected_per_window = target_rate * window_secs;
    let pass_fraction = (expected_per_window / total).clamp(0.0, 1.0);

    // We want the threshold where `pass_fraction` of scores are above it
    let index = ((1.0 - pass_fraction) * (total - 1.0)) as usize;
    scores[index.min(scores.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_threshold_computation() {
        let mut scores: Vec<f64> = (0..100).map(|i| i as f64 / 100.0).collect();
        // target_rate=10/s, window=10s => want 100 per window => 100% pass => threshold ~0
        let t = compute_percentile_threshold(&mut scores, 10.0, 10.0);
        assert!(t < 0.05, "should be near 0 when all should pass, got {t}");

        let mut scores: Vec<f64> = (0..100).map(|i| i as f64 / 100.0).collect();
        // target_rate=1/s, window=10s => want 10 per window => 10% pass => threshold ~0.9
        let t = compute_percentile_threshold(&mut scores, 1.0, 10.0);
        assert!(t > 0.85 && t < 0.95, "expected ~0.9, got {t}");
    }

    #[test]
    fn fixed_mode_uses_fallback() {
        let config = Config {
            algorithm: Algorithm::Fixed,
            fallback_threshold: Some(0.7),
            ..Config::default()
        };
        let controller = ThresholdController::new(&config);
        assert!((controller.threshold() - 0.7).abs() < f64::EPSILON);
        assert_eq!(controller.state_name(), "fixed");
    }

    #[test]
    fn warmup_when_no_fallback() {
        let config = Config {
            algorithm: Algorithm::Pid,
            fallback_threshold: None,
            warmup_samples: 5,
            ..Config::default()
        };
        let mut controller = ThresholdController::new(&config);
        assert_eq!(controller.state_name(), "warmup");
        assert!(controller.is_buffering());

        for i in 0..5 {
            controller.check(i as f64 / 5.0);
        }
        assert_eq!(controller.state_name(), "pid");
        assert!(!controller.is_buffering());
    }

    #[test]
    fn cold_start_with_fallback() {
        let config = Config {
            algorithm: Algorithm::Pid,
            fallback_threshold: Some(0.5),
            ..Config::default()
        };
        let mut controller = ThresholdController::new(&config);
        assert_eq!(controller.state_name(), "cold_start");

        assert_eq!(controller.check(0.8), CheckResult::Emit);
        assert_eq!(controller.check(0.3), CheckResult::Reject);

        controller.tick();
        assert_eq!(controller.state_name(), "pid");
    }

    #[test]
    fn switch_to_fixed_and_back() {
        let config = Config {
            algorithm: Algorithm::Pid,
            fallback_threshold: Some(0.5),
            ..Config::default()
        };
        let mut controller = ThresholdController::new(&config);
        controller.check(0.8);
        controller.tick();
        assert_eq!(controller.state_name(), "pid");

        let mut fixed_config = config.clone();
        fixed_config.algorithm = Algorithm::Fixed;
        fixed_config.fallback_threshold = Some(0.6);
        controller.update_config(&fixed_config);
        assert_eq!(controller.state_name(), "fixed");
        assert!((controller.threshold() - 0.6).abs() < f64::EPSILON);

        let mut pid_config = fixed_config.clone();
        pid_config.algorithm = Algorithm::Pid;
        controller.update_config(&pid_config);
        assert_eq!(controller.state_name(), "pid");
    }

    #[test]
    fn buffered_mode_returns_buffer() {
        let config = Config {
            algorithm: Algorithm::BufferedBatch,
            fallback_threshold: Some(0.5),
            ..Config::default()
        };
        let mut controller = ThresholdController::new(&config);
        assert_eq!(controller.check(0.1), CheckResult::Buffer);
        assert_eq!(controller.check(0.9), CheckResult::Buffer);

        let mut streaming_config = config.clone();
        streaming_config.algorithm = Algorithm::BufferedStreaming;
        controller.update_config(&streaming_config);
        assert_eq!(controller.check(0.5), CheckResult::Buffer);
    }
}
