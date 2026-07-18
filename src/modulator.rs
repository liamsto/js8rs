// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported PCM modulation to Rust and added allocation free typed rendering.

use crate::codec::EncodedFrame;
use crate::encoder::TONES_PER_FRAME;
use crate::internal::commons::JS8_NUM_SYMBOLS;
use crate::protocol::{Js8Protocol, Submode, SubmodeLookup};
use crate::timing::unix_time_ms;
use num_complex::Complex64;
use std::f64::consts::TAU;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

/// Operating state.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Rendering initial timing-alignment silence.
    Synchronizing,
    /// Rendering the encoded frame.
    Active,
    /// No transmission is active.
    Idle,
}

/// Output channel selection (modulation itself is mono).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// Dual-mono into L/R.
    Mono,
    /// Signal in left channel, right = 0.
    Left,
    /// Signal in right channel, left = 0.
    Right,
}

const FRAME_RATE_U64: u64 = 48_000;
const FRAME_RATE_F64: f64 = 48_000.0;
const MS_PER_SEC_U64: u64 = 1_000;

/// PCM modulator. Generates interleaved stereo i16 frames (L,R) at 48 kHz.
pub struct Modulator {
    m_state: AtomicU8,
    m_quick_close: bool,
    m_tuning: bool,
    m_audio_frequency: f64,
    m_audio_frequency0: f64,
    m_tone_spacing: f64,
    m_phi: f64,
    m_dphi: f64,
    m_osc: Complex64,
    m_step: Complex64,
    m_amp: f64,
    m_nsps: f64,
    m_symbol_frames: f64,
    m_next_symbol: f64,
    m_silent_frames: u64,
    m_ic: u32,
    m_isym0: u32,
    itone: [u8; TONES_PER_FRAME],

    channel: Channel,
    open: bool,
}

impl Default for Modulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Modulator {
    const ST_SYNCHRONIZING: u8 = 0;
    const ST_ACTIVE: u8 = 1;
    const ST_IDLE: u8 = 2;

    #[must_use]
    /// Creates an idle modulator with no frame loaded.
    pub const fn new() -> Self {
        Self {
            m_state: AtomicU8::new(Self::ST_IDLE),
            m_quick_close: false,
            m_tuning: false,
            m_audio_frequency: 0.0,
            m_audio_frequency0: 0.0,
            m_tone_spacing: 0.0,
            m_phi: 0.0,
            m_dphi: 0.0,
            m_osc: Complex64::new(1.0, 0.0),
            m_step: Complex64::new(1.0, 0.0),
            m_amp: 0.0,
            m_nsps: 0.0,
            m_symbol_frames: 0.0,
            m_next_symbol: 0.0,
            m_silent_frames: 0,
            m_ic: 0,
            m_isym0: u32::MAX,
            itone: [0; TONES_PER_FRAME],
            channel: Channel::Mono,
            open: false,
        }
    }

    #[inline]
    fn state_load(&self) -> State {
        match self.m_state.load(Ordering::Acquire) {
            Self::ST_SYNCHRONIZING => State::Synchronizing,
            Self::ST_ACTIVE => State::Active,
            _ => State::Idle,
        }
    }

    #[inline]
    fn state_store(&self, s: State) {
        let v = match s {
            State::Synchronizing => Self::ST_SYNCHRONIZING,
            State::Active => Self::ST_ACTIVE,
            State::Idle => Self::ST_IDLE,
        };
        self.m_state.store(v, Ordering::Release);
    }

    /// Thread-safe.
    #[inline]
    pub fn is_idle(&self) -> bool {
        self.state_load() == State::Idle
    }

    /// Returns the current modulation state.
    pub fn state(&self) -> State {
        self.state_load()
    }

    /// Returns the configured output channel.
    pub const fn channel(&self) -> Channel {
        self.channel
    }

    /// Sets the base audio frequency (Hz). Not thread-safe w.r.t. rendering.
    pub const fn set_audio_frequency(&mut self, audio_frequency_hz: f64) {
        self.m_audio_frequency = audio_frequency_hz;
    }

    /// Equivalent to C++ `tune(bool)`.
    pub fn tune(&mut self, tuning: bool) {
        self.m_tuning = tuning;
        if !self.m_tuning {
            self.stop(true);
        }
    }

    /// Equivalent to C++ `stop(bool)`.
    pub fn stop(&mut self, quick_close: bool) {
        self.m_quick_close = quick_close;
        self.close();
    }

    /// Equivalent to C++ `close()`, minus the `SoundOutput` interactions.
    pub fn close(&mut self) {
        self.state_store(State::Idle);
        self.open = false;
    }

