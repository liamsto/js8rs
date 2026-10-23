// SPDX-License-Identifier: GPL-3.0-only

use js8rs::codec::encode_tones;
use js8rs::protocol::{DecodeModes, FrameFlags, Submode};
use js8rs::rx::{DecodeConfig, Decoder, Event, SAMPLE_BUFFER_SIZE};
use std::f64::consts::TAU;

fn add(dd: &mut [f64], fr: &[u8; 12], hz: f64, amp: f64) {
    let mode = Submode::Normal;
    let tones = encode_tones(FrameFlags::ALL, mode, fr).unwrap();
    let nsps = mode.samples_for_one_symbol() as usize;
    let start = mode.start_delay_ms() as usize * 12;
    let mut ph: f64 = 0.0;
    for (sym, &tone) in tones.iter().enumerate() {
        let step = TAU * (hz / 12_000.0 + f64::from(tone) / nsps as f64);
        for i in 0..nsps {
            dd[start + sym * nsps + i] += amp * ph.cos();
            ph += step;
        }
    }
}

fn noise(amp: f64) -> Vec<f64> {
    let mut rng = 0x051c_0372_u32;
    (0..SAMPLE_BUFFER_SIZE)
        .map(|_| {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            amp * (f64::from(rng) / f64::from(u32::MAX) * 2.0 - 1.0)
        })
        .collect()
}

fn decode(dec: &mut Decoder, dd: &[f64]) -> Vec<String> {
    let pcm: Vec<_> = dd.iter().map(|v| v.round() as i16).collect();
    let cfg = DecodeConfig::default()
        .with_modes(DecodeModes::NORMAL)
        .with_nominal_frequency(1500)
        .with_frequency_range(1400, 1700);
    let mut got = Vec::new();
    let mut finished = false;
    dec.decode(
        &pcm,
        Submode::Normal.samples_per_period(),
        &cfg,
        |ev| match ev {
            Event::Decoded(v) => got.push(v.frame.encoded),
            Event::DecodeFinished(_) => finished = true,
            _ => {}
        },
    );
    assert!(finished);
    got
}

#[test]
fn overlap_weak_strong() {
    let mut dd = noise(120.0);
    add(&mut dd, b"UBXRtA7+++++", 1500.0, 12_000.0);
    add(&mut dd, b"sAaBrDiV++++", 1506.25, 600.0);
    let got = decode(&mut Decoder::new(), &dd);
    assert!(
        got.iter().any(|v| v == "UBXRtA7+++++"),
        "strong frame missing: {got:?}"
    );
    assert!(
        got.iter().any(|v| v == "sAaBrDiV++++"),
        "weak frame missing: {got:?}"
    );
}

#[test]
fn overlap_near() {
    for hz in [1500.0, 1503.125] {
        let mut dd = vec![0.0; SAMPLE_BUFFER_SIZE];
        add(&mut dd, b"UBXRtA7+++++", 1500.0, 12_000.0);
        add(&mut dd, b"sAaBrDiV++++", hz, 2400.0);
        let mut dec = Decoder::new();
        let got = decode(&mut dec, &dd);
        assert!(got.iter().any(|v| v == "UBXRtA7+++++"));
        assert!(
            got.iter().any(|v| v == "sAaBrDiV++++"),
            "weak frame missing at {hz} Hz: {got:?}"
        );
        assert!(decode(&mut dec, &noise(2000.0)).is_empty());
    }
}

#[test]
fn single_frame() {
    let mut dd = noise(120.0);
    add(&mut dd, b"UBXRtA7+++++", 1500.0, 12_000.0);
    let got = decode(&mut Decoder::new(), &dd);
    assert!(got.iter().any(|v| v == "UBXRtA7+++++"));
}

#[test]
fn noise_no_frames() {
    assert!(decode(&mut Decoder::new(), &noise(2000.0)).is_empty());
}
