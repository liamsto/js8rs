use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use js8rs::protocol::Submode;
use js8rs::rx::{DecodeConfig, Decoder, Event};
use std::{hint::black_box, time::Duration};

mod support;
use support::{synth_decode_fixture, validate_decode_fixture};

fn bench_decode_submode(c: &mut Criterion, submode: Submode, name: &str) {
    let fixture = synth_decode_fixture(submode, "HELLO WORLD");
    validate_decode_fixture(&fixture);

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
    bench_decode_submode(c, Submode::Normal, "decode_js8_normal");
    bench_decode_submode(c, Submode::Fast, "decode_js8_fast");
    bench_decode_submode(c, Submode::Turbo, "decode_js8_turbo");
    bench_decode_submode(c, Submode::Slow, "decode_js8_slow");
    bench_decode_submode(c, Submode::Ultra, "decode_js8_ultra");
}

criterion_group!(name = benches; config = Criterion::default().measurement_time(Duration::from_secs(30)); targets = bench_decode_all_submodes);
criterion_main!(benches);