    /// Starts modulation of an encoded frame using standard JS8 timing data.
    pub fn start(
        &mut self,
        frame: &EncodedFrame,
        now_unix_ms: u64,
        frequency_hz: f64,
        tx_delay: Duration,
        channel: Channel,
    ) {
        self.start_tones_with::<Js8Protocol>(
            &frame.tones,
            frame.submode,
            now_unix_ms,
            frequency_hz,
            tx_delay,
            channel,
        );
    }

    /// Starts fixed tones using standard JS8 timing data.
    pub fn start_tones(
        &mut self,
        tones: &[u8; TONES_PER_FRAME],
        submode: Submode,
        now_unix_ms: u64,
        frequency_hz: f64,
        tx_delay: Duration,
        channel: Channel,
    ) {
        self.start_tones_with::<Js8Protocol>(
            tones,
            submode,
            now_unix_ms,
            frequency_hz,
            tx_delay,
            channel,
        );
    }

    /// Starts fixed tones using a custom submode lookup.
    ///
    /// This is primarily for testing or advanced use cases. Most of the time, you should use
    /// [`Self::start`] or [`Self::start_tones`].
    pub fn start_tones_with<L: SubmodeLookup>(
        &mut self,
        tones: &[u8; TONES_PER_FRAME],
        submode: Submode,
        now_unix_ms: u64,
        frequency_hz: f64,
        tx_delay: Duration,
        channel: Channel,
    ) {
        let current_state = self.state_load();
        if current_state != State::Idle {
            // C++ logs and calls stop().
            self.stop(false);
        }

        self.m_quick_close = false;
        self.m_audio_frequency = frequency_hz;
        self.itone = *tones;

        self.m_nsps = L::samples_for_one_symbol(submode);
        self.m_tone_spacing = L::tone_spacing(submode);
        self.m_symbol_frames = 4.0 * self.m_nsps;
        self.m_next_symbol = 0.0;

        self.m_isym0 = u32::MAX;
        self.m_amp = f64::from(i16::MAX);
        self.m_audio_frequency0 = 0.0;
        self.m_phi = 0.0;
        self.m_osc = Complex64::new(1.0, 0.0);
        self.m_step = Complex64::new(1.0, 0.0);
        self.m_silent_frames = 0;
        self.m_ic = 0;

        self.channel = channel;
        self.open = true;

        if !self.m_tuning {
            // Timing alignment to submode period and nominal start delay.
            let period_ms: u64 = L::period_seconds(submode).saturating_mul(MS_PER_SEC_U64);
            let start_delay_ms: u64 = L::start_delay_ms(submode);

            let period_offset_ms: u64 = if period_ms > 0 {
                now_unix_ms % period_ms
            } else {
                0
            };

            let tx_delay_ms = tx_delay.as_millis().min(u128::from(u64::MAX)) as u64;

            let in_tx_delay_before_period_start =
                period_ms <= period_offset_ms.saturating_add(tx_delay_ms);

            if in_tx_delay_before_period_start {
                let additional_ms_needed_for_tx_delay = period_ms.saturating_sub(period_offset_ms);
                let total_ms = start_delay_ms.saturating_add(additional_ms_needed_for_tx_delay);
                self.m_silent_frames = (total_ms * FRAME_RATE_U64) / MS_PER_SEC_U64;
            } else if start_delay_ms > period_offset_ms {
                let total_ms = start_delay_ms - period_offset_ms;
                self.m_silent_frames = (total_ms * FRAME_RATE_U64) / MS_PER_SEC_U64;
            } else {
                let late_ms = period_offset_ms - start_delay_ms;
                self.m_ic = ((late_ms * FRAME_RATE_U64) / MS_PER_SEC_U64) as u32;
            }
        }

        if self.m_silent_frames > 0 {
            self.state_store(State::Synchronizing);
        } else {
            self.state_store(State::Active);
        }
    }

    /// Starts an encoded frame using the current system time.
    pub fn start_now(
        &mut self,
        frame: &EncodedFrame,
        frequency_hz: f64,
        tx_delay: Duration,
        channel: Channel,
    ) {
        self.start(frame, unix_time_ms(), frequency_hz, tx_delay, channel);
    }

    #[inline]
    fn write_frame(&self, sample: i16, out: &mut [i16], cursor: &mut usize) {
        debug_assert!(*cursor + 1 < out.len());
        match self.channel {
            Channel::Mono => {
                out[*cursor] = sample;
                out[*cursor + 1] = sample;
            }
            Channel::Left => {
                out[*cursor] = sample;
                out[*cursor + 1] = 0;
            }
            Channel::Right => {
                out[*cursor] = 0;
                out[*cursor + 1] = sample;
            }
        }
        *cursor += 2;
    }

