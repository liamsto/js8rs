// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

//! Continuous receive support for the normal decoder API.

use super::{DecodeConfig, DecodeFinished, Decoded, Decoder, Event, window_from_kin};
use crate::internal::commons::{DecData, JS8_RX_SAMPLE_RATE};
use crate::protocol::Submode;

const MODES: [Submode; 5] = [
    Submode::Normal,
    Submode::Fast,
    Submode::Turbo,
    Submode::Slow,
    Submode::Ultra,
];

impl Decoder {
    pub(super) fn decode_stream<E>(
        &mut self,
        samples: &[i16],
        end: usize,
        config: &DecodeConfig,
        emit: &mut E,
    ) -> usize
    where
        E: FnMut(Event),
    {
        let starts = MODES.map(|mode| {
            window_from_kin(end, mode.samples_per_period().min(samples.len())).0 as u64
        });
        let mut params = config.legacy(end, samples.len());
        // Only reuse spectra for advancing, complete windows, partial windows contain zero padding which changes as new samples arrive
        let mut reuse = true;
        for (i, mode) in MODES.iter().copied().enumerate() {
            if config.modes.contains(mode.into()) {
                if end < self.ends[i] {
                    self.seen.retain(|entry| entry.2 != mode);
                }
                reuse &= end > self.ends[i] && end.min(samples.len()) >= mode.samples_per_period();
                self.ends[i] = end;
            }
        }
        if reuse {
            params.stream_starts = Some(starts);
        }

        let mut data = DecData {
            d2: samples,
            params,
        };
        let seen = &mut self.seen;
        let mut decoded = 0;
        self.core.decode_pass(&mut data, &mut |event| match event {
            Event::Decoded(mut frame) => {
                let mode = frame.frame.submode;
                let i = MODES.iter().position(|&m| m == mode).unwrap();
                let offset = frame.time_offset_seconds + mode.start_delay_ms() as f32 / 1_000.0;
                let offset = (offset * JS8_RX_SAMPLE_RATE as f32)
                    .round()
                    .clamp(0.0, mode.samples_per_period() as f32)
                    as u64;
                let start = starts[i] + offset;
                frame.sample_position = Some(start);
                if remember(seen, &frame, start) {
                    decoded += 1;
                    emit(Event::Decoded(frame));
                }
            }
            Event::DecodeFinished(_) => emit(Event::DecodeFinished(DecodeFinished { decoded })),
            other => emit(other),
        });
        decoded
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
