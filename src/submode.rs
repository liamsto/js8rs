// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2024 Allan Bazinet <w6baz@arrl.net>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 submode parameter queries to Rust.

use std::fmt::Display;

use crate::{
    internal::{commons, costas},
    protocol::Submode,
};

#[inline]
const fn floor_f64(v: f64) -> i32 {
    let i = v as i32;
    if v < (i as f64) { i - 1 } else { i }
}

#[derive(Clone, Copy)]
pub struct Data {
    pub name: &'static str,
    pub samples_for_one_symbol: u64,
    pub start_delay_ms: u64,
    pub period_s: u64,
    pub costas: costas::Type,
    pub rx_snr_threshold: i32,
    pub rx_threshold: i32,
    pub samples_for_symbols: u64,
    pub bandwidth: u64,
    pub samples_per_period: u64,
    pub tone_spacing: f64,
    pub samples_needed: u64,
    pub data_duration: f64,
    pub tx_duration: f64,
}

impl Data {
    const fn new(
        name: &'static str,
        samples_for_one_symbol: u64,
        start_delay_ms: u64,
        period_s: u64,
        costas: costas::Type,
        rx_snr_threshold: i32,
        rx_threshold: i32,
    ) -> Self {
        let js8_num_symbols = commons::JS8_NUM_SYMBOLS;
        let rx_rate = commons::JS8_RX_SAMPLE_RATE;

        let samples_for_symbols = js8_num_symbols * samples_for_one_symbol;
        let bandwidth = (8 * rx_rate) / samples_for_one_symbol;
        let samples_per_period = rx_rate * period_s;
        let tone_spacing = (rx_rate as f64) / (samples_for_one_symbol as f64);

        let samples_needed_f = (samples_for_symbols as f64)
            + (0.5 + (start_delay_ms as f64) / 1000.0) * (rx_rate as f64);
        let samples_needed = floor_f64(samples_needed_f) as u64;

        let data_duration = (samples_for_symbols as f64) / (rx_rate as f64);
        let tx_duration = data_duration + (start_delay_ms as f64) / 1000.0;

        Self {
            name,
            samples_for_one_symbol,
            start_delay_ms,
            period_s,
            costas,
            rx_snr_threshold,
            rx_threshold,
            samples_for_symbols,
            bandwidth,
            samples_per_period,
            tone_spacing,
            samples_needed,
            data_duration,
            tx_duration,
        }
    }

    const fn new_default_rx_threshold(
        name: &'static str,
        samples_for_one_symbol: u64,
        start_delay_ms: u64,
        period_s: u64,
        costas: costas::Type,
        rx_snr_threshold: i32,
    ) -> Self {
        Self::new(
            name,
            samples_for_one_symbol,
            start_delay_ms,
            period_s,
            costas,
            rx_snr_threshold,
            10,
        )
    }
}

pub const NORMAL: Data = Data::new_default_rx_threshold(
    "NORMAL",
    commons::JS8A_SYMBOL_SAMPLES,
    commons::JS8A_START_DELAY_MS,
    commons::JS8A_TX_SECONDS,
    costas::Type::Original,
    -24,
);

pub const FAST: Data = Data::new(
    "FAST",
    commons::JS8B_SYMBOL_SAMPLES,
    commons::JS8B_START_DELAY_MS,
    commons::JS8B_TX_SECONDS,
    costas::Type::Modified,
    -22,
    16,
);

pub const TURBO: Data = Data::new(
    "TURBO",
    commons::JS8C_SYMBOL_SAMPLES,
    commons::JS8C_START_DELAY_MS,
    commons::JS8C_TX_SECONDS,
    costas::Type::Modified,
    -20,
    32,
);

pub const SLOW: Data = Data::new_default_rx_threshold(
    "SLOW",
    commons::JS8E_SYMBOL_SAMPLES,
    commons::JS8E_START_DELAY_MS,
    commons::JS8E_TX_SECONDS,
    costas::Type::Modified,
    -28,
);

pub const ULTRA: Data = Data::new(
    "ULTRA",
    commons::JS8I_SYMBOL_SAMPLES,
    commons::JS8I_START_DELAY_MS,
    commons::JS8I_TX_SECONDS,
    costas::Type::Modified,
    -18,
    50,
);

impl Display for Submode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Normal => "Normal",
            Self::Fast => "Fast",
            Self::Turbo => "Turbo",
            Self::Slow => "Slow",
            Self::Ultra => "Ultra",
        };
        f.write_str(s)
    }
}

