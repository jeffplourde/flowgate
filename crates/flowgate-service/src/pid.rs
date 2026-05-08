#[derive(Debug, Clone)]
pub struct PidParams {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    pub min_output: f64,
    pub max_output: f64,
    pub anti_windup_limit: f64,
}

#[derive(Debug)]
pub struct PidController {
    params: PidParams,
    integral: f64,
    prev_error: f64,
    output: f64,
    initialized: bool,
}

impl PidController {
    pub fn new(params: PidParams, initial_output: f64) -> Self {
        let output = initial_output.clamp(params.min_output, params.max_output);
        Self {
            params,
            integral: 0.0,
            prev_error: 0.0,
            output,
            initialized: false,
        }
    }

    /// Update the PID controller with a new error measurement.
    ///
    /// `error` = target_rate - actual_rate (positive means "emitting too few").
    /// `dt` = time since last update in seconds.
    ///
    /// Returns the new threshold. Because higher threshold = fewer emissions,
    /// we *subtract* the PID adjustment from the current threshold.
    pub fn update(&mut self, error: f64, dt: f64) -> f64 {
        if dt <= 0.0 {
            return self.output;
        }

        if !self.initialized {
            self.prev_error = error;
            self.initialized = true;
        }

        self.integral += error * dt;
        self.integral = self.integral.clamp(
            -self.params.anti_windup_limit,
            self.params.anti_windup_limit,
        );

        let derivative = (error - self.prev_error) / dt;
        self.prev_error = error;

        let adjustment =
            self.params.kp * error + self.params.ki * self.integral + self.params.kd * derivative;

        // Positive error = emitting too few = lower the threshold
        self.output =
            (self.output - adjustment).clamp(self.params.min_output, self.params.max_output);

        self.output
    }

    #[allow(dead_code)]
    pub fn output(&self) -> f64 {
        self.output
    }

    pub fn set_output(&mut self, value: f64) {
        self.output = value.clamp(self.params.min_output, self.params.max_output);
    }

    pub fn update_params(&mut self, params: PidParams) {
        self.integral = self
            .integral
            .clamp(-params.anti_windup_limit, params.anti_windup_limit);
        self.output = self.output.clamp(params.min_output, params.max_output);
        self.params = params;
    }

    pub fn reset(&mut self) {
        self.integral = 0.0;
        self.prev_error = 0.0;
        self.initialized = false;
    }

    pub fn integral(&self) -> f64 {
        self.integral
    }

    pub fn prev_error(&self) -> f64 {
        self.prev_error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_params() -> PidParams {
        PidParams {
            kp: 0.01,
            ki: 0.001,
            kd: 0.0005,
            min_output: 0.0,
            max_output: 1.0,
            anti_windup_limit: 100.0,
        }
    }

    #[test]
    fn steady_state_no_change() {
        let mut pid = PidController::new(default_params(), 0.5);
        // Zero error should produce no change (after initial derivative settles)
        let _ = pid.update(0.0, 1.0);
        let t = pid.update(0.0, 1.0);
        assert!(
            (t - 0.5).abs() < 0.001,
            "threshold should stay near 0.5, got {t}"
        );
    }

    #[test]
    fn emitting_too_few_lowers_threshold() {
        let mut pid = PidController::new(default_params(), 0.8);
        // Positive error: target > actual, need more emissions, threshold should decrease
        for _ in 0..50 {
            pid.update(10.0, 1.0);
        }
        assert!(
            pid.output() < 0.8,
            "threshold should decrease when emitting too few"
        );
    }

    #[test]
    fn emitting_too_many_raises_threshold() {
        let mut pid = PidController::new(default_params(), 0.2);
        // Negative error: actual > target, emitting too many, threshold should increase
        for _ in 0..50 {
            pid.update(-10.0, 1.0);
        }
        assert!(
            pid.output() > 0.2,
            "threshold should increase when emitting too many"
        );
    }

    #[test]
    fn output_clamped_to_bounds() {
        let mut pid = PidController::new(default_params(), 0.5);
        // Drive hard in one direction
        for _ in 0..1000 {
            pid.update(1000.0, 1.0);
        }
        assert!(pid.output() >= 0.0);
        assert!(pid.output() <= 1.0);
    }

    #[test]
    fn anti_windup() {
        let params = PidParams {
            anti_windup_limit: 10.0,
            ..default_params()
        };
        let mut pid = PidController::new(params, 0.5);
        for _ in 0..1000 {
            pid.update(100.0, 1.0);
        }
        assert!(pid.integral().abs() <= 10.0, "integral should be clamped");
    }

    #[test]
    fn zero_dt_is_noop() {
        let mut pid = PidController::new(default_params(), 0.5);
        let t = pid.update(10.0, 0.0);
        assert!((t - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn params_update_clamps_state() {
        let mut pid = PidController::new(default_params(), 0.9);
        let new_params = PidParams {
            max_output: 0.7,
            ..default_params()
        };
        pid.update_params(new_params);
        assert!(pid.output() <= 0.7);
    }
}
