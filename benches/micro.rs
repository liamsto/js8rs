use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use js8rs::codec::{BuildFramesOptions, build_frames, parse_compound, parse_directed, parse_frame};
use js8rs::protocol::{DecodeModes, Js8Protocol, Submode, SubmodeLookup};
use js8rs::rx::{Decoder, Detector, InputFormat, SAMPLE_BUFFER_SIZE};
use js8rs::tx::{Channel, Modulator};
use std::{hint::black_box, time::Duration};

mod support;
use support::synth_encode_fixture;

const BLOCK_FRAMES: usize = 4096;

fn bench_decoder_init(c: &mut Criterion) {
    c.bench_function("decoder_init", |b| {
        b.iter_batched(
            || (),
            |()| black_box(Decoder::new()),
            BatchSize::PerIteration,
        );
    });
    c.bench_function("decoder_init_fast", |b| {
        b.iter_batched(
            || (),
            |()| black_box(Decoder::with_modes(DecodeModes::FAST)),
            BatchSize::PerIteration,
        );
    });
    c.bench_function("decoder_init_none", |b| {
        b.iter_batched(
            || (),
            |()| black_box(Decoder::with_modes(DecodeModes::NONE)),
            BatchSize::PerIteration,
        );
    });
    c.bench_function("detector_init", |b| {
        b.iter_batched(
            || (),
            |()| black_box(Detector::new(1, 1024)),
            BatchSize::PerIteration,
        );
    });
}

fn bench_detector_write(c: &mut Criterion) {
    let mono: Vec<i16> = (0..BLOCK_FRAMES)
        .map(|i| ((i * 251) as i16).wrapping_add(17))
        .collect();
    let stereo: Vec<i16> = mono.iter().flat_map(|&v| [v, v.wrapping_neg()]).collect();
    let mut group = c.benchmark_group("detector_write");
    group.throughput(Throughput::Elements(BLOCK_FRAMES as u64));

    for (name, samples, format) in [
        ("mono", mono.as_slice(), InputFormat::Mono),
        ("stereo_left", stereo.as_slice(), InputFormat::StereoLeft),
        (
            "stereo_average",
            stereo.as_slice(),
            InputFormat::StereoAverage,
        ),
    ] {
        group.bench_with_input(BenchmarkId::from_parameter(name), samples, |b, samples| {
            b.iter_batched(
                // A one-second period always aligns the detector cursor to zero.
                || Detector::new(1, 1024),
                |detector| {
                    let stats = detector.write_i16(black_box(samples), format);
                    black_box(stats);
                    detector
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

fn bench_detector_samples(c: &mut Criterion) {
    let detector = Detector::new(1, 1024);
    let samples = vec![1i16; BLOCK_FRAMES];
    black_box(detector.write_i16(&samples, InputFormat::Mono));

    c.bench_function("detector_samples/borrowed", |b| {
        b.iter(|| {
            detector.with_samples(|samples, kin| {
                black_box(samples);
                black_box(kin);
            });
        });
    });

    let mut copy = vec![0i16; SAMPLE_BUFFER_SIZE];
    let mut group = c.benchmark_group("detector_copy");
    group.throughput(Throughput::Bytes(
        (SAMPLE_BUFFER_SIZE * size_of::<i16>()) as u64,
    ));
    group.bench_function("reused_vec", |b| {
        b.iter(|| {
            let kin = detector.copy_samples(black_box(&mut copy));
            black_box(kin);
        });
    });
    group.finish();
}

fn bench_modulator_render(c: &mut Criterion) {
    let fixture = synth_encode_fixture(Submode::Fast, "HELLO WORLD");
    let tones = js8rs::codec::encode_tones(fixture.flags, fixture.submode, &fixture.frame12)
        .expect("encode_tones should work");
    let mut group = c.benchmark_group("modulator_render");
    group.throughput(Throughput::Elements(BLOCK_FRAMES as u64));
    group.bench_function("fast_stereo", |b| {
        b.iter_batched(
            || {
                let mut modulator = Modulator::new();
                modulator.start_tones(
                    &tones,
                    Submode::Fast,
                    Js8Protocol::start_delay_ms(Submode::Fast),
                    1500.0,
                    Duration::ZERO,
                    Channel::Mono,
                );
                (modulator, vec![0i16; BLOCK_FRAMES * 2])
            },
            |(mut modulator, mut output)| {
                let frames = modulator.render_stereo(black_box(&mut output));
                black_box(frames);
                (modulator, output)
            },
            BatchSize::PerIteration,
        );
    });
    group.finish();
}

fn bench_parse_frame(c: &mut Criterion) {
    let options = BuildFramesOptions::new("HELLO WORLD", Submode::Fast);
    let built = build_frames(&options);
    let frame = built
        .frames
        .first()
        .expect("build_frames should produce a frame");

    let directed = build_frames(
        &BuildFramesOptions::new("K2XYZ MSG HELLO", Submode::Fast).with_station("K1ABC", "EM73"),
    )
    .frames
    .into_iter()
    .find(|frame| parse_directed(&frame.encoded).is_some())
    .expect("fixture should contain a directed frame");

    let compound = build_frames(&BuildFramesOptions::new("`K1ABC EM73", Submode::Fast))
        .frames
        .into_iter()
        .find(|frame| parse_compound(&frame.encoded).is_some())
        .expect("fixture should contain a compound frame");

    c.bench_function("parse_frame_free_text", |b| {
        b.iter_batched(
            || (),
            |()| {
                parse_frame(
                    black_box(&frame.encoded),
                    black_box(frame.flags),
                    black_box(Submode::Fast),
                )
            },
            BatchSize::LargeInput,
        );
    });
    c.bench_function("parse_directed", |b| {
        b.iter_batched(
            || (),
            |()| {
                parse_directed(black_box(&directed.encoded)).expect("directed fixture should parse")
            },
            BatchSize::LargeInput,
        );
    });
    c.bench_function("parse_compound", |b| {
        b.iter_batched(
            || (),
            |()| {
                parse_compound(black_box(&compound.encoded)).expect("compound fixture should parse")
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    bench_decoder_init,
    bench_detector_write,
    bench_detector_samples,
    bench_modulator_render,
    bench_parse_frame,
);
criterion_main!(benches);
