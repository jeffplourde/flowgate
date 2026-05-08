# Analytical Model & Simulation Guide

This document describes the mathematical framework behind Flowgate's PID-controlled adaptive threshold, how to predict its steady-state behavior, analyze its stability, and simulate its response to various conditions.

## The Plant Model

Flowgate's "plant" is the relationship between the threshold θ and the emission rate. If incoming scores are drawn from a distribution with CDF F(s) and PDF f(s), and the input message rate is λ messages/sec, the emission rate at threshold θ is:

```
r(θ) = λ · (1 - F(θ))
```

This is the fraction of messages whose score exceeds the threshold, scaled by the input rate.

## Steady-State Equilibrium

At steady state the PID error is zero, meaning `actual_rate = target_rate`. Solving for the equilibrium threshold:

```
target_rate = λ · (1 - F(θ*))

θ* = F⁻¹(1 - target_rate / λ)
```

This is the threshold the PID controller will converge to. You can compute it analytically for any known distribution — for example, the demo's Beta(2,5) has a closed-form inverse CDF via the regularized incomplete beta function.

**Example:** With Beta(2,5) scores at λ = 500 msg/s targeting 10 emissions/s:

```
1 - F(θ*) = 10/500 = 0.02
θ* = F⁻¹(0.98) ≈ 0.72
```

## Plant Gain — The Key Nonlinearity

The sensitivity of emission rate to threshold changes is:

```
∂r/∂θ = -λ · f(θ)
```

This **plant gain** is the central quantity for understanding system behavior. It varies depending on where the threshold sits in the score distribution:

- **Near a mode** (where the PDF is tall): small threshold changes cause large rate swings — the system is highly sensitive and prone to oscillation if gains are too high.
- **In the tails** (where the PDF is low): large threshold changes are needed for any effect — the system may feel sluggish.
- **During bursts**: the plant gain scales linearly with λ — a 5x burst means 5x the effective loop gain.

This nonlinearity means that PID gains tuned for one operating point can oscillate or go sluggish at another.

## Linearized Stability Analysis

Around the equilibrium θ*, define small perturbations δθ = θ - θ* and δr = r - target_rate:

```
δr ≈ -λ · f(θ*) · δθ
```

The measurement window (duration T seconds) acts as a low-pass filter on the rate signal. In the Laplace domain, this is approximately:

```
G_window(s) = 1 / (Ts + 1)
```

The PID controller transfer function is:

```
C(s) = Kp + Ki/s + Kd·s
```

The open-loop transfer function for the linearized system is:

```
L(s) = λ · f(θ*) · (Kp + Ki/s + Kd·s) / (Ts + 1)
```

Standard Bode or Nyquist analysis can be applied to L(s) to determine:

- **Gain margin**: how much the loop gain can increase before instability (relevant during bursts)
- **Phase margin**: how close to oscillation the system is
- **Bandwidth**: how fast the system can track changes in target rate or input rate

The critical parameter is `λ · f(θ*)` — the product of input rate and PDF density at the operating point. This determines the effective loop gain.

## Tuning Implications

### Operating Region Effects

| Region | PDF density | Effective gain | Behavior | Recommendation |
|--------|------------|----------------|----------|----------------|
| Near mode | High | High | Sensitive, risk of oscillation | Reduce Kp, increase Kd for damping |
| In tails | Low | Low | Sluggish, slow convergence | Increase Kp, or accept slower response |
| During burst | — | Scales with λ | Amplified sensitivity | Ensure gains are stable at peak λ |

### Measurement Window Tradeoff

The window duration T controls noise vs responsiveness:

- **Longer T**: smoother rate estimate, but delays the controller's reaction. The PID's D term partially compensates but there's a fundamental lag.
- **Shorter T**: faster reaction, but noisy rate estimates cause threshold jitter, which increases variance in the output rate.

A useful rule of thumb: T should be at least 3-5x the PID tick interval to get a meaningful rate estimate.

### Anti-Windup Relevance

