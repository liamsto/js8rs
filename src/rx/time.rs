// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

//! Continuous receive helpers that do not depend on UTC-aligned slots.

use super::{
    DecodeConfig, DecodeFinished, Decoded, Decoder, Event, InputFormat, SAMPLE_BUFFER_SIZE,
};
use crate::detector::{FirDecimator49x4, LOWPASS};
use crate::internal::commons::{DecData, DecodeParams, JS8_RX_SAMPLE_RATE};
use crate::protocol::{DecodeModes, Submode};

const MODES: [Submode; 5] = [
    Submode::Normal,
    Submode::Fast,
    Submode::Turbo,
    Submode::Slow,
    Submode::Ultra,
];
/// Continuous decoder for new 12 kHz mono samples.
///
/// Samples are retained in a 60s ring.
pub struct UntimedDecoder {
    decoder: Decoder,
    prepared: DecodeModes,
    samples: Vec<i16>,
    total: u64,
    next: [u64; MODES.len()],
    active: DecodeModes,
    seen: Vec<([u8; 12], u8, Submode, u64)>,
    scans: u64,
}

impl UntimedDecoder {
    /// Creates a decoder prepared for every supported submode.
    #[must_use]
    pub fn new() -> Self {
        Self::with_modes(DecodeModes::ALL)
    }

    /// Prepares the selected modes up front. Other modes remain available and
    /// are initialized lazily if selected by [`DecodeConfig`].
    #[must_use]
    pub fn with_modes(modes: DecodeModes) -> Self {
        Self {
            decoder: Decoder::with_modes(modes),
            prepared: modes,
            samples: vec![0; SAMPLE_BUFFER_SIZE],
            total: 0,
            next: MODES.map(|mode| mode.samples_per_period() as u64),
            active: DecodeModes::NONE,
            seen: Vec::with_capacity(64),
            scans: 0,
        }
    }

    /// Pushes newly received 12 kHz mono samples and emits completed decoder
    /// passes as soon as their overlapping windows are ready.
    ///
    /// The return value is the number of non-duplicate frames emitted during
    /// this call.
    pub fn push<E>(&mut self, mut input: &[i16], config: &DecodeConfig, mut emit: E) -> usize
    where
        E: FnMut(Event),
    {
        self.activate(config.modes);
        let mut decoded = 0;

        loop {
            let Some(end) = self.next_end(config.modes) else {
                self.write_ring(input);
                break;
            };

            if self.total < end && !input.is_empty() {
                let count = ((end - self.total) as usize).min(input.len());
                self.write_ring(&input[..count]);
                input = &input[count..];
            }

            if self.total == end {
                decoded += self.decode_ready(config, &mut emit);
                continue;
            }

            if input.is_empty() {
                break;
            }
        }

        decoded
    }

    /// Number of 12 kHz samples accepted since construction or reset.
    #[must_use]
    pub const fn sample_count(&self) -> u64 {
        self.total
    }

    /// Number of overlapping decoder passes performed.
    #[must_use]
    pub const fn scan_count(&self) -> u64 {
        self.scans
    }

    /// Clears buffered samples, scheduling state, and duplicate history.
    pub fn reset(&mut self) {
        self.decoder = Decoder::with_modes(self.prepared);
        self.samples.fill(0);
        self.total = 0;
        self.next = MODES.map(|mode| mode.samples_per_period() as u64);
        self.active = DecodeModes::NONE;
        self.seen.clear();
        self.scans = 0;
    }

    fn activate(&mut self, modes: DecodeModes) {
        let added = modes.bits() & !self.active.bits();
        for mode in MODES {
            let bit = DecodeModes::from(mode).bits();
            if added & bit != 0 {
                let i = mode_index(mode);
                self.next[i] = self.total.max(mode.samples_per_period() as u64);
            }
        }
        self.active = modes;
    }

    fn next_end(&self, modes: DecodeModes) -> Option<u64> {
        MODES
            .iter()
            .copied()
            .filter(|mode| modes.contains((*mode).into()))
            .map(|mode| self.next[mode_index(mode)])
            .min()
    }