impl Submode {
    /// Returns the uppercase submode name.
    pub const fn name(self) -> &'static str {
        data(self).name
    }

    /// Returns the occupied bandwidth in hertz.
    pub const fn bandwidth_hz(self) -> u64 {
        data(self).bandwidth
    }

    /// Returns the slot period in seconds.
    pub const fn period_seconds(self) -> u64 {
        data(self).period_s
    }

    /// Returns 12 kHz samples per transmitted symbol.
    pub const fn samples_for_one_symbol(self) -> u64 {
        data(self).samples_for_one_symbol
    }

    /// Returns samples used to transmit all symbols in a frame.
    pub const fn samples_for_symbols(self) -> u64 {
        data(self).samples_for_symbols
    }

    /// Returns samples needed for a complete receive capture.
    pub const fn samples_needed(self) -> u64 {
        data(self).samples_needed
    }

    /// Returns decoder samples in one submode period.
    pub const fn samples_per_period(self) -> usize {
        data(self).samples_per_period as usize
    }

    /// Returns the nominal receive SNR threshold in decibels.
    pub const fn rx_snr_threshold(self) -> i32 {
        data(self).rx_snr_threshold
    }

    /// Returns the decoder synchronization threshold.
    pub const fn rx_threshold(self) -> i32 {
        data(self).rx_threshold
    }

    /// Returns the nominal slot start delay in milliseconds.
    pub const fn start_delay_ms(self) -> u64 {
        data(self).start_delay_ms
    }

    /// Returns tone spacing in hertz.
    pub const fn tone_spacing(self) -> f64 {
        data(self).tone_spacing
    }

    /// Returns the complete waveform duration in seconds.
    pub const fn tx_duration(self) -> f64 {
        data(self).tx_duration
    }

    /// Returns the late-start threshold scale.
    pub const fn late_threshold_multiplier(self) -> f64 {
        match self {
            Self::Ultra | Self::Turbo => 0.5,
            Self::Fast => 0.75,
            Self::Normal | Self::Slow => 1.0,
        }
    }

    /// Returns the transmit-to-slot duration ratio.
    pub const fn compute_ratio(self) -> f64 {
        let data = data(self);
        let period = data.period_s as f64;
        (period - data.data_duration) / period
    }

    /// Returns the receive cycle containing sample position `k`.
    pub const fn compute_cycle_for_decode(self, k: usize) -> usize {
        let max_frames = commons::JS8_RX_SAMPLE_SIZE;
        let cycle_frames = self.samples_per_period();

        (k / cycle_frames) % (max_frames / cycle_frames)
    }

    /// Returns the receive cycle after applying an alternate sample offset.
    pub const fn compute_alt_cycle_for_decode(self, k: usize, offset_frames: usize) -> usize {
        let max_frames = commons::JS8_RX_SAMPLE_SIZE;
        let alt_k = if k >= offset_frames {
            k - offset_frames
        } else {
            max_frames - ((offset_frames - k) % max_frames)
        };

        self.compute_cycle_for_decode(alt_k % max_frames)
    }

    pub(crate) const fn costas_type(self) -> costas::Type {
        data(self).costas
    }
}

const fn data(submode: Submode) -> &'static Data {
    match submode {
        Submode::Normal => &NORMAL,
        Submode::Fast => &FAST,
        Submode::Turbo => &TURBO,
        Submode::Slow => &SLOW,
        Submode::Ultra => &ULTRA,
    }
}

#[cfg(test)]
mod tests {
    use super::{FAST, NORMAL, SLOW, Submode, TURBO, ULTRA, floor_f64};
    use crate::internal::commons::JS8_RX_SAMPLE_SIZE;

    #[test]
    fn floor_matches_cpp_static_asserts() {
        assert_eq!(floor_f64(0.0), 0);
        assert_eq!(floor_f64(0.499_999), 0);
        assert_eq!(floor_f64(0.5), 0);
        assert_eq!(floor_f64(0.999_999), 0);
        assert_eq!(floor_f64(1.0), 1);
        assert_eq!(floor_f64(123.0), 123);
        assert_eq!(floor_f64(123.4), 123);

        assert_eq!(floor_f64(-0.499_999), -1);
        assert_eq!(floor_f64(-0.5), -1);
        assert_eq!(floor_f64(-0.999_999), -1);
        assert_eq!(floor_f64(-1.0), -1);
        assert_eq!(floor_f64(-123.0), -123);
        assert_eq!(floor_f64(-123.4), -124);
    }

    #[test]
    fn submode_constants_match_js8_values() {
        assert_eq!(NORMAL.samples_for_symbols, 151_680);
        assert_eq!(NORMAL.samples_needed, 163_680);
        assert_eq!(NORMAL.bandwidth, 50);
        assert_eq!(NORMAL.period_s, 15);
        assert_eq!(NORMAL.rx_snr_threshold, -24);
        assert_eq!(NORMAL.rx_threshold, 10);

        assert_eq!(FAST.samples_for_symbols, 94_800);
        assert_eq!(FAST.samples_needed, 103_200);
        assert_eq!(FAST.bandwidth, 80);
        assert_eq!(FAST.period_s, 10);
        assert_eq!(FAST.rx_snr_threshold, -22);
        assert_eq!(FAST.rx_threshold, 16);

        assert_eq!(TURBO.samples_for_symbols, 47_400);
        assert_eq!(TURBO.samples_needed, 54_600);
        assert_eq!(TURBO.bandwidth, 160);
        assert_eq!(TURBO.period_s, 6);
        assert_eq!(TURBO.rx_snr_threshold, -20);
        assert_eq!(TURBO.rx_threshold, 32);

        assert_eq!(SLOW.samples_for_symbols, 303_360);
        assert_eq!(SLOW.samples_needed, 315_360);
        assert_eq!(SLOW.bandwidth, 25);
        assert_eq!(SLOW.period_s, 30);
        assert_eq!(SLOW.rx_snr_threshold, -28);
        assert_eq!(SLOW.rx_threshold, 10);

        assert_eq!(ULTRA.samples_for_symbols, 30_336);
        assert_eq!(ULTRA.samples_needed, 37_536);
        assert_eq!(ULTRA.bandwidth, 250);
        assert_eq!(ULTRA.period_s, 4);
        assert_eq!(ULTRA.rx_snr_threshold, -18);
        assert_eq!(ULTRA.rx_threshold, 50);
    }

    #[test]
    fn cycle_math_wraps_and_alt_offsets() {
        let period = FAST.samples_per_period as usize;
        let max_frames = JS8_RX_SAMPLE_SIZE;
        let cycles = max_frames / period;
        let k = period * (cycles + 3);
        assert_eq!(Submode::Fast.compute_cycle_for_decode(k), 3);

        let offset: usize = 5_000;
        let alt = Submode::Fast.compute_alt_cycle_for_decode(offset - 100, offset);
        let expected = Submode::Fast.compute_cycle_for_decode(max_frames - 100);
        assert_eq!(alt, expected);
    }
}
