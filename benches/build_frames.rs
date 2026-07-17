use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use js8rs::codec::{BuildFramesOptions, build_frames};
use js8rs::protocol::Submode;
use std::hint::black_box;

fn bench_build_frames(c: &mut Criterion) {
    let short = BuildFramesOptions::new("HELLO WORLD", Submode::Fast);
    c.bench_function("build_frames_short_fast", |b| {
        b.iter_batched(
            || (),
            |()| build_frames(black_box(&short)),
            BatchSize::LargeInput,
        );
    });

    let long = BuildFramesOptions::new(
        "THIS IS A LONG BENCHMARKING MESSAGE TO TEST JS8RS FRAME PACKING",
        Submode::Normal,
    );
    c.bench_function("build_frames_long_normal", |b| {
        b.iter_batched(
            || (),
            |()| build_frames(black_box(&long)),
            BatchSize::LargeInput,
        );
    });

    let directed =
        BuildFramesOptions::new("K2XYZ MSG HELLO", Submode::Fast).with_station("K1ABC", "EM73");
    c.bench_function("build_frames_directed_fast", |b| {
        b.iter_batched(
            || (),
            |()| build_frames(black_box(&directed)),
            BatchSize::LargeInput,
        );
    });

    let compound = BuildFramesOptions::new("`K1ABC EM73", Submode::Fast);
    c.bench_function("build_frames_compound_fast", |b| {
        b.iter_batched(
            || (),
            |()| build_frames(black_box(&compound)),
            BatchSize::LargeInput,
        );
    });

    for (name, text) in [
        ("build_frames_cq_fast", "CQ CQ CQ EM73"),
        ("build_frames_hb_fast", "HB EM73"),
    ] {
        let options = BuildFramesOptions::new(text, Submode::Fast).with_station("K1ABC", "EM73");
        c.bench_function(name, |b| {
            b.iter_batched(
                || (),
                |()| build_frames(black_box(&options)),
                BatchSize::LargeInput,
            );
        });
    }

    let selected = BuildFramesOptions::new("HELLO", Submode::Fast)
        .with_station("K1ABC", "EM73")
        .with_selected_call("K2XYZ");
    c.bench_function("build_frames_selected_fast", |b| {
        b.iter_batched(
            || (),
            |()| build_frames(black_box(&selected)),
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(benches, bench_build_frames);
criterion_main!(benches);
