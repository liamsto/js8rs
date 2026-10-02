// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

#![cfg(feature = "experimental-time")]

use js8rs::codec::{BuildFramesOptions, build_frames};
use js8rs::protocol::{DecodeModes, Submode};
use js8rs::rx::{
    DecodeConfig, DecodeScheduler, Decoder, Detector, Event, InputFormat, SAMPLE_BUFFER_SIZE,
};
use js8rs::tx::{Channel, Modulator, State};
use std::time::Duration;

const OFFSET_48K: usize = 64_176;

#[test]
fn ignores_pos() {
    for mode in [
        Submode::Normal,
        Submode::Fast,
        Submode::Turbo,
        Submode::Slow,
        Submode::Ultra,
    ] {
        let frame = build_frames(&BuildFramesOptions::new("HELLO", mode))
            .encode()
            .unwrap()
            .remove(0);
        let mut start = Modulator::new();
        let mut late = Modulator::new();
        start.start(&frame, 0, 1500.0, Duration::ZERO, Channel::Mono);
        late.start_tones(
            &frame.tones,
            mode,
            u64::MAX,
            1500.0,
            Duration::from_secs(2),
            Channel::Mono,
        );
        assert_eq!(start.state(), State::Active);
        assert_eq!(late.state(), State::Active);
        let mut expected = [0; 2048];
        let mut actual = [0; 2048];
        let mut nonzero = false;
        while !start.is_idle() {
            let count = start.render_stereo(&mut expected);
            assert_eq!(late.render_stereo(&mut actual), count);
            assert_eq!(actual[..count * 2], expected[..count * 2]);
            nonzero |= actual[..count * 2].iter().any(|&sample| sample != 0);
        }
        assert!(nonzero);
        assert!(late.is_idle());
    }
}

#[test]
fn arbitrary_start_roundtrips_in_every_submode() {
    for mode in [
        Submode::Normal,
        Submode::Fast,
        Submode::Turbo,
        Submode::Slow,
        Submode::Ultra,
    ] {
        roundtrip(mode, OFFSET_48K);
    }
}

#[test]
fn transmission_crossing_a_nominal_slot_roundtrips() {
    let offset = 5 * 48_000 + OFFSET_48K;
    roundtrip(Submode::Fast, offset);
}

#[test]
fn slow_transmission_near_scan_limit_roundtrips() {
    let offset = 3 * 48_000 + 140_000;
    roundtrip(Submode::Slow, offset);
}

#[test]
fn transmission_crossing_the_sample_ring_roundtrips() {
    let mode = Submode::Fast;
    let modes = DecodeModes::from(mode);
    let disabled = DecodeConfig::default().with_modes(DecodeModes::NONE);
    let config = DecodeConfig::default()
        .with_modes(modes)
        .with_nominal_frequency(1500)
        .with_frequency_range(200, 3_000);
    let encoded = build_frames(&BuildFramesOptions::new("RING WRAP", mode))
        .encode()
        .unwrap()
        .remove(0);
    let detector = Detector::new(60, 64);
    let mut decoder = Decoder::with_modes(modes);
    let mut scheduler = DecodeScheduler::new();
    let silence = [0i16; 4_096];
    let start = SAMPLE_BUFFER_SIZE - 20_000;

    let mut remaining = start * 4;
    while remaining != 0 {
        let count = remaining.min(silence.len());
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &silence[..count],
            InputFormat::Mono,
            &disabled,
            |_| {},
        );
        remaining -= count;
    }

    let mut decoded = None;
    let mut modulator = Modulator::new();
    modulator.start(&encoded, 9_999, 1500.0, Duration::ZERO, Channel::Mono);
    let mut pcm = [0i16; 2_048];
    while !modulator.is_idle() {
        let frames = modulator.render_stereo(&mut pcm);
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &pcm[..frames * 2],
            InputFormat::StereoLeft,
            &config,
            |event| {
                if let Event::Decoded(frame) = event {
                    decoded = Some(frame);
                }
            },
        );
    }

    for _ in 0..128 {
        if decoded.is_some() {
            break;
        }
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &silence,
            InputFormat::Mono,
            &config,
            |event| {
                if let Event::Decoded(frame) = event {
                    decoded = Some(frame);
                }
            },
        );
    }

    let decoded = decoded.expect("ring-spanning transmission should decode");
    assert_eq!(decoded.frame.encoded, encoded.frame.encoded);
    let actual = decoded.sample_position.unwrap();
    assert!(actual.abs_diff(start as u64) < mode.samples_for_one_symbol());
}