    /// Renders interleaved stereo `i16` samples and returns the frame count.
    pub fn render_stereo(&mut self, out: &mut [i16]) -> usize {
        if out.is_empty() {
            return 0;
        }

        debug_assert!(
            self.open,
            "Modulator is not open (start not called or closed)."
        );
        debug_assert!(
            out.len().is_multiple_of(2),
            "No torn frames: out must be multiple of 2 i16."
        );

        let max_frames: u64 = (out.len() / 2) as u64;
        let mut cursor: usize = 0;

        match self.state_load() {
            State::Synchronizing => {
                if self.m_silent_frames > 0 {
                    let mut frames_generated = self.m_silent_frames.min(max_frames);

                    while self.m_silent_frames > 0 && frames_generated > 0 && cursor < out.len() {
                        self.write_frame(0, out, &mut cursor);
                        self.m_silent_frames -= 1;
                        frames_generated -= 1;
                    }
                    if self.m_silent_frames == 0 {
                        self.state_store(State::Active);
                    }
                } else {
                    self.state_store(State::Active);
                }
                self.render_active(out, &mut cursor)
            }
            State::Active => self.render_active(out, &mut cursor),
            State::Idle => 0,
        }
    }

    #[inline]
    fn render_active(&mut self, out: &mut [i16], cursor: &mut usize) -> usize {
        let i0: u32 = if self.m_tuning {
            (9999.0 * self.m_nsps) as u32
        } else {
            (((JS8_NUM_SYMBOLS as f64) - 0.017) * self.m_symbol_frames) as u32
        };

        let i1: u32 = if self.m_tuning {
            (9999.0 * self.m_nsps) as u32
        } else {
            ((JS8_NUM_SYMBOLS as f64) * self.m_symbol_frames) as u32
        };

        let (left_mask, right_mask) = match self.channel {
            Channel::Mono => (-1i16, -1i16),
            Channel::Left => (-1i16, 0i16),
            Channel::Right => (0i16, -1i16),
        };
        let start = *cursor;
        let mut written = 0usize;

        for frame in out[start..].chunks_exact_mut(2) {
            if self.m_ic >= i1 {
                break;
            }

            let symbol_changed = if self.m_isym0 == u32::MAX {
                self.m_isym0 = if self.m_tuning {
                    0
                } else {
                    (f64::from(self.m_ic) / self.m_symbol_frames) as u32
                };
                self.m_next_symbol = f64::from(self.m_isym0 + 1) * self.m_symbol_frames;
                true
            } else if !self.m_tuning && f64::from(self.m_ic) >= self.m_next_symbol {
                self.m_isym0 += 1;
                self.m_next_symbol += self.m_symbol_frames;
                true
            } else {
                false
            };

            if symbol_changed || self.m_audio_frequency != self.m_audio_frequency0 {
                let (phase_sin, phase_cos) = self.m_phi.sin_cos();
                self.m_osc = Complex64::new(phase_cos, phase_sin);
                let tone_index = self.itone[self.m_isym0 as usize];
                let tone_freq =
                    f64::from(tone_index).mul_add(self.m_tone_spacing, self.m_audio_frequency);
                self.m_dphi = TAU * tone_freq / FRAME_RATE_F64;
                let (sin, cos) = self.m_dphi.sin_cos();
                self.m_step = Complex64::new(cos, sin);
                self.m_audio_frequency0 = self.m_audio_frequency;
            }

            self.m_phi += self.m_dphi;
            if self.m_phi > TAU {
                self.m_phi -= TAU;
            }
            self.m_osc *= self.m_step;

            if self.m_ic > i0 {
                self.m_amp *= 0.98;
            }
            if self.m_ic > i1 {
                self.m_amp = 0.0;
            }

            let sample = (self.m_amp * self.m_osc.im).round() as i16;
            frame[0] = sample & left_mask;
            frame[1] = sample & right_mask;
            written += 2;

            self.m_ic = self.m_ic.wrapping_add(1);
        }
        *cursor = start + written;

        if self.m_ic >= i1 {
            self.m_amp = 0.0;
            self.state_store(State::Idle);
        }

        out[*cursor..].fill(0);
        *cursor = out.len();

        out.len() / 2
    }

