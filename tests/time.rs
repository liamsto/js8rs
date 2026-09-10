// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

#![cfg(feature = "experimental-time")]

use js8rs::codec::{BuildFramesOptions, build_frames};
use js8rs::protocol::{DecodeModes, Submode};
use js8rs::rx::{DecodeConfig, Event, InputFormat, SAMPLE_BUFFER_SIZE, UntimedReceiver};
use js8rs::tx::{Channel, Modulator, State};

const OFFSET_48K: usize = 64_176;

#[test]
fn immediate_transmission_ignores_slot_position() {
    let frame = build_frames(&BuildFramesOptions::new("HELLO", Submode::Fast))
        .encode()
        .unwrap()
        .remove(0);
    let mut modulator = Modulator::new();

    modulator.start_immediate(&frame, 1500.0, Channel::Mono);

    assert_eq!(modulator.state(), State::Active);
    let mut out = [0; 512];
    assert_eq!(modulator.render_stereo(&mut out), out.len() / 2);
    assert!(out.iter().any(|&sample| sample != 0));
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
    let mut receiver = UntimedReceiver::with_modes(modes);
    let silence = [0i16; 4_096];
    let start = SAMPLE_BUFFER_SIZE - 20_000;

    let mut remaining = start * 4;
    while remaining != 0 {
        let count = remaining.min(silence.len());
        receiver.write_i16(&silence[..count], InputFormat::Mono, &disabled, |_| {});
        remaining -= count;
    }

    let mut decoded = None;
    let mut modulator = Modulator::new();
    modulator.start_immediate(&encoded, 1500.0, Channel::Mono);
    let mut pcm = [0i16; 2_048];
    while !modulator.is_idle() {
        let frames = modulator.render_stereo(&mut pcm);
        receiver.write_i16(
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
        receiver.write_i16(&silence, InputFormat::Mono, &config, |event| {
            if let Event::Decoded(frame) = event {
                decoded = Some(frame);
            }
        });
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
    let mut receiver = UntimedReceiver::with_modes(modes);
    let mut decoded = Vec::new();

    let silence = [0i16; 1_024];
    let mut remaining = offset_48k;
    while remaining != 0 {
        let count = remaining.min(silence.len());
        receiver.write_i16(&silence[..count], InputFormat::Mono, &config, |_| {});
        remaining -= count;
    }

    let mut modulator = Modulator::new();
    modulator.start_immediate(&encoded, 1500.0, Channel::Mono);
    let mut pcm = [0i16; 2_048];
    while !modulator.is_idle() {
        let frames = modulator.render_stereo(&mut pcm);
        receiver.write_i16(
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
        receiver.write_i16(&silence, InputFormat::Mono, &config, |event| {
            if let Event::Decoded(frame) = event {
                decoded.push(frame);
            }
        });
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

    // The next overlapping scan sees the same frame but must not emit it again.
    let duplicate_span = 2 * 48_000;
    for _ in (0..duplicate_span).step_by(silence.len()) {
        receiver.write_i16(&silence, InputFormat::Mono, &config, |event| {
            if let Event::Decoded(frame) = event {
                decoded.push(frame);
            }
        });
    }
    assert_eq!(decoded.len(), 1, "{mode} duplicate suppression");
}
