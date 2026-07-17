// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2024 Allan Bazinet <w6baz@arrl.net>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 submode parameter queries to Rust.

use core::fmt;
use std::fmt::Display;

use crate::{
    internal::{commons, costas},
    protocol::Submode,
};

/// Error returned on invalid submode usage (C++: `JS8::Submode::error`).
#[derive(Debug, Clone)]
pub struct Error {
    what: String,
}

impl Error {
    #[inline]
    pub fn new<S: Into<String>>(what: S) -> Self {
        Self { what: what.into() }
    }

    #[inline]
    pub fn invalid_submode(submode: i32) -> Self {
        Self::new(format!("Invalid JS8 submode {submode}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

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

    pub const fn costas_type(&self) -> costas::Type {
        self.costas
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

impl TryFrom<i32> for Submode {
    type Error = Error;

    fn try_from(value: i32) -> Result<Self> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Fast),
            2 => Ok(Self::Turbo),
            4 => Ok(Self::Slow),
            8 => Ok(Self::Ultra),
            _ => Err(Error::invalid_submode(value)),
        }
    }
}

#[inline]
pub const fn data(submode: Submode) -> &'static Data {
    match submode {
        Submode::Normal => &NORMAL,
        Submode::Fast => &FAST,
        Submode::Turbo => &TURBO,
        Submode::Slow => &SLOW,
        Submode::Ultra => &ULTRA,
    }
}

/// Name of the submode, in all uppercase letters.
pub const fn name(submode: Submode) -> &'static str {
    data(submode).name
}

pub const fn bandwidth(submode: Submode) -> u64 {
    data(submode).bandwidth
}

/// Period from one transmission start to the next, in seconds.
pub const fn period_seconds(submode: Submode) -> u64 {
    data(submode).period_s
}

/// Audio samples (at 12000 samples/sec) per symbol.
pub const fn samples_for_one_symbol(submode: Submode) -> u64 {
    data(submode).samples_for_one_symbol
}

/// Samples used to transmit the symbols of one period.
pub const fn samples_for_symbols(submode: Submode) -> u64 {
    data(submode).samples_for_symbols
}

/// Samples needed to capture entire TX duration, including start delay, plus another 500 ms.
pub const fn samples_needed(submode: Submode) -> u64 {
    data(submode).samples_needed
}

/// Samples per period at 12000 samples/sec.
pub const fn samples_per_period(submode: Submode) -> u64 {
    data(submode).samples_per_period
}

pub const fn rx_snr_threshold(submode: Submode) -> i32 {
    data(submode).rx_snr_threshold
}

pub const fn rx_threshold(submode: Submode) -> i32 {
    data(submode).rx_threshold
}

/// Start delay after tx start before sending data, in milliseconds.
pub const fn start_delay_ms(submode: Submode) -> u64 {
    data(submode).start_delay_ms
}

pub const fn tone_spacing(submode: Submode) -> f64 {
    data(submode).tone_spacing
}

/// Total TX duration (seconds): `data_duration + start_delay_ms/1000`.
pub const fn tx_duration(submode: Submode) -> f64 {
    data(submode).tx_duration
}

pub const fn late_threshold_multiplier(submode: Submode) -> f64 {
    match submode as i32 {
        8 | 2 => 0.5,
        1 => 0.75,
        0 | 4 => 1.0,
        _ => unreachable!(),
    }
}

/// Compute which cycle we are currently in based on submode frames per cycle and current `k` position.
pub const fn compute_cycle_for_decode(submode: Submode, k: usize) -> usize {
    let max_frames = commons::JS8_RX_SAMPLE_SIZE;
    let cycle_frames = samples_per_period(submode) as usize;

    (k / cycle_frames) % (max_frames / cycle_frames)
}

/// Compute an alternate cycle offset by a specific number of frames.
pub const fn compute_alt_cycle_for_decode(
    submode: Submode,
    k: usize,
    offset_frames: usize,
) -> usize {
    let max_frames = commons::JS8_RX_SAMPLE_SIZE;
    let alt_k = if k >= offset_frames {
        k - offset_frames
    } else {
        max_frames - ((offset_frames - k) % max_frames)
    };

    compute_cycle_for_decode(submode, alt_k % max_frames)
}

pub const fn compute_ratio(submode: Submode, period_s: f64) -> f64 {
    let d = data(submode);
    (period_s - d.data_duration) / period_s
}

#[cfg(test)]
mod tests {
    use super::{
        FAST, NORMAL, SLOW, Submode, TURBO, ULTRA, compute_alt_cycle_for_decode,
        compute_cycle_for_decode, floor_f64,
    };
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
        assert_eq!(compute_cycle_for_decode(Submode::Fast, k), 3);

        let offset: usize = 5_000;
        let alt = compute_alt_cycle_for_decode(Submode::Fast, offset - 100, offset);
        let expected = compute_cycle_for_decode(Submode::Fast, max_frames - 100);
        assert_eq!(alt, expected);
    }
}
