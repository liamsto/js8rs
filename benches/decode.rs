// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use js8rs::protocol::Submode;
use js8rs::rx::{DecodeConfig, Decoder, Event};
use std::{hint::black_box, time::Duration};

mod support;
use support::{DecodeFixture, synth_decode_fixture, validate_decode_fixture};

fn bench_dec(c: &mut Criterion, fixture: &DecodeFixture, name: &str) {
    c.bench_function(name, |b| {
        b.iter_batched(
            || {
                let dec = Decoder::new();
                let config = fixture.config;
                (dec, config)
            },
            |(mut dec, config): (Decoder, DecodeConfig)| {
                let mut n_events = 0usize;
                let n = dec.decode(
                    black_box(&fixture.d2),
                    black_box(fixture.valid_samples),
                    black_box(&config),
                    |ev: Event| {
                        black_box(&ev);
                        n_events += 1;
                    },
                );
                black_box(n);
                black_box(n_events);
                (dec, config)
            },
            BatchSize::PerIteration,
        )
    });
}

fn bench_decode_all_submodes(c: &mut Criterion) {
    for (mode, name) in [
        (Submode::Normal, "decode_js8_normal"),
        (Submode::Fast, "decode_js8_fast"),
        (Submode::Turbo, "decode_js8_turbo"),
        (Submode::Slow, "decode_js8_slow"),
        (Submode::Ultra, "decode_js8_ultra"),
    ] {
        let fixture = synth_decode_fixture(mode, "HELLO WORLD");
        let rep = synth_decode_fixture(mode, "HELLO WORLD");
        assert_eq!(fixture.valid_samples, rep.valid_samples);
        assert_eq!(fixture.d2, rep.d2);
        validate_decode_fixture(&fixture);
        bench_dec(c, &fixture, name);
    }

    let mut fixture = synth_decode_fixture(Submode::Normal, "HELLO WORLD");
    let weak = synth_decode_fixture(Submode::Normal, "CQ CQ");
    for (v, w) in fixture.d2.iter_mut().zip(&weak.d2) {
        *v = *v / 2 + *w / 16;
    }
    validate_decode_fixture(&fixture);
    fixture.encoded = weak.encoded;
    validate_decode_fixture(&fixture);
    bench_dec(c, &fixture, "decode_normal_overlap");

    let mut rng = 0x051c_0372_u32;
    for v in &mut fixture.d2 {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        *v = (rng as i16) / 8;
    }
    assert_eq!(
        Decoder::new().decode(&fixture.d2, fixture.valid_samples, &fixture.config, |_| {}),
        0,
    );
    bench_dec(c, &fixture, "decode_normal_noise");
}

criterion_group!(name = benches; config = Criterion::default().measurement_time(Duration::from_secs(30)); targets = bench_decode_all_submodes);
criterion_main!(benches);
