// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2025 Allan Bazinet <w6baz@arrl.net>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 Costas arrays to compile-time Rust data.

/// Costas array selection type.
#[derive(Copy, Clone)]
pub enum Type {
    Original = 0,
    Modified = 1,
}

pub type Array = [[u8; 7]; 3];

pub const ORIGINAL: Array = [
    [4, 2, 5, 6, 1, 3, 0],
    [4, 2, 5, 6, 1, 3, 0],
    [4, 2, 5, 6, 1, 3, 0],
];

pub const MODIFIED: Array = [
    [0, 6, 2, 3, 5, 4, 1],
    [1, 5, 0, 2, 3, 6, 4],
    [2, 5, 0, 6, 4, 1, 3],
];

pub const fn for_type(t: Type) -> &'static Array {
    match t {
        Type::Original => &ORIGINAL,
        Type::Modified => &MODIFIED,
    }
}
#[test]
fn costas_normal_is_original() {
    let d = crate::submode::NORMAL;
    assert_eq!(
        d.costas as u8,
        crate::internal::costas::Type::Original as u8
    );
}
