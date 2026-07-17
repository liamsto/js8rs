#![allow(dead_code)]

use num_complex::Complex32;

/// Lightweight Kalman tracker for residual frequency offset.
#[derive(Clone, Debug)]
pub struct FrequencyTracker {
    enabled: bool,
    est_hz: f64,
    fs_hz: f64,
    alpha: f64,
    max_step_hz: f64,
    max_error_hz: f64,
    sum_abs: f64,
    updates: u32,
}

impl Default for FrequencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl FrequencyTracker {
    #[inline]
    pub const fn new() -> Self {
        Self {
            enabled: true,
            est_hz: 0.0,
            fs_hz: 0.0,
            alpha: 0.15,
            max_step_hz: 0.3,
            max_error_hz: 5.0,
            sum_abs: 0.0,
            updates: 0,
        }
    }

    #[inline]
    pub const fn reset(
        &mut self,
        initial_hz: f64,
        sample_rate_hz: f64,
        alpha: f64,
        max_step_hz: f64,
        max_error_hz: f64,
    ) {
        self.enabled = true;
        self.est_hz = initial_hz;
        self.fs_hz = sample_rate_hz;
        self.alpha = alpha;
        self.max_step_hz = max_step_hz;
        self.max_error_hz = max_error_hz;
        self.sum_abs = 0.0;
        self.updates = 0;
    }

    #[inline]
    pub const fn reset_default(&mut self, initial_hz: f64, sample_rate_hz: f64) {
        self.reset(initial_hz, sample_rate_hz, 0.15, 0.3, 5.0);
    }

    #[inline]
    pub const fn disable(&mut self) {
        self.enabled = false;
    }

    #[inline]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    pub const fn current_hz(&self) -> f64 {
        self.est_hz
    }

    #[inline]
    pub fn average_step_hz(&self) -> f64 {
        if self.updates > 0 {
            self.sum_abs / f64::from(self.updates)
        } else {
            0.0
        }
    }

    /// Rotate samples by the tracked residual frequency.
    ///
    /// Equivalent to C++ `apply(std::complex`<float>* data, int count).
    #[inline]
    pub fn apply(&self, data: &mut [Complex32]) {
        if !self.enabled || data.is_empty() || self.fs_hz <= 0.0 {
            return;
        }

        let dphi = (2.0 * core::f64::consts::PI) * (self.est_hz / self.fs_hz);
        let wstep = Complex32::from_polar(1.0, dphi as f32);
        let mut w = Complex32::new(1.0, 0.0);

        for x in data.iter_mut() {
            w *= wstep;
            *x *= w;
        }
    }

    /// Nudge estimate using pilot residuals.
    #[inline]
    pub const fn update(&mut self, mut residual_hz: f64, weight: f64) {
        if !self.enabled || self.fs_hz <= 0.0 {
            return;
        }
        if !residual_hz.is_finite() || !weight.is_finite() || weight <= 0.0 {
            return;
        }
        if residual_hz.abs() > self.max_error_hz {
            return;
        }

        residual_hz *= weight.min(1.0);

        let step = clamp(residual_hz, -self.max_step_hz, self.max_step_hz);
        self.est_hz = self.alpha.mul_add(step, self.est_hz);

        self.sum_abs += step.abs();
        self.updates += 1;
    }
}

/// Tracks residual timing (sample) offset between the symbol clock and the signal.
#[derive(Clone, Debug)]
pub struct TimingTracker {
    enabled: bool,
    est_samples: f64,
    alpha: f64,
    max_step: f64,
    max_total: f64,
    sum_abs: f64,
    updates: u32,
}

impl Default for TimingTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TimingTracker {
    #[inline]
    pub const fn new() -> Self {
        Self {
            enabled: true,
            est_samples: 0.0,
            alpha: 0.15,
            max_step: 0.35,
            max_total: 2.0,
            sum_abs: 0.0,
            updates: 0,
        }
    }

    #[inline]
    pub const fn reset(
        &mut self,
        initial_samples: f64,
        alpha: f64,
        max_step: f64,
        max_total_error: f64,
    ) {
        self.enabled = true;
        self.est_samples = initial_samples;
        self.alpha = alpha;
        self.max_step = max_step;
        self.max_total = max_total_error;
        self.sum_abs = 0.0;
        self.updates = 0;
    }

    #[inline]
    pub const fn disable(&mut self) {
        self.enabled = false;
    }

    #[inline]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    pub const fn current_samples(&self) -> f64 {
        self.est_samples
    }

    #[inline]
    pub fn average_step_samples(&self) -> f64 {
        if self.updates > 0 {
            self.sum_abs / f64::from(self.updates)
        } else {
            0.0
        }
    }

    #[inline]
    pub const fn update(&mut self, mut residual_samples: f64, weight: f64) {
        if !self.enabled {
            return;
        }
        if !residual_samples.is_finite() || !weight.is_finite() || weight <= 0.0 {
            return;
        }

        residual_samples *= weight.min(1.0);

        let step = clamp(residual_samples, -self.max_step, self.max_step);
        let next = self.alpha.mul_add(step, self.est_samples);

        if next.abs() > self.max_total {
            return;
        }

        self.est_samples = next;
        self.sum_abs += step.abs();
        self.updates += 1;
    }
}

#[inline]
const fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}
