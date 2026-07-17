use criterion::{Criterion, criterion_group, criterion_main};
use js8rs::codec::encode_tones;
use js8rs::protocol::Submode;
use std::hint::black_box;

mod support;
use support::synth_encode_fixture;

fn bench_encode_submode(c: &mut Criterion, submode: Submode, name: &str) {
    let fixture = synth_encode_fixture(submode, "HELLO WORLD");
    c.bench_function(name, |b| {
        b.iter(|| {
            let tones = encode_tones(
                black_box(fixture.flags),
                black_box(fixture.submode),
                black_box(&fixture.frame12),
            )
            .expect("encode_tones should work");
            black_box(tones);
        });
    });
}

fn bench_encode_all_submodes(c: &mut Criterion) {
    bench_encode_submode(c, Submode::Normal, "encode_tones_js8_normal");
    bench_encode_submode(c, Submode::Fast, "encode_tones_js8_fast");
    bench_encode_submode(c, Submode::Turbo, "encode_tones_js8_turbo");
    bench_encode_submode(c, Submode::Slow, "encode_tones_js8_slow");
    bench_encode_submode(c, Submode::Ultra, "encode_tones_js8_ultra");
}

criterion_group!(benches, bench_encode_all_submodes);
criterion_main!(benches);