During cold start or after a long quiet period, the integral term can accumulate large error. When messages resume, this causes a large threshold overshoot before the integral unwinds. The `anti_windup_limit` parameter clamps the integral to prevent this — it should be set relative to the expected steady-state integral value, which is typically small (the integral only needs to correct for steady-state offset, which the P term can't eliminate alone).

## Burst Response Dynamics

When the input rate suddenly increases from λ to B·λ (burst multiplier B):

1. **Immediate effect**: emission rate jumps by factor B (same threshold, more messages passing)
2. **Error goes negative**: actual_rate > target_rate
3. **PID raises threshold**: proportional term reacts immediately, derivative term detects the rate of change
4. **New equilibrium during burst**: θ_burst = F⁻¹(1 - target_rate/(B·λ)) — higher than steady state
5. **Burst ends**: emission rate drops, error goes positive, PID lowers threshold back
6. **Transient**: overshoot/undershoot depending on gains and integral accumulation

The size of the transient overshoot depends on:
- How fast the PID reacts (Kp magnitude, D term damping)
- How much integral error accumulated during the burst
- The burst duration relative to the measurement window

## Discrete-Time Simulation

The system can be simulated without NATS using a simple discrete-time model:

```python
import numpy as np
from scipy import stats

# Parameters
target_rate = 10.0       # emissions per second
lambda_rate = 500.0      # input messages per second
dt = 1.0                 # PID tick interval (seconds)
window = 10.0            # measurement window (seconds)
kp, ki, kd = 0.01, 0.001, 0.0005
min_thresh, max_thresh = 0.0, 1.0
anti_windup = 100.0

# Score distribution
dist = stats.beta(2, 5)

# State
theta = 0.5              # initial threshold
integral = 0.0
prev_error = 0.0
emission_times = []

# Simulation
duration = 300           # seconds
messages_per_tick = int(lambda_rate * dt)

history = {'t': [], 'theta': [], 'actual_rate': [], 'error': []}

for step in range(int(duration / dt)):
    t = step * dt

    # Generate scores for this tick
    scores = dist.rvs(size=messages_per_tick)
    emissions = np.sum(scores >= theta)
    emission_times.extend([t] * emissions)

    # Prune old emissions outside window
    cutoff = t - window
    emission_times = [et for et in emission_times if et > cutoff]

    # Compute actual rate
    actual_rate = len(emission_times) / window

    # PID update
    error = target_rate - actual_rate
    integral = np.clip(integral + error * dt, -anti_windup, anti_windup)
    derivative = (error - prev_error) / dt if step > 0 else 0.0
    prev_error = error

    adjustment = kp * error + ki * integral + kd * derivative
    theta = np.clip(theta - adjustment, min_thresh, max_thresh)

    history['t'].append(t)
    history['theta'].append(theta)
    history['actual_rate'].append(actual_rate)
    history['error'].append(error)
```

### Useful Simulation Experiments

1. **Step response**: change `target_rate` mid-simulation, observe convergence time and overshoot
2. **Burst response**: multiply `lambda_rate` by 5 for a short interval, observe threshold adaptation and recovery
3. **Distribution shift**: switch from Beta(2,5) to Normal(0.5, 0.15) mid-run, observe how the PID tracks the new equilibrium
4. **Gain sweep**: run the simulation across a range of Kp/Ki/Kd values, plot overshoot vs settling time
5. **Window size sweep**: vary the measurement window, observe the noise/responsiveness tradeoff

## Analytical Predictions vs Reality

The analytical model assumes:

- **Stationary distribution**: scores are i.i.d. from a fixed distribution. In practice, ML model outputs may drift.
- **Constant input rate between ticks**: real traffic is bursty at sub-second timescales.
- **Instantaneous threshold application**: in practice there's a small delay between PID update and the next message being checked.
- **Perfect rate measurement**: the sliding window is a finite-sample estimate with variance proportional to 1/N.

These gaps are generally small for medium-throughput systems (100-10K msg/s) but become significant at low throughput where the rate estimate is noisy, or at very high throughput where per-message processing latency matters.
