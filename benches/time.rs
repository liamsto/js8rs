// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use js8rs::protocol::{DecodeModes, Submode};
use js8rs::rx::{DecodeConfig, Event, InputFormat, UntimedDecoder, UntimedReceiver};
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
            || UntimedDecoder::with_modes(DecodeModes::FAST),
            |mut decoder| {
                let decoded =
                    decoder.push(black_box(&samples), black_box(&config), |event: Event| {
                        black_box(event);
                    });
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
    let mut decoder = UntimedDecoder::with_modes(DecodeModes::FAST);
    let initial = vec![0; Submode::Fast.samples_per_period()];
    let samples = vec![0; FAST_STRIDE];
    decoder.push(&initial, &config, |_| {});

    c.bench_function("untimed_fast_idle_scan", |b| {
        b.iter(|| {
            let decoded = decoder.push(black_box(&samples), black_box(&config), |event: Event| {
                black_box(event);
            });
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
    let mut decoder = UntimedDecoder::with_modes(DecodeModes::FAST);
    let initial = noise(Submode::Fast.samples_per_period(), 0x1234_5678_9abc_def0);
    let samples = noise(FAST_STRIDE, 0xfedc_ba98_7654_3210);
    decoder.push(&initial, &config, |_| {});

    c.bench_function("untimed_fast_noise_scan", |b| {
        b.iter(|| {
            let decoded = decoder.push(black_box(&samples), black_box(&config), |event: Event| {
                black_box(event);
            });
            black_box(decoded);
        });
    });
}

fn bench_slow_period(c: &mut Criterion) {
    let config = DecodeConfig::default()
        .with_modes(DecodeModes::SLOW)
        .with_frequency_range(200, 3_000);
    let samples = vec![0; Submode::Slow.samples_per_period()];

    c.bench_function("untimed_slow_idle_30s", |b| {
        b.iter_batched(
            || {
                let mut decoder = UntimedDecoder::with_modes(DecodeModes::SLOW);
                decoder.push(&samples, &config, |_| {});
                decoder
            },
            |mut decoder| {
                let decoded =
                    decoder.push(black_box(&samples), black_box(&config), |event: Event| {
                        black_box(event);
                    });
                black_box(decoded);
            },
            BatchSize::PerIteration,
        );
    });
}

fn bench_decimator(c: &mut Criterion) {
    let config = DecodeConfig::default().with_modes(DecodeModes::NONE);
    let samples = vec![0i16; 48_000];
    let mut receiver = UntimedReceiver::with_modes(DecodeModes::NONE);

    c.bench_function("untimed_decimate_48k_mono", |b| {
        b.iter(|| {
            let decoded = receiver.write_i16(
                black_box(&samples),
                InputFormat::Mono,
                black_box(&config),
                |event: Event| {
                    black_box(event);
                },
            );
            black_box(decoded);
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
