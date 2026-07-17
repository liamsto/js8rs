// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use js8rs::command::{CommandKind, parse_command};
use std::hint::black_box;

const CASES: &[(&str, Option<&str>)] = &[
    ("K2XYZ MSG HELLO", None),
    ("K2XYZ ACK", None),
    ("K2XYZ SNR -12", None),
    ("KN4CRD: K0OG MSG HELLO BRAVE SOUL", None),
    ("@ALLCALL QUERY MSGS?", None),
    ("@APRSIS MSG TO: K1ABC HELLO", None),
    ("K2XYZ QUERY CALL KN4CRD?", None),
    ("K2XYZ QUERY MSG 123", None),
    ("K2XYZ GRID FN42FN42", None),
    ("N0JDS > OH8STN > KN4CRD MSG HELLO", None),
    (">OH8STN>KN4CRD MSG HELLO", Some("N0JDS")),
    ("MSG HELLO", Some("K2XYZ")),
    ("CQ CQ CQ EM73", None),
    ("HB EM73", None),
    ("k2xyz msg a lowercase payload", None),
    (
        "K2XYZ MSG THIS IS A LONGER REALISTIC PAYLOAD WITH SEVERAL WORDS",
        None,
    ),
    ("K2XYZ QUERY MSG nope", None),
    ("K2XYZ STATUSX", None),
    ("HELLO BRAVE NEW WORLD", None),
    ("`K1ABC MSG", None),
    ("", None),
];

const TOKENS: &[&str] = &[
    " SNR?",
    " DIT DIT",
    " NACK",
    " HEARING?",
    " GRID?",
    ">",
    " STATUS?",
    " STATUS",
    " HEARING",
    " MSG",
    " MSG TO:",
    " QUERY",
    " QUERY MSGS",
    " QUERY CALL",
    " ACK",
    " GRID",
    " INFO?",
    " INFO",
    " FB",
    " HW CPY?",
    " SK",
    " RR",
    " QSL?",
    " QSL",
    " CMD",
    " SNR",
    " NO",
    " YES",
    " 73",
    " HEARTBEAT SNR",
    " AGN?",
    " ",
];

fn bench_command_parse(c: &mut Criterion) {
    let bytes = CASES.iter().map(|(text, _)| text.len()).sum::<usize>();
    let mut group = c.benchmark_group("command_parse");
    group.throughput(Throughput::Bytes(bytes as u64));
    group.bench_function("mixed", |b| {
        b.iter_batched(
            || (),
            |()| {
                for &(text, target) in black_box(CASES) {
                    black_box(parse_command(black_box(text), black_box(target)));
                }
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();

    let mut group = c.benchmark_group("command_parse_case");
    for &(name, text, target) in &[
        ("explicit_msg", "K2XYZ MSG HELLO", None),
        ("sender", "KN4CRD: K0OG MSG HELLO BRAVE SOUL", None),
        ("full_relay", "N0JDS > OH8STN > KN4CRD MSG HELLO", None),
        ("partial_relay", ">OH8STN>KN4CRD MSG HELLO", Some("N0JDS")),
        ("bare_cq", "CQ CQ CQ EM73", None),
        ("miss", "HELLO BRAVE NEW WORLD", None),
    ] {
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(BenchmarkId::new("case", name), &text, |b, text| {
            b.iter(|| black_box(parse_command(black_box(text), black_box(target))));
        });
    }
    group.finish();
}

fn bench_command_token(c: &mut Criterion) {
    let bytes = TOKENS.iter().map(|token| token.len()).sum::<usize>();
    let mut group = c.benchmark_group("command_token");
    group.throughput(Throughput::Bytes(bytes as u64));
    group.bench_function("mixed", |b| {
        b.iter(|| {
            for &token in black_box(TOKENS) {
                black_box(CommandKind::from_wire(black_box(token)));
            }
        });
    });
    group.finish();
}

criterion_group!(benches, bench_command_parse, bench_command_token);
criterion_main!(benches);
