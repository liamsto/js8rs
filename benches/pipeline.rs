// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use js8rs::codec::encode_tones;
use js8rs::protocol::Submode;
use js8rs::rx::{Decoder, Detector, Event};
use std::hint::black_box;

mod support;
use support::{
    bench_config, modulate_into_detector_with_buffers, synth_decode_fixture, synth_encode_fixture,
    validate_decode_fixture,
};

fn bench_modulate_detect(c: &mut Criterion) {
    let enc = synth_encode_fixture(Submode::Fast, "HELLO WORLD");
    let tones =
        encode_tones(enc.flags, enc.submode, &enc.frame12).expect("encode_tones should work");

    c.bench_function("modulate_detect_js8_fast", |b| {
        b.iter_batched(
            || (Detector::new(1, 1024), vec![0i16; 2048], vec![0i16; 1024]),
            |(detector, mut stereo, mut mono)| {
                modulate_into_detector_with_buffers(
                    black_box(&tones),
                    Submode::Fast,
                    &detector,
                    &mut stereo,
                    &mut mono,
                );
                black_box(detector.kin());
                (detector, stereo, mono)
            },
            BatchSize::PerIteration,
        );
    });
}

fn bench_full_chain_fast(c: &mut Criterion) {
    let enc = synth_encode_fixture(Submode::Fast, "HELLO WORLD");
    let decoded = synth_decode_fixture(Submode::Fast, "HELLO WORLD");
    validate_decode_fixture(&decoded);

    c.bench_function("full_chain_js8_fast", |b| {
        b.iter_batched(
            || {
                (
                    Decoder::new(),
                    Detector::new(1, 1024),
                    vec![0i16; 2048],
                    vec![0i16; 1024],
                )
            },
            |(mut decoder, detector, mut stereo, mut mono)| {
                let tones = encode_tones(
                    black_box(enc.flags),
                    black_box(enc.submode),
                    black_box(&enc.frame12),
                )
                .expect("encode_tones should work");
                modulate_into_detector_with_buffers(
                    &tones,
                    Submode::Fast,
                    &detector,
                    &mut stereo,
                    &mut mono,
                );
                let (decoded_count, n_events) = detector.with_samples(|samples, kin_end| {
                    let config = bench_config(Submode::Fast);
                    let mut n_events = 0usize;
                    let decoded_count =
                        decoder.decode(samples, kin_end, &config, |event: Event| {
                            black_box(&event);
                            n_events += 1;
                        });
                    (decoded_count, n_events)
                });

                black_box(decoded_count);
                black_box(n_events);
                (decoder, detector, stereo, mono)
            },
            BatchSize::PerIteration,
        );
    });
}

criterion_group!(benches, bench_modulate_detect, bench_full_chain_fast);
criterion_main!(benches);