fn roundtrip(mode: Submode, offset_48k: usize) {
    let encoded = build_frames(&BuildFramesOptions::new("HELLO WORLD", mode))
        .encode()
        .unwrap()
        .remove(0);
    let modes = DecodeModes::from(mode);
    let config = DecodeConfig::default()
        .with_modes(modes)
        .with_nominal_frequency(1500)
        .with_frequency_range(200, 3_000);
    let detector = Detector::new(60, 64);
    let mut decoder = Decoder::with_modes(modes);
    let mut scheduler = DecodeScheduler::new();
    let mut decoded = Vec::new();

    let silence = [0i16; 1_024];
    let mut remaining = offset_48k;
    while remaining != 0 {
        let count = remaining.min(silence.len());
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &silence[..count],
            InputFormat::Mono,
            &config,
            |_| {},
        );
        remaining -= count;
    }

    let mut modulator = Modulator::new();
    modulator.start(&encoded, 9_999, 1500.0, Duration::ZERO, Channel::Mono);
    let mut pcm = [0i16; 2_048];
    while !modulator.is_idle() {
        let frames = modulator.render_stereo(&mut pcm);
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &pcm[..frames * 2],
            InputFormat::StereoLeft,
            &config,
            |event| {
                if let Event::Decoded(frame) = event {
                    decoded.push(frame);
                }
            },
        );
    }

    let limit = mode.samples_per_period() * 4;
    let mut trailing = 0;
    while decoded.is_empty() && trailing < limit {
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &silence,
            InputFormat::Mono,
            &config,
            |event| {
                if let Event::Decoded(frame) = event {
                    decoded.push(frame);
                }
            },
        );
        trailing += silence.len();
    }

    assert_eq!(decoded.len(), 1, "{mode} decode count");
    assert_eq!(decoded[0].frame.encoded, encoded.frame.encoded);
    let expected = (offset_48k / 4) as u64;
    let actual = decoded[0]
        .sample_position
        .expect("untimed decode must report a stream position");
    assert!(
        actual.abs_diff(expected) < mode.samples_for_one_symbol(),
        "{mode}: expected start near {expected}, got {actual}"
    );

    // Another mode may have an earlier window end. Its scan must not discard
    // duplicate history for this mode.
    let other = if mode == Submode::Fast {
        DecodeModes::TURBO
    } else {
        DecodeModes::FAST
    };
    detector.with_samples(|samples, kin| {
        decoder.decode(samples, kin / 2, &config.with_modes(other), |_| {});
    });

    // The next overlapping scan sees the same frame but must not emit it again.
    let duplicate_span = mode.samples_per_period() * 4;
    for _ in (0..duplicate_span).step_by(silence.len()) {
        receive(
            &detector,
            &mut decoder,
            &mut scheduler,
            mode,
            &silence,
            InputFormat::Mono,
            &config,
            |event| {
                if let Event::Decoded(frame) = event {
                    decoded.push(frame);
                }
            },
        );
    }
    assert_eq!(decoded.len(), 1, "{mode} duplicate suppression");
}

// Use the same receive process as a slot-based caller: write, schedule, decode.
fn receive(
    detector: &Detector,
    decoder: &mut Decoder,
    scheduler: &mut DecodeScheduler,
    mode: Submode,
    pcm: &[i16],
    format: InputFormat,
    config: &DecodeConfig,
    emit: impl FnMut(Event),
) {
    let prev = detector.kin();
    let stats = detector.write_i16(pcm, format);
    assert_eq!(stats.frames_dropped, 0);
    if let Some(window) = scheduler.next_window(mode, stats.kin_after, prev) {
        detector.with_samples(|samples, _| {
            decoder.decode(samples, window.start + window.size, config, emit);
        });
    }
}
