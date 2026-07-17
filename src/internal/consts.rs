use crate::internal::{
    self,
    commons::{
        JS8_RX_SAMPLE_RATE, JS8A_SYMBOL_SAMPLES, JS8A_TX_SECONDS, JS8B_SYMBOL_SAMPLES,
        JS8B_TX_SECONDS, JS8C_SYMBOL_SAMPLES, JS8C_TX_SECONDS, JS8E_SYMBOL_SAMPLES,
        JS8E_TX_SECONDS, JS8I_SYMBOL_SAMPLES, JS8I_TX_SECONDS,
    },
    cos_approx,
};
use crate::protocol::Submode;
use std::f64::consts::PI;

/// Total bit count.
pub const N: usize = 174;
/// Message bit count.
pub const K: usize = 87;
/// Check bit count.
pub const M: usize = N - K;
/// Data symbol count.
pub const ND: usize = 58;
/// Sync symbol count, 3 @ Costas 7x7.
pub const NS: usize = 21;
/// Total channel symbol count (79).
pub const NN: usize = NS + ND;

/// Minimum sync.
pub const ASYNCMIN: f32 = 1.5_f32;
/// Search frequency range in Hz (i.e. +/- 2.5 Hz).
pub const NFSRCH: i32 = 5;
/// Maxiumum number of candidate signals.
pub const NMAXCAND: usize = 300;
/// Filter length.
pub const NFILT: i32 = 1400;
pub const NROWS: usize = 8;
pub const NFOS: usize = 2;
pub const NSSY: i32 = 4;
pub const NP: usize = 3200;
pub const NP2: usize = 2812;

// Tunable settings, the degree of the polynomial used for the baseline curve fit
// and the percentile of the span at which to sample.
pub const BASELINE_DEGREE: usize = 5;
pub const BASELINE_SAMPLE: i32 = 10;

// Closed range in Hz for baseline determination.
pub const BASELINE_MIN: i32 = 500;
pub const BASELINE_MAX: i32 = 2500;

// Compile-time constraints
const _: () = {
    assert!((BASELINE_DEGREE & 1) != 0, "Degree must be odd");
    assert!(
        0 <= BASELINE_SAMPLE && BASELINE_SAMPLE <= 100,
        "Sample must be a percentage"
    );
};

// Chebyshev nodes over [0, 1], computed at compile time, for scaling at runtime.
const fn make_baseline_nodes() -> [f64; BASELINE_DEGREE + 1] {
    let mut nodes = [0.0_f64; BASELINE_DEGREE + 1];
    let len = nodes.len() as f64;
    let slice = PI / (2.0_f64 * len);

    let mut i: usize = 0;
    while i < nodes.len() {
        let t = slice * (2.0_f64 * (i as f64) + 1.0_f64);
        nodes[i] = 0.5_f64 * (1.0_f64 - cos_approx(t));
        i += 1;
    }

    nodes
}

pub const BASELINE_NODES: [f64; BASELINE_DEGREE + 1] = make_baseline_nodes();
pub const BASELINE_COUNT: usize = BASELINE_DEGREE + 1;

pub trait ModeSpec {
    // Base values (vary by mode)
    const NSUBMODE: Submode;
    const NCOSTAS: internal::costas::Type;
    const NSPS: usize;
    const NTXDUR: usize;
    const NDOWNSPS: usize;
    const NDD: usize;
    const JZ: i32;
    const ASTART: f32;
    const BASESUB: f32;
    // Multipliers differ, base const
    const AZMUL: f32;
    // Derived values (shared formulas, but values change by mode)
    const AZ: f32 = (12000.0_f32 / (Self::NSPS as f32)) * Self::AZMUL;
    const NMAX: usize = Self::NTXDUR * JS8_RX_SAMPLE_RATE as usize;
    const NFFT1: usize = Self::NSPS * NFOS;
    const NSTEP: usize = Self::NSPS / NSSY as usize;
    const NHSYM: usize = Self::NMAX / Self::NSTEP - 3;
    const NDOWN: usize = Self::NSPS / Self::NDOWNSPS;
    const NQSYMBOL: i32 = (Self::NDOWNSPS / 4) as i32;
    const NDFFT1: usize = Self::NSPS * Self::NDD;
    const NDFFT2: usize = Self::NDFFT1 / Self::NDOWN;
    const NP2: usize = NN * Self::NDOWNSPS;
    const TSTEP: f32 = (Self::NSTEP as f32) / 12000.0_f32;
    const JSTRT: i32 = (Self::ASTART / Self::TSTEP) as i32; // trunc toward zero
    const DF: f32 = 12000.0_f32 / (Self::NFFT1 as f32);
}