    fn write_ring(&mut self, mut input: &[i16]) {
        while !input.is_empty() {
            let pos = (self.total % SAMPLE_BUFFER_SIZE as u64) as usize;
            let count = input.len().min(SAMPLE_BUFFER_SIZE - pos);
            self.samples[pos..pos + count].copy_from_slice(&input[..count]);
            self.total += count as u64;
            input = &input[count..];
        }
    }

    fn decode_ready<E>(&mut self, config: &DecodeConfig, emit: &mut E) -> usize
    where
        E: FnMut(Event),
    {
        let mut modes = DecodeModes::NONE;
        let mut starts = [0u64; MODES.len()];
        let mut params = config.legacy(0);

        for mode in MODES {
            let i = mode_index(mode);
            if config.modes.contains(mode.into()) && self.next[i] == self.total {
                let size = mode.samples_per_period();
                let start = self.total - size as u64;
                starts[i] = start;
                let pos = (start % SAMPLE_BUFFER_SIZE as u64) as usize;
                set_window(&mut params, mode, pos, size);
                modes |= mode.into();
                self.next[i] += scan_stride(mode);
            }
        }

        if modes == DecodeModes::NONE {
            return 0;
        }

        params.nsubmodes = modes.bits();
        params.stream_starts = Some(starts);
        self.scans += 1;

        let mut data = DecData {
            d2: &self.samples,
            params,
        };
        let seen = &mut self.seen;
        let mut decoded = 0;

        self.decoder
            .core
            .decode_pass(&mut data, &mut |event| match event {
                Event::Decoded(mut frame) => {
                    let mode = frame.frame.submode;
                    let offset = frame.time_offset_seconds + mode.start_delay_ms() as f32 / 1_000.0;
                    let offset = (offset * JS8_RX_SAMPLE_RATE as f32)
                        .round()
                        .clamp(0.0, mode.samples_per_period() as f32)
                        as u64;
                    let start = starts[mode_index(mode)] + offset;
                    frame.sample_position = Some(start);

                    if remember(seen, &frame, start) {
                        decoded += 1;
                        emit(Event::Decoded(frame));
                    }
                }
                Event::DecodeFinished(_) => {
                    emit(Event::DecodeFinished(DecodeFinished { decoded }));
                }
                other => emit(other),
            });

        decoded
    }
}

impl Default for UntimedDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Continuous 48 kHz PCM receiver with integrated decimation and decoding.
pub struct UntimedReceiver {
    decoder: UntimedDecoder,
    filter: FirDecimator49x4,
    pending: [i16; 4],
    pending_len: usize,
    decimated: Vec<i16>,
}

impl UntimedReceiver {
    /// Creates a receiver prepared for every supported submode.
    #[must_use]
    pub fn new() -> Self {
        Self::with_modes(DecodeModes::ALL)
    }

    /// Creates a receiver with the selected decoder modes prepared up front.
    #[must_use]
    pub fn with_modes(modes: DecodeModes) -> Self {
        Self {
            decoder: UntimedDecoder::with_modes(modes),
            filter: FirDecimator49x4::new(LOWPASS),
            pending: [0; 4],
            pending_len: 0,
            decimated: Vec::new(),
        }
    }

    /// Accepts new 48 kHz PCM and synchronously runs any ready decode passes.
    ///
    /// Input channel interpretation matches [`crate::rx::Detector::write_i16`].
    /// The return value is the number of non-duplicate frames emitted.
    pub fn write_i16<E>(
        &mut self,
        input: &[i16],
        format: InputFormat,
        config: &DecodeConfig,
        emit: E,
    ) -> usize
    where
        E: FnMut(Event),
    {
        self.decimated.clear();
        let frames = match format {
            InputFormat::Mono => input.len(),
            InputFormat::StereoLeft | InputFormat::StereoAverage => input.len() / 2,
        };
        self.decimated.reserve((self.pending_len + frames) / 4);

        match format {
            InputFormat::Mono => {
                for &sample in input {
                    self.push_input(sample);
                }
            }
            InputFormat::StereoLeft => {
                for frame in input.chunks_exact(2) {
                    self.push_input(frame[0]);
                }
            }
            InputFormat::StereoAverage => {
                for frame in input.chunks_exact(2) {
                    let left = i32::from(frame[0]);
                    let right = i32::from(frame[1]);
                    self.push_input(i32::midpoint(left, right) as i16);
                }
            }
        }

        self.decoder.push(&self.decimated, config, emit)
    }

