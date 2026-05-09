use metrics::{counter, describe_counter, describe_gauge, gauge};

pub fn register() {
    describe_counter!(
        "flowgate_messages_received_total",
        "Total inbound messages processed"
    );
    describe_counter!(
        "flowgate_messages_emitted_total",
        "Messages that passed the threshold"
    );
    describe_counter!(
        "flowgate_messages_rejected_total",
        "Messages below the threshold"
    );
    describe_gauge!("flowgate_current_threshold", "Current threshold value");
    describe_gauge!(
        "flowgate_target_rate",
        "Configured target emission rate (per second)"
    );
    describe_gauge!(
        "flowgate_actual_rate",
        "Measured emission rate (per second)"
    );
    describe_gauge!("flowgate_pid_error", "Current PID error signal");
    describe_gauge!("flowgate_pid_p_term", "PID proportional term");
    describe_gauge!("flowgate_pid_i_term", "PID integral term");
    describe_gauge!("flowgate_pid_d_term", "PID derivative term");
    describe_gauge!(
        "flowgate_controller_state",
        "Controller state: 0=cold_start, 1=warmup, 2=pid, 3=fixed"
    );
    describe_gauge!(
        "flowgate_warmup_samples_collected",
        "Samples collected during warmup phase"
    );
    describe_gauge!("flowgate_buffer_size", "Messages currently in the buffer");
    describe_counter!(
        "flowgate_buffer_evicted_total",
        "Messages evicted from buffer (expired)"
    );
    describe_gauge!(
        "flowgate_avg_emitted_score",
        "Exponential moving average of emitted message scores"
    );
    describe_gauge!(
        "flowgate_avg_latency_ms",
        "EMA of time from message arrival to emission"
    );
    describe_gauge!(
        "flowgate_ingestion_rate",
        "Measured ingestion rate (messages per second)"
    );
}

pub fn record_received() {
    counter!("flowgate_messages_received_total").increment(1);
}

pub fn record_emitted() {
    counter!("flowgate_messages_emitted_total").increment(1);
}

pub fn record_rejected() {
    counter!("flowgate_messages_rejected_total").increment(1);
}

pub fn set_threshold(v: f64) {
    gauge!("flowgate_current_threshold").set(v);
}

pub fn set_target_rate(v: f64) {
    gauge!("flowgate_target_rate").set(v);
}

pub fn set_actual_rate(v: f64) {
    gauge!("flowgate_actual_rate").set(v);
}

pub fn set_pid_error(v: f64) {
    gauge!("flowgate_pid_error").set(v);
}

pub fn set_pid_terms(p: f64, i: f64, d: f64) {
    gauge!("flowgate_pid_p_term").set(p);
    gauge!("flowgate_pid_i_term").set(i);
    gauge!("flowgate_pid_d_term").set(d);
}

#[derive(Debug, Clone, Copy)]
pub enum ControllerState {
    ColdStart = 0,
    Warmup = 1,
    Pid = 2,
    Fixed = 3,
}

pub fn set_controller_state(state: ControllerState) {
    gauge!("flowgate_controller_state").set(state as u64 as f64);
}

pub fn set_warmup_samples(n: u64) {
    gauge!("flowgate_warmup_samples_collected").set(n as f64);
}

pub fn set_buffer_size(n: usize) {
    gauge!("flowgate_buffer_size").set(n as f64);
}

pub fn set_ingestion_rate(v: f64) {
    gauge!("flowgate_ingestion_rate").set(v);
}

pub fn record_evicted(n: usize) {
    counter!("flowgate_buffer_evicted_total").increment(n as u64);
}

pub fn set_avg_emitted_score(v: f64) {
    gauge!("flowgate_avg_emitted_score").set(v);
}

pub fn set_avg_latency_ms(v: f64) {
    gauge!("flowgate_avg_latency_ms").set(v);
}