macro_rules! define_mode {
    ($name:ident {
        NSUBMODE: $nsub:expr,
        NCOSTAS:  $ncostas:expr,
        NSPS:     $nsps:expr,
        NTXDUR:   $ntxdur:expr,
        NDOWNSPS: $ndownsps:expr,
        NDD:      $ndd:expr,
        JZ:       $jz:expr,
        ASTART:   $astart:expr,
        BASESUB:  $basesub:expr,
        AZMUL:    $azmul:expr $(,)?
    }) => {
        pub struct $name;

        impl ModeSpec for $name {
            const NSUBMODE: Submode = $nsub;
            const NCOSTAS: internal::costas::Type = $ncostas;
            const NSPS: usize = $nsps;
            const NTXDUR: usize = $ntxdur;
            const NDOWNSPS: usize = $ndownsps;
            const NDD: usize = $ndd;
            const JZ: i32 = $jz;
            const ASTART: f32 = $astart;
            const BASESUB: f32 = $basesub;
            const AZMUL: f32 = $azmul;
        }
    };
}

define_mode!(Normal {
    NSUBMODE: Submode::Normal,
    NCOSTAS: internal::costas::Type::Original,
    NSPS: JS8A_SYMBOL_SAMPLES as usize,
    NTXDUR: JS8A_TX_SECONDS as usize,
    NDOWNSPS: 32,
    NDD: 100,
    JZ: 62,
    ASTART: 0.5_f32,
    BASESUB: 40.0_f32,
    AZMUL: 0.64_f32,
});

define_mode!(Fast {
    NSUBMODE: Submode::Fast,
    NCOSTAS: internal::costas::Type::Modified,
    NSPS: JS8B_SYMBOL_SAMPLES as usize,
    NTXDUR: JS8B_TX_SECONDS as usize,
    NDOWNSPS: 20,
    NDD: 100,
    JZ: 144,
    ASTART: 0.2_f32,
    BASESUB: 39.0_f32,
    AZMUL: 0.8_f32,
});

define_mode!(Turbo {
    NSUBMODE: Submode::Turbo,
    NCOSTAS: internal::costas::Type::Modified,
    NSPS: JS8C_SYMBOL_SAMPLES as usize,
    NTXDUR: JS8C_TX_SECONDS as usize,
    NDOWNSPS: 12,
    NDD: 120,
    JZ: 172,
    ASTART: 0.1_f32,
    BASESUB: 38.0_f32,
    AZMUL: 0.6_f32,
});

define_mode!(Slow {
    NSUBMODE: Submode::Slow,
    NCOSTAS: internal::costas::Type::Modified,
    NSPS: JS8E_SYMBOL_SAMPLES as usize,
    NTXDUR: JS8E_TX_SECONDS as usize,
    NDOWNSPS: 32,
    NDD: 94,
    JZ: 32,
    ASTART: 0.5_f32,
    BASESUB: 42.0_f32,
    AZMUL: 0.64_f32,
});

define_mode!(Ultra {
    NSUBMODE: Submode::Ultra,
    NCOSTAS: internal::costas::Type::Modified,
    NSPS: JS8I_SYMBOL_SAMPLES as usize,
    NTXDUR: JS8I_TX_SECONDS as usize,
    NDOWNSPS: 12,
    NDD: 125,
    JZ: 250,
    ASTART: 0.1_f32,
    BASESUB: 36.0_f32,
    AZMUL: 0.64_f32,
});