    /// Accesses the continuous 12 kHz decoder state.
    #[must_use]
    pub const fn decoder(&self) -> &UntimedDecoder {
        &self.decoder
    }

    /// Mutably accesses the continuous 12 kHz decoder state.
    pub const fn decoder_mut(&mut self) -> &mut UntimedDecoder {
        &mut self.decoder
    }

    /// Clears the decimator and decoder state.
    pub fn reset(&mut self) {
        self.decoder.reset();
        self.filter = FirDecimator49x4::new(LOWPASS);
        self.pending_len = 0;
        self.pending.fill(0);
        self.decimated.clear();
    }

    fn push_input(&mut self, sample: i16) {
        self.pending[self.pending_len] = sample;
        self.pending_len += 1;
        if self.pending_len == self.pending.len() {
            self.decimated
                .push(self.filter.down_sample_i16(self.pending));
            self.pending_len = 0;
        }
    }
}

impl Default for UntimedReceiver {
    fn default() -> Self {
        Self::new()
    }
}

const fn mode_index(mode: Submode) -> usize {
    match mode {
        Submode::Normal => 0,
        Submode::Fast => 1,
        Submode::Turbo => 2,
        Submode::Slow => 3,
        Submode::Ultra => 4,
    }
}

const fn scan_stride(mode: Submode) -> u64 {
    let period = mode.samples_per_period();
    let slack = period - mode.samples_for_symbols() as usize;
    let guard = mode.samples_for_one_symbol() as usize;
    let stride = slack.saturating_sub(guard);
    let limit = match mode {
        Submode::Slow => {
            let step = mode.samples_for_one_symbol() as usize / 4;
            let limit = 3 * JS8_RX_SAMPLE_RATE as usize;
            limit - limit % step
        }
        _ => usize::MAX,
    };
    let stride = if stride < limit { stride } else { limit };
    if stride == 0 { 1 } else { stride as u64 }
}

fn set_window(params: &mut DecodeParams, mode: Submode, start: usize, size: usize) {
    match mode {
        Submode::Normal => (params.kpos_a, params.ksz_a) = (start, size),
        Submode::Fast => (params.kpos_b, params.ksz_b) = (start, size),
        Submode::Turbo => (params.kpos_c, params.ksz_c) = (start, size),
        Submode::Slow => (params.kpos_e, params.ksz_e) = (start, size),
        Submode::Ultra => (params.kpos_i, params.ksz_i) = (start, size),
    }
}

fn remember(seen: &mut Vec<([u8; 12], u8, Submode, u64)>, frame: &Decoded, start: u64) -> bool {
    let mut encoded = [0; 12];
    let bytes = frame.frame.encoded.as_bytes();
    let len = bytes.len().min(encoded.len());
    encoded[..len].copy_from_slice(&bytes[..len]);

    let range = frame.frame.submode.samples_per_period() as u64 / 2;
    let oldest = start.saturating_sub(30 * JS8_RX_SAMPLE_RATE);
    seen.retain(|entry| entry.3 >= oldest);
    if seen.iter().any(|entry| {
        entry.0 == encoded
            && entry.1 == frame.frame.flags.bits()
            && entry.2 == frame.frame.submode
            && entry.3.abs_diff(start) < range
    }) {
        return false;
    }

    seen.push((
        encoded,
        frame.frame.flags.bits(),
        frame.frame.submode,
        start,
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strides_cover_the_frame_slack() {
        for mode in MODES {
            let slack = mode.samples_per_period() - mode.samples_for_symbols() as usize;
            assert!((scan_stride(mode) as usize) < slack);
        }
        assert_eq!(scan_stride(Submode::Slow), 35_520);
    }

    #[test]
    fn ring_accepts_more_than_one_complete_buffer() {
        let mut decoder = UntimedDecoder::with_modes(DecodeModes::NONE);
        let config = DecodeConfig::default().with_modes(DecodeModes::NONE);
        let input = vec![7; SAMPLE_BUFFER_SIZE + 123];
        assert_eq!(decoder.push(&input, &config, |_| {}), 0);
        assert_eq!(decoder.sample_count(), (SAMPLE_BUFFER_SIZE + 123) as u64);
    }
}