    /// Renders little-endian interleaved stereo PCM. Allocation free.
    ///
    /// Returns the number of stereo frames written.
    pub fn render_stereo_le(&mut self, out: &mut [u8]) -> usize {
        if out.is_empty() {
            return 0;
        }
        debug_assert!(
            out.len().is_multiple_of(4),
            "No torn frames: out must be multiple of 4 bytes."
        );

        const CHUNK_FRAMES: usize = 512;
        let mut samples = [0i16; CHUNK_FRAMES * 2];
        let mut offset = 0;
        let mut written = 0;

        while offset < out.len() {
            let frames = ((out.len() - offset) / 4).min(CHUNK_FRAMES);
            let sample_count = frames * 2;
            let rendered = self.render_stereo(&mut samples[..sample_count]);
            if rendered == 0 {
                out[offset..].fill(0);
                break;
            }

            for (dst, sample) in out[offset..offset + frames * 4]
                .chunks_exact_mut(2)
                .zip(&samples[..sample_count])
            {
                dst.copy_from_slice(&sample.to_le_bytes());
            }
            offset += frames * 4;
            written += rendered;
        }

        written
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSubmode;
    impl SubmodeLookup for FakeSubmode {
        fn samples_for_one_symbol(_submode: Submode) -> f64 {
            1200.0
        }
        fn tone_spacing(_submode: Submode) -> f64 {
            10.0
        }
        fn period_seconds(_submode: Submode) -> u64 {
            10
        }
        fn start_delay_ms(_submode: Submode) -> u64 {
            500
        }
        fn tx_duration(_submode: Submode) -> f64 {
            8.0
        }
        fn compute_ratio(_submode: Submode) -> f64 {
            0.8
        }
        fn late_threshold_multiplier(_submode: Submode) -> f64 {
            1.0
        }
    }

    struct NoDelaySubmode;
    impl SubmodeLookup for NoDelaySubmode {
        fn samples_for_one_symbol(_submode: Submode) -> f64 {
            1200.0
        }
        fn tone_spacing(_submode: Submode) -> f64 {
            10.0
        }
        fn period_seconds(_submode: Submode) -> u64 {
            10
        }
        fn start_delay_ms(_submode: Submode) -> u64 {
            0
        }
        fn tx_duration(_submode: Submode) -> f64 {
            8.0
        }
        fn compute_ratio(_submode: Submode) -> f64 {
            0.8
        }
        fn late_threshold_multiplier(_submode: Submode) -> f64 {
            1.0
        }
    }

    #[test]
    fn read_returns_zero_after_transmission_finishes() {
        let mut modulator = Modulator::new();
        modulator.start_tones_with::<NoDelaySubmode>(
            &[0; 79],
            Submode::Normal,
            0,
            1500.0,
            Duration::ZERO,
            Channel::Mono,
        );

        let mut out = [0i16; 4096];
        for _ in 0..200 {
            let _ = modulator.render_stereo(&mut out);
            if modulator.is_idle() {
                break;
            }
        }
        assert!(modulator.is_idle());
        assert_eq!(modulator.render_stereo(&mut out), 0);
    }

    #[test]
    fn start_enters_synchronizing_when_start_delay_not_elapsed() {
        let mut modulator = Modulator::new();
        modulator.start_tones_with::<FakeSubmode>(
            &[0; 79],
            Submode::Normal,
            100, // 100ms into slot, still before 500ms start delay
            1500.0,
            Duration::ZERO,
            Channel::Mono,
        );
        assert_eq!(modulator.state(), State::Synchronizing);
    }

    #[test]
    fn start_without_delay_enters_active_and_generates_samples() {
        let mut modulator = Modulator::new();
        modulator.start_tones_with::<NoDelaySubmode>(
            &[0; 79],
            Submode::Fast,
            0,
            1500.0,
            Duration::ZERO,
            Channel::Mono,
        );
        assert_eq!(modulator.state(), State::Active);

        let mut out = [0i16; 1024];
        let frames = modulator.render_stereo(&mut out);
        assert_eq!(frames, out.len() / 2);
        assert!(out.iter().any(|&s| s != 0));
    }

    #[test]
    fn channel_routing_matches_mode() {
        let mut left = Modulator::new();
        left.start_tones_with::<NoDelaySubmode>(
            &[0; 79],
            Submode::Fast,
            0,
            1500.0,
            Duration::ZERO,
            Channel::Left,
        );

        let mut right = Modulator::new();
        right.start_tones_with::<NoDelaySubmode>(
            &[0; 79],
            Submode::Fast,
            0,
            1500.0,
            Duration::ZERO,
            Channel::Right,
        );

        let mut out_left = [0i16; 256];
        let mut out_right = [0i16; 256];
        let _ = left.render_stereo(&mut out_left);
        let _ = right.render_stereo(&mut out_right);

        for i in (0..out_left.len()).step_by(2) {
            assert_eq!(out_left[i + 1], 0);
        }
        for i in (0..out_right.len()).step_by(2) {
            assert_eq!(out_right[i], 0);
        }
    }

    #[test]
    fn active_transmission_eventually_returns_to_idle() {
        let mut modulator = Modulator::new();
        modulator.start_tones_with::<NoDelaySubmode>(
            &[0; 79],
            Submode::Normal,
            0,
            1500.0,
            Duration::ZERO,
            Channel::Mono,
        );

        let mut out = [0i16; 4096];
        for _ in 0..200 {
            let _ = modulator.render_stereo(&mut out);
            if modulator.is_idle() {
                break;
            }
        }

        assert!(modulator.is_idle());
    }
}
