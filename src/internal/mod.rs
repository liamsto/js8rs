// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

pub(crate) mod commons;
pub(crate) mod consts;
pub(crate) mod costas;
pub(crate) mod crc12;
pub(crate) mod decoded_text;
pub(crate) mod kalman;
pub(crate) mod ldpc_feedback;
pub(crate) mod local_routines;
pub(crate) mod local_types;
pub(crate) mod soft_combiner;
pub(crate) mod whitening_processor;

use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// Full-range cosine approximation using symmetries of cos(x).
#[inline]
pub const fn cos_approx(mut x: f64) -> f64 {
    #[inline]
    const fn poly(x: f64) -> f64 {
        const C0: f64 = 1.0;
        const C1: f64 = -0.499_999_999_999_999_94;
        const C2: f64 = 0.041_666_666_666_666_664;
        const C3: f64 = -0.001_388_888_888_888_889;
        const C4: f64 = 0.000_024_801_587_301_587;
        const C5: f64 = -0.000_000_275_573_192_239_86;
        const C6: f64 = 0.000_000_002_087_675_698_786_81;
        const C7: f64 = -0.000_000_000_011_470_745_138_751_76;
        const C8: f64 = 0.000_000_000_000_047_794_773_323_873_3;

        let x2 = x * x;
        let x4 = x2 * x2;
        let x6 = x4 * x2;
        let x8 = x4 * x4;
        let x10 = x8 * x2;
        let x12 = x8 * x4;
        let x14 = x12 * x2;
        let x16 = x8 * x8;

        C0 + C1 * x2 + C2 * x4 + C3 * x6 + C4 * x8 + C5 * x10 + C6 * x12 + C7 * x14 + C8 * x16
    }

    let k = (x / TAU) as i64;
    x -= (k as f64) * TAU;

    if x > PI {
        x = TAU - x;
    }

    if x > FRAC_PI_2 {
        -poly(PI - x)
    } else {
        poly(x)
    }
}
