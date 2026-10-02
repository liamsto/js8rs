// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use js8rs::protocol::{DecodeModes, Submode};
use js8rs::rx::{
    DecodeConfig, DecodeScheduler, Decoder, Detector, Event, InputFormat, SAMPLE_BUFFER_SIZE,
};
use std::hint::black_box;

mod support;
use support::synth_decode_fixture;

const FAST_STRIDE: usize = 24_000;

fn bench_signal(c: &mut Criterion) {
    let fixture = synth_decode_fixture(Submode::Fast, "HELLO WORLD");
    let mut samples = fixture.d2[..fixture.valid_samples].to_vec();
    samples.resize(Submode::Fast.samples_per_period(), 0);
    let config = fixture.config;

    c.bench_function("untimed_fast_signal", |b| {
        b.iter_batched(
            || Decoder::with_modes(DecodeModes::FAST),
            |mut decoder| {
                let decoded = decoder.decode(
                    black_box(&samples),
                    samples.len(),
                    black_box(&config),
                    |event: Event| {
                        black_box(event);
                    },
                );
                black_box(decoded);
            },
            BatchSize::PerIteration,
        );
    });
}

fn bench_idle_scans(c: &mut Criterion) {
    let config = DecodeConfig::default()
        .with_modes(DecodeModes::FAST)
        .with_frequency_range(200, 3_000);
    let mut decoder = Decoder::with_modes(DecodeModes::FAST);
    let samples = vec![0; SAMPLE_BUFFER_SIZE];
    let mut kin = Submode::Fast.samples_per_period();
    decoder.decode(&samples, kin, &config, |_| {});

    c.bench_function("untimed_fast_idle_scan", |b| {
        b.iter(|| {
            kin += FAST_STRIDE;
            let decoded = decoder.decode(
                black_box(&samples),
                kin,
                black_box(&config),
                |event: Event| {
                    black_box(event);
                },
            );
            black_box(decoded);
        });
    });
}

fn noise(len: usize, mut state: u64) -> Vec<i16> {
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state as i16) / 8
        })
        .collect()
}

fn bench_noise_scans(c: &mut Criterion) {
    let config = DecodeConfig::default()
        .with_modes(DecodeModes::FAST)
        .with_frequency_range(200, 3_000);
    let mut decoder = Decoder::with_modes(DecodeModes::FAST);
    let samples = noise(SAMPLE_BUFFER_SIZE, 0x1234_5678_9abc_def0);
    let mut kin = Submode::Fast.samples_per_period();
    decoder.decode(&samples, kin, &config, |_| {});

    c.bench_function("untimed_fast_noise_scan", |b| {
        b.iter(|| {
            kin += FAST_STRIDE;
            let decoded = decoder.decode(
                black_box(&samples),
                kin,
                black_box(&config),
                |event: Event| {
                    black_box(event);
                },
            );
            black_box(decoded);
        });
    });
}

fn bench_slow_period(c: &mut Criterion) {
    let config = DecodeConfig::default()
        .with_modes(DecodeModes::SLOW)
        .with_frequency_range(200, 3_000);
    let samples = vec![0; SAMPLE_BUFFER_SIZE];

    c.bench_function("untimed_slow_idle_30s", |b| {
        b.iter_batched(
            || {
                let mut decoder = Decoder::with_modes(DecodeModes::SLOW);
                decoder.decode(
                    &samples,
                    Submode::Slow.samples_per_period(),
                    &config,
                    |_| {},
                );
                decoder
            },
            |mut decoder| {
                let mut scheduler = DecodeScheduler::new();
                let period = Submode::Slow.samples_per_period();
                scheduler.next_window(Submode::Slow, period, 0);
                for kin in (period + 1..=period * 2).step_by(256) {
                    if let Some(window) = scheduler.next_window(Submode::Slow, kin, kin - 1) {
                        black_box(decoder.decode(
                            black_box(&samples),
                            window.start + window.size,
                            black_box(&config),
                            |event: Event| {
                                black_box(event);
                            },
                        ));
                    }
                }
            },
            BatchSize::PerIteration,
        );
    });
}

fn bench_decimator(c: &mut Criterion) {
    let samples = vec![0i16; 48_000];
    let detector = Detector::new(60, 64);

    c.bench_function("untimed_decimate_48k_mono", |b| {
        b.iter(|| {
            black_box(detector.write_i16(black_box(&samples), InputFormat::Mono));
        });
    });
}

criterion_group!(
    benches,
    bench_signal,
    bench_idle_scans,
    bench_noise_scans,
    bench_slow_period,
    bench_decimator
);
criterion_main!(benches);
