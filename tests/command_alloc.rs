// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
};

use js8rs::command::parse_command;

struct CountAlloc;

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;

#[test]
fn command_parser_does_not_allocate() {
    const CASES: &[(&str, Option<&str>)] = &[
        ("K2XYZ MSG HELLO", None),
        ("KN4CRD: K0OG MSG HELLO BRAVE SOUL", None),
        ("@ALLCALL QUERY MSGS?", None),
        ("N0JDS > OH8STN > KN4CRD MSG HELLO", None),
        (">OH8STN>KN4CRD MSG HELLO", Some("N0JDS")),
        ("CQ CQ CQ EM73", None),
        ("HELLO BRAVE NEW WORLD", None),
    ];

    for &(text, target) in CASES {
        black_box(parse_command(text, target));
    }

    ALLOCS.store(0, Ordering::Relaxed);
    for &(text, target) in black_box(CASES) {
        black_box(parse_command(black_box(text), black_box(target)));
    }
    let allocations = ALLOCS.load(Ordering::Relaxed);

    assert_eq!(allocations, 0);
}
