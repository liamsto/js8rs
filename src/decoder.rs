// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2025 Allan Bazinet <w6baz@arrl.net>

use crate::encoder::encode_with_costas;
use crate::internal::commons::DecData;
use crate::internal::consts::{
    ASYNCMIN, BASELINE_COUNT, BASELINE_MAX, BASELINE_MIN, BASELINE_NODES, BASELINE_SAMPLE, Fast,
    ModeSpec, NFILT, NFOS, NMAXCAND, NSSY, Normal, Slow, Turbo, Ultra,
};
use crate::internal::consts::{K, N, ND, NFSRCH, NN, NP, NP2, NROWS};
use crate::internal::kalman::{FrequencyTracker, TimingTracker};
use crate::internal::ldpc_feedback::{
    LDPC_FEEDBACK_MAX_PASSES_DEFAULT, LLR_ERASURE_THRESHOLD_DEFAULT, LLR_SCALE_DEFAULT,
    refine_llrs_with_ldpc_feedback,
};
use crate::internal::local_routines::{check_crc12, extract_message_174};
use crate::internal::local_types::{
    Decode, DecodeMap, FftPlanManager, FftPlanType, KahanSum,
    belief_decoder::{BpOpts, BpRes, bpdecode174},
};
use crate::internal::soft_combiner::{SoftCombined, SoftCombiner};
use crate::internal::whitening_processor::WhiteningProcessor;
use crate::internal::{self, aided, coherent, sic};
use crate::protocol::{DecodeModes, FrameFlags};
use crate::rx::{
    DecodeFinished, DecodeStarted, Decoded, Event, SyncMetric, SyncStart, SyncState, SyncStateType,
};
use internal::local_types::Sync;
use libm::{atanf, cosf, fmodf, log10f, powf, roundf, sqrtf};
use num_complex::Complex32;
use std::cmp::Ordering;
use std::cmp::{max, min};
use std::collections::HashMap;
use std::f32::consts::TAU;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::time::Duration;

#[repr(align(64))]
pub struct Align64<T>(pub T);

impl<T> Align64<T> {
    #[inline]
    const fn get(&self) -> &T {
        &self.0
    }
    #[inline]
    const fn get_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T> Deref for Align64<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Align64<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

const CZERO: Complex32 = Complex32 { re: 0.0, im: 0.0 };

#[derive(Clone, Copy)]
struct BpSnap {
    llr: [f32; N],
    res: BpRes,
    cw: [i8; N],
    dec: [i8; K],
}

struct BpTry {
    dec: [i8; K],
    cw: [i8; N],
    near_llr: [f32; N],
    near_cw: [i8; N],
    checks: usize,
    pass: u8,
    hard: i32,
    cache: [Option<BpSnap>; 2],
}

impl BpTry {
    fn new() -> Self {
        Self {
            dec: [0; K],
            cw: [0; N],
            near_llr: [0.0; N],
            near_cw: [0; N],
            checks: K + 1,
            pass: 0,
            hard: -1,
            cache: [None; 2],
        }
    }

    fn run(&mut self, llr: &[f32; N], pass: u8, sync: f32, opts: BpOpts, remember: bool) -> bool {
        let cacheable = opts.max_iter == 30 && opts.early_abort && opts.llr_scale == 1.0;
        let cached = cacheable
            .then(|| {
                self.cache.iter().flatten().find(|snap| {
                    snap.llr
                        .iter()
                        .zip(llr)
                        .all(|(a, b)| a.to_bits() == b.to_bits())
                })
            })
            .flatten();
        let bp = if let Some(snap) = cached {
            self.cw = snap.cw;
            if snap.res.hard_err >= 0 {
                self.dec = snap.dec;
            }
            snap.res
        } else {
            let bp = bpdecode174(llr, &mut self.dec, &mut self.cw, opts);
            if cacheable
                && pass == 1
                && remember
                && llr.iter().all(|v| v.is_finite())
                && let Some(slot) = self.cache.iter_mut().find(|slot| slot.is_none())
            {
                *slot = Some(BpSnap {
                    llr: *llr,
                    res: bp,
                    cw: self.cw,
                    dec: self.dec,
                });
            }
            bp
        };
        if remember && bp.best_checks < self.checks && bp.best_cw_valid {
            self.checks = bp.best_checks;
            self.pass = pass;
            self.near_llr = *llr;
            self.near_cw = bp.best_cw;
        }
        self.hard = bp.hard_err;
        self.cw.iter().any(|&b| b != 0)
            && (0..60).contains(&self.hard)
            && !(sync < 2.0 && self.hard > 35)
            && !(pass > 2 && self.hard > 39)
            && !(pass == 4 && self.hard > 30)
            && check_crc12(&self.dec)
    }
}

/// Decoder state specialized by Mode.
pub struct DecodeMode<
    Mode: ModeSpec,
    const NFFT1: usize,
    const NDOWNSPS: usize,
    const NMAX: usize,
    const NDFFT1: usize,
    const NDFFT1_CX: usize,
    const NDDP1: usize,
    const NSPS: usize,
    const NHSYM: usize,
> {
    _mode: PhantomData<Mode>,
    nuttal: [f32; NFFT1],
    csyncs: [[[Complex32; NDOWNSPS]; 7]; 3],
    csymb: Align64<[Complex32; NDOWNSPS]>,
    filter: Vec<Complex32>,
    cfilt: Vec<Complex32>,
    ds_time: Vec<Complex32>,
    sd_time: Vec<Complex32>,
    ds_cx: Vec<Complex32>,
    cd0: Align64<[Complex32; NP]>,
    dd: Vec<f32>,
    s: Vec<[f32; NHSYM]>,
    #[cfg(feature = "experimental-time")]
    spectrum_sum: [f32; NSPS],
    savg: [f32; NSPS],
    plans: FftPlanManager,
    m_soft_combiner: SoftCombiner<N>,
    legacy_combiner: SoftCombiner<N>,
    sic_alt: Option<Vec<f32>>,
    m_llr_erasure_threshold: f32,
    m_enable_ldpc_feedback: bool,
    m_max_ldpc_passes: u8,
    baseline_p: [[f64; 2]; BASELINE_COUNT],
    baseline_c: [f64; BASELINE_COUNT],
    taper: [[f32; NDDP1]; 2],
    #[cfg(feature = "experimental-time")]
    spectrum_start: Option<u64>,
    #[cfg(feature = "experimental-time")]
    spectrum_updates: u8,
}

macro_rules! define_decode_mode {
    ($alias:ident, $mode:ty) => {
        pub type $alias = DecodeMode<
            $mode,
            { <$mode as ModeSpec>::NFFT1 },
            { <$mode as ModeSpec>::NDOWNSPS },
            { <$mode as ModeSpec>::NMAX },
            { <$mode as ModeSpec>::NDFFT1 },
            { <$mode as ModeSpec>::NDFFT1 / 2 + 1 },
            { <$mode as ModeSpec>::NDD + 1 },
            { <$mode as ModeSpec>::NSPS },
            { <$mode as ModeSpec>::NHSYM },
        >;
    };
}

define_decode_mode!(DecodeNormal, Normal);
define_decode_mode!(DecodeFast, Fast);
define_decode_mode!(DecodeTurbo, Turbo);
define_decode_mode!(DecodeSlow, Slow);
define_decode_mode!(DecodeUltra, Ultra);

impl<
    Mode: ModeSpec,
    const NFFT1: usize,
    const NDOWNSPS: usize,
    const NMAX: usize,
    const NDFFT1: usize,
    const NDFFT1_CX: usize,
    const NDDP1: usize,
    const NSPS: usize,
    const NHSYM: usize,
> DecodeMode<Mode, NFFT1, NDOWNSPS, NMAX, NDFFT1, NDFFT1_CX, NDDP1, NSPS, NHSYM>
{
    #[inline]
    fn zeroed() -> Self {
        Self {
            _mode: PhantomData,
            nuttal: [0.0; NFFT1],
            csyncs: [[[CZERO; NDOWNSPS]; 7]; 3],
            csymb: Align64([CZERO; NDOWNSPS]),
            filter: vec![CZERO; Mode::NMAX],
            cfilt: vec![CZERO; Mode::NMAX],
            ds_time: vec![CZERO; NDFFT1],
            sd_time: vec![CZERO; NFFT1],
            ds_cx: vec![CZERO; NDFFT1_CX],
            cd0: Align64([CZERO; NP]),
            dd: vec![0.0; Mode::NMAX],
            s: vec![[0.0; NHSYM]; NSPS],
            #[cfg(feature = "experimental-time")]
            spectrum_sum: [0.0; NSPS],
            savg: [0.0; NSPS],
            plans: FftPlanManager::new(),
            m_soft_combiner: SoftCombiner::new(),
            legacy_combiner: SoftCombiner::new(),
            sic_alt: None,
            m_llr_erasure_threshold: LLR_ERASURE_THRESHOLD_DEFAULT,
            m_enable_ldpc_feedback: true,
            m_max_ldpc_passes: LDPC_FEEDBACK_MAX_PASSES_DEFAULT,
            baseline_p: [[0.0; 2]; BASELINE_COUNT],
            baseline_c: [0.0; BASELINE_COUNT],
            taper: [[0.0; NDDP1]; 2],
            #[cfg(feature = "experimental-time")]
            spectrum_start: None,
            #[cfg(feature = "experimental-time")]
            spectrum_updates: 0,
        }
    }

    const fn costas() -> &'static internal::costas::Array {
        internal::costas::for_type(Mode::NCOSTAS)
    }

    pub(super) fn js8dec<E>(
        &mut self,
        sync_stats: bool,
        lsubtract: bool,
        f1_hz: &mut f32,
        xdt: &mut f32,
        nharderrors: &mut i32,
        xsnr: &mut f32,
        rescue: &mut u8,
        deep: bool,
        retry: bool,
        mut emit_event: E,
    ) -> Option<Decode>
    where
        E: FnMut(Event),
    {
        let costas = Self::costas();

        let fr: f32 = 12000.0 / (Mode::NFFT1 as f32);
        let fs2: f32 = 12000.0 / (Mode::NDOWN as f32);
        let dt2: f32 = 1.0 / fs2;

        let index = (*f1_hz / fr).round() as i32;
        let index_u = index.clamp(0, (Mode::NSPS as i32) - 1) as usize;

        let scaled_value = 0.1 * (self.savg[index_u] - Mode::BASESUB);
        let xbase = libm::powf(10.0, scaled_value);

        let mut delfbest: f32 = 0.0;
        let mut ibest: i32 = 0;

        self.js8_downsample(*f1_hz);

        let mut i0 = ((*xdt + Mode::ASTART) * fs2).round() as i32;
        let mut smax: f32 = 0.0;

        for idt in (i0 - Mode::NQSYMBOL)..=(i0 + Mode::NQSYMBOL) {
            let s = self.syncjs8d(idt, 0.0);
            if s > smax {
                smax = s;
                ibest = idt;
            }
        }

        let xdt2 = (ibest as f32) * dt2;

        i0 = (xdt2 * fs2).round() as i32;
        smax = 0.0;

        for ifr in (-NFSRCH)..=NFSRCH {
            let delf = (ifr as f32) * 0.5;
            let s = self.syncjs8d(i0, delf);
            if s > smax {
                smax = s;
                delfbest = delf;
            }
        }

        let dphi = -delfbest * (TAU / fs2);
        let wstep = Complex32::from_polar(1.0, dphi);
        let mut w = Complex32 { re: 1.0, im: 0.0 };
        {
            let cd0 = self.cd0.get_mut();
            for sample in cd0.iter_mut().take(NP2) {
                w *= wstep;
                *sample *= w;
            }
        }

        *xdt = xdt2;
        *f1_hz += delfbest;

        let sync = self.syncjs8d(i0, 0.0);

        let mut s2 = [[0.0f32; NN]; NROWS];
        let mut bins = [[CZERO; NN]; NROWS];
        let mut starts = [0i32; NN];
        let mut shifts = [0i32; NN];
        let mut hz = [0.0f32; NN];

        let mut freq_tracker = FrequencyTracker::new();
        freq_tracker.reset_default(0.0, f64::from(fs2));

        let timing_max_shift = (0.08f64 * (Mode::NDOWNSPS as f64)).clamp(0.5, 2.0);

        let mut timing_tracker = TimingTracker::new();
        timing_tracker.reset(0.0, 0.15, 0.35, timing_max_shift);

        let estimate_residual_hz =
            |csymb: &[Complex32; NDOWNSPS], expected_tone: usize| -> Option<f32> {
                if expected_tone + 1 >= Mode::NDOWNSPS {
                    return None;
                }

                let m0 = csymb[expected_tone].norm_sqr();
                let mplus = csymb[expected_tone + 1].norm_sqr();
                let mminus = if expected_tone > 0 {
                    csymb[expected_tone - 1].norm_sqr()
                } else {
                    0.0
                };

                if m0 <= 0.0 {
                    return None;
                }

                let ratio = m0 / (mplus + mminus + 1e-12);
                if ratio < 1.5 {
                    return None;
                }

                let denom = 2.0f32.mul_add(-m0, mminus) + mplus;
                if denom.abs() < 1e-9 {
                    return None;
                }

                let mut delta: f32 = 0.5 * (mminus - mplus) / denom;
                delta = delta.clamp(-0.5, 0.5);

                Some(delta * (fs2 / (Mode::NDOWNSPS as f32)))
            };

        let goertzel_energy = |cd0: &[Complex32; NP],
                               start: i32,
                               expected_tone: usize,
                               freq_tracker: &mut FrequencyTracker|
         -> Option<f32> {
            if start < 0 {
                return None;
            }
            let start = start as usize;
            if start + Mode::NDOWNSPS > NP2 {
                return None;
            }

            let mut tmp = [CZERO; NDOWNSPS];
            tmp.copy_from_slice(&cd0[start..start + Mode::NDOWNSPS]);

            freq_tracker.apply(&mut tmp);

            let goertzel_wstep =
                Complex32::from_polar(1.0, -TAU * (expected_tone as f32) / (Mode::NDOWNSPS as f32));

            let mut phase = Complex32 { re: 1.0, im: 0.0 };
            let mut acc = Complex32 { re: 0.0, im: 0.0 };

            for sample in &tmp {
                acc += *sample * phase.conj();
                phase *= goertzel_wstep;
            }

            Some(acc.norm_sqr())
        };

        let fft = self
            .plans
            .get_or_create(FftPlanType::CS, Mode::NDOWNSPS, false);
        let mut k = 0usize;
        while k < NN {
            let i1_base = ibest + (k as i32) * (Mode::NDOWNSPS as i32);
            let timing_shift = timing_tracker.current_samples().round() as i32;
            let mut i1 = i1_base + timing_shift;

            let max_start = (NP2 - Mode::NDOWNSPS) as i32;
            if i1 < 0 {
                i1 = 0;
            } else if i1 > max_start {
                i1 = max_start;
            }

            {
                let start = i1 as usize;
                let cd0 = self.cd0.get();
                let csymb = self.csymb.get_mut();
                csymb.copy_from_slice(&cd0[start..start + Mode::NDOWNSPS]);

                freq_tracker.apply(csymb);

                fft.process(csymb);

                for i in 0..NROWS {
                    let mag = libm::sqrtf(csymb[i].norm_sqr());
                    s2[i][k] = mag / 1000.0;
                    bins[i][k] = csymb[i] / 1000.0;
                }
                starts[k] = i1_base;
                shifts[k] = i1 - i1_base;
                hz[k] = freq_tracker.current_hz() as f32;

                let is_pilot = (k < 7) || (36..43).contains(&k) || (72..79).contains(&k);
                if is_pilot {
                    let (costas_block, costas_col) = if (72..79).contains(&k) {
                        (2usize, k - 72)
                    } else if (36..43).contains(&k) {
                        (1usize, k - 36)
                    } else {
                        (0usize, k)
                    };

                    let expected_tone = costas[costas_block][costas_col];

                    if let Some(residual) = estimate_residual_hz(csymb, expected_tone as usize) {
                        freq_tracker.update(f64::from(residual), 1.0);
                    }

                    let cd0 = self.cd0.get();
                    let e0 = goertzel_energy(cd0, i1, expected_tone as usize, &mut freq_tracker);
                    let e_early =
                        goertzel_energy(cd0, i1 - 1, expected_tone as usize, &mut freq_tracker);
                    let e_late =
                        goertzel_energy(cd0, i1 + 1, expected_tone as usize, &mut freq_tracker);

                    let tone_mag = s2[expected_tone as usize][k];

                    if let (Some(e0), Some(e_early), Some(e_late)) = (e0, e_early, e_late)
                        && tone_mag > 1e-6
                    {
                        let denom = e0 + 1e-6;
                        let grad = (e_late - e_early) / denom;

                        let weight = f64::from(tone_mag / 5.0).clamp(0.0, 1.0);
                        let error_samples = (0.25 * f64::from(grad)).clamp(-1.0, 1.0);

                        timing_tracker.update(error_samples, weight);
                    }
                }
            }
            k += 1;
        }

        let mut nsync: u32 = 0;
        let mut expected = 0.0f64;
        let mut other = 0.0f64;
        for (block, costas_row) in costas.iter().enumerate() {
            let offset = block * 36;
            for (col, costas_tone) in costas_row.iter().enumerate().take(7usize) {
                let idx = offset + col;

                let mut best_row = 0usize;
                let mut best_val = s2[0][idx];
                for (row, s2_row) in s2.iter().enumerate().take(NROWS).skip(1) {
                    let v = s2_row[idx];
                    if v > best_val {
                        best_val = v;
                        best_row = row;
                    }
                }

                for (row, mags) in s2.iter().enumerate() {
                    let power = f64::from(mags[idx]).powi(2);
                    if row == usize::from(*costas_tone) {
                        expected += power;
                    } else {
                        other += power;
                    }
                }
                if *costas_tone == best_row as u8 {
                    nsync += 1;
                }
            }
        }

        let soft = expected / (other / 7.0 + 1e-12);
        if nsync <= 6 && (!deep || !soft.is_finite() || soft < 1.5) {
            return None;
        }

        if sync_stats {
            emit_event(Event::SyncState(SyncState {
                kind: SyncStateType::Candidate,
                submode: Mode::NSUBMODE,
                frequency_hz: *f1_hz,
                time_offset_seconds: *xdt,
                metric: SyncMetric::Candidate(nsync),
            }));
        }

        let (llr0, llr1) = self.tone_llrs(&s2, &bins, &starts, &shifts, &hz);
        let ttl = Duration::from_secs((Mode::NTXDUR * 2) as u64);
        self.m_soft_combiner.flush(ttl);
        self.legacy_combiner.flush(ttl);
        let key = self
            .m_soft_combiner
            .make_key(Mode::NSUBMODE, *f1_hz, *xdt, &llr0, &llr1);
        let combined = if retry {
            SoftCombined { key, llr0, llr1 }
        } else {
            self.m_soft_combiner.combine(key, &llr0, &llr1, ttl)
        };
        let mut llr0 = combined.llr0;
        let llr1 = combined.llr1;
        let mut bp = BpTry::new();
        let opts = BpOpts::default();
        let mut passes = 0;
        let mut ok = false;
        let mut legacy_key = None;
        for pass in 1..=4 {
            if passes >= self.m_max_ldpc_passes {
                break;
            }
            if pass == 3 {
                llr0[..24].fill(0.0);
            }
            if pass == 4 {
                llr0[24..48].fill(0.0);
            }
            let llr = if pass == 2 { &llr1 } else { &llr0 };
            if bp.run(llr, pass, sync, opts, true) {
                ok = true;
                break;
            }
            passes += 1;
            if self.m_enable_ldpc_feedback && passes < self.m_max_ldpc_passes {
                let mut refined = [0.0; N];
                let (mut confident, mut uncertain) = (0, 0);
                refine_llrs_with_ldpc_feedback(
                    llr,
                    &bp.cw,
                    self.m_llr_erasure_threshold,
                    &mut refined,
                    &mut confident,
                    &mut uncertain,
                );
                if bp.run(&refined, pass, sync, opts, true) {
                    ok = true;
                    break;
                }
                passes += 1;
            }
        }
        if !ok && !deep {
            let (s1, winners) = Self::tone_mags(&s2);
            let (lin, log) = WhiteningProcessor::fallback::<NROWS, ND, N>(&s1, &winners);
            let key = self
                .legacy_combiner
                .make_key(Mode::NSUBMODE, *f1_hz, *xdt, &lin, &log);
            let old = if retry {
                SoftCombined {
                    key,
                    llr0: lin,
                    llr1: log,
                }
            } else {
                self.legacy_combiner.combine(key, &lin, &log, ttl)
            };
            legacy_key = Some(old.key);
            let mut lin = old.llr0;
            let log = old.llr1;
            for pass in 1..=4 {
                if pass == 3 {
                    lin[..24].fill(0.0);
                }
                if pass == 4 {
                    lin[24..48].fill(0.0);
                }
                let llr = if pass == 2 { &log } else { &lin };
                if bp.run(llr, pass, sync, opts, false) {
                    ok = true;
                    break;
                }
                let mut refined = [0.0; N];
                let (mut confident, mut uncertain) = (0, 0);
                refine_llrs_with_ldpc_feedback(
                    llr,
                    &bp.cw,
                    0.25,
                    &mut refined,
                    &mut confident,
                    &mut uncertain,
                );
                if bp.run(&refined, pass, sync, opts, false) {
                    ok = true;
                    break;
                }
            }
        }
        if !ok && *rescue > 0 && bp.pass > 0 && bp.checks <= 12 {
            *rescue -= 1;
            for scale in [1.0, 0.8, 1.25] {
                let llr = bp.near_llr;
                let opts = BpOpts {
                    max_iter: 80,
                    early_abort: false,
                    llr_scale: scale,
                };
                if bp.run(&llr, bp.pass, sync, opts, true) {
                    ok = true;
                    break;
                }
            }
        }
        if !ok
            && (1..=aided::MAX_SYND).contains(&bp.checks)
            && bp.near_cw.iter().any(|&b| b != 0)
            && bp.near_llr.iter().all(|v| v.is_finite())
        {
            let bases = std::array::from_fn(|k| aided::Baseline {
                start: (starts[k] + shifts[k]) as isize,
                hz: hz[k],
            });
            let tones = aided::tones(&bp.near_cw, costas);
            let weights = aided::weights(&bp.near_llr);
            let refined = aided::refine(
                &self.cd0[..NP2],
                &bases,
                NDOWNSPS,
                f64::from(fs2),
                &tones,
                &weights,
            );
            if refined.accepted() {
                let hyp = refined.best;
                let mut mags = [[0.0; NN]; NROWS];
                let mut cx = [[CZERO; NN]; NROWS];
                let mut adj = shifts;
                for k in 0..NN {
                    let start = (bases[k].start + hyp.dt) as usize;
                    adj[k] = start as i32 - starts[k];
                    self.csymb
                        .copy_from_slice(&self.cd0[start..start + NDOWNSPS]);
                    if !aided::replay(&mut self.csymb[..], f64::from(hz[k]), f64::from(fs2)) {
                        self.csymb.fill(CZERO);
                    }
                    for (n, v) in self.csymb.iter_mut().enumerate() {
                        let t = (start + n) as f64 / f64::from(fs2);
                        let phi = std::f64::consts::TAU * hyp.df * t
                            + std::f64::consts::PI * hyp.drift * t * t;
                        *v *= Complex32::from_polar(1.0, -phi as f32);
                    }
                    fft.process(&mut self.csymb[..]);
                    for tone in 0..NROWS {
                        cx[tone][k] = self.csymb[tone] / 1000.0;
                        mags[tone][k] = self.csymb[tone].norm() / 1000.0;
                    }
                }
                let (llr, _) = self.tone_llrs(&mags, &cx, &starts, &adj, &hz);
                // don't combine samples twice
                ok = bp.run(&llr, 1, sync, BpOpts::default(), false);
                if !ok && *rescue > 0 {
                    *rescue -= 1;
                    for scale in [1.0, 0.8, 1.25] {
                        let opts = BpOpts {
                            max_iter: 80,
                            early_abort: false,
                            llr_scale: scale,
                        };
                        if bp.run(&llr, 1, sync, opts, false) {
                            ok = true;
                            break;
                        }
                    }
                }
                if ok {
                    let weights =
                        std::array::from_fn(|k| if aided::is_pilot(k) { 1.0 } else { 0.0 });
                    if let Some(resid) = aided::mid_resid(
                        hyp.df,
                        hyp.drift,
                        &bases,
                        NDOWNSPS,
                        f64::from(fs2),
                        &weights,
                    ) {
                        *f1_hz += resid as f32;
                        *xdt = xdt2 + hyp.dt as f32 * dt2;
                    }
                    s2 = mags;
                }
            }
        }
        if !ok {
            *nharderrors = -1;
            *xsnr = -99.0;
            return None;
        }
        *nharderrors = bp.hard;
        if sync_stats {
            emit_event(Event::SyncState(SyncState {
                kind: SyncStateType::Decoded,
                submode: Mode::NSUBMODE,
                frequency_hz: *f1_hz,
                time_offset_seconds: *xdt,
                metric: SyncMetric::Decoded(sync),
            }));
        }
        let message = extract_message_174(&bp.dec);
        let flags = ((bp.dec[72] << 2) | (bp.dec[73] << 1) | bp.dec[74]) as u8;
        let mut tones = [0; NN];
        encode_with_costas(flags, costas, message.as_bytes(), &mut tones).unwrap();
        if lsubtract {
            let raw = self.genjs8refsig(&tones, *f1_hz);
            let sig = sic::refine(raw, &self.dd, &tones, Mode::NSPS, *xdt);
            self.subtractjs8(&sig, *xdt);
        }
        let xsig: f32 = tones
            .iter()
            .enumerate()
            .map(|(k, &tone)| s2[usize::from(tone)][k].powi(2))
            .sum();
        *xsnr = (10.0 * (xsig / xbase - 1.0).max(1.259e-10).log10() - 32.0).max(-60.0);
        self.m_soft_combiner.mark_decoded(combined.key);
        if let Some(key) = legacy_key {
            self.legacy_combiner.mark_decoded(key);
        }
        Some(Decode {
            type_id: flags,
            data: message.as_bytes().try_into().unwrap(),
        })
    }

    fn tone_mags(mags: &[[f32; NN]; NROWS]) -> ([[f32; ND]; NROWS], [usize; ND]) {
        let s1 =
            std::array::from_fn(|tone| std::array::from_fn(|j| mags[tone][aided::data_idx(j)]));
        let winners = std::array::from_fn(|j| {
            let mut best = 0;
            for tone in 1..NROWS {
                if s1[tone][j] > s1[best][j] {
                    best = tone;
                }
            }
            best
        });
        (s1, winners)
    }

    fn tone_llrs(
        &self,
        mags: &[[f32; NN]; NROWS],
        bins: &[[Complex32; NN]; NROWS],
        starts: &[i32; NN],
        shifts: &[i32; NN],
        hz: &[f32; NN],
    ) -> ([f32; N], [f32; N]) {
        let (s1, winners) = Self::tone_mags(mags);
        let fs = 12000.0 / Mode::NDOWN as f64;
        let mut pilots = Vec::with_capacity(21);
        let mut count = [0; 3];
        for (block, row) in Self::costas().iter().enumerate() {
            for (col, &tone) in row.iter().enumerate() {
                let k = block * 36 + col;
                let val = bins[usize::from(tone)][k];
                if val.norm().is_finite() && val.norm() > 0.0 {
                    count[block] += 1;
                    pilots.push(coherent::Pilot {
                        time: f64::from(starts[k]) / fs,
                        val: num_complex::Complex64::new(f64::from(val.re), f64::from(val.im)),
                        tone: usize::from(tone),
                        sym: k,
                        shift: f64::from(shifts[k]),
                        hz: f64::from(hz[k]),
                    });
                }
            }
        }
        let blend = if count.iter().all(|&n| n >= 4) {
            let data = std::array::from_fn(|j| {
                let k = aided::data_idx(j);
                coherent::Data {
                    time: f64::from(starts[k]) / fs,
                    shift: f64::from(shifts[k]),
                    hz: f64::from(hz[k]),
                    bins: std::array::from_fn(|tone| bins[tone][k]),
                }
            });
            let span = f64::from(starts[NN - 1] + shifts[NN - 1] - starts[0] - shifts[0]) / fs;
            coherent::scores::<ND>(&pilots, &data, 12, 0.5 * span, NDOWNSPS, fs)
        } else {
            None
        };
        let mut result = WhiteningProcessor::process::<NROWS, ND, N>(
            &s1,
            &winners,
            self.m_llr_erasure_threshold,
            blend.as_ref(),
            LLR_SCALE_DEFAULT,
        );
        if !result.erasure_applied && self.m_llr_erasure_threshold > 0.0 {
            for v in result.llr0.iter_mut().chain(&mut result.llr1) {
                if v.abs() < self.m_llr_erasure_threshold {
                    *v = 0.0;
                }
            }
        }
        (result.llr0, result.llr1)
    }

    #[inline]
    fn evaluate_baseline(&self, x: f32) -> f32 {
        let x64 = f64::from(x);
        let x2 = x64 * x64;

        let mut baseline = 0.0f64;
        let mut exponent = 1.0f64;

        let pairs = self.baseline_c.len() / 2;
        for p in 0..pairs {
            let a = self.baseline_c[2 * p];
            let b = self.baseline_c[2 * p + 1];
            baseline = b.mul_add(x64, a).mul_add(exponent, baseline);
            exponent *= x2;
        }

        if (self.baseline_c.len() & 1) != 0 {
            baseline = self.baseline_c[self.baseline_c.len() - 1].mul_add(exponent, baseline);
        }

        baseline as f32
    }

    fn baselinejs8(&mut self, ia: i32, ib: i32) {
        let nsps_m1 = (Mode::NSPS as f32) - 1.0;
        let bmin = roundf(BASELINE_MIN as f32 / Mode::DF).clamp(0.0, nsps_m1) as usize;
        let bmax = roundf(BASELINE_MAX as f32 / Mode::DF).clamp(0.0, nsps_m1) as usize;

        if bmax < bmin {
            self.savg.fill(0.0);
            return;
        }

        let size = bmax - bmin + 1;
        let nodes_len = BASELINE_NODES.len().max(1);
        let arm = size / (2 * nodes_len);

        {
            let window = &mut self.savg[bmin..=bmax];
            for v in window.iter_mut() {
                *v = 10.0 * log10f(*v);
            }
        }

        let sample_pct = BASELINE_SAMPLE as usize;
        let max_span = (2 * arm).min(size);
        let mut span_buf: Vec<f32> = Vec::with_capacity(max_span);

        for (i, &node_frac) in BASELINE_NODES.iter().enumerate() {
            let node = (size as f64) * node_frac;
            let base = roundf(node as f32) as isize;

            let start = (base - arm as isize).max(0) as usize;
            let end = (base + arm as isize).min(size as isize) as usize;

            let start_abs = bmin + start;
            let end_abs = bmin + end;

            let slice = &self.savg[start_abs..end_abs];
            if slice.is_empty() {
                self.baseline_p[i] = [node, 0.0];
                continue;
            }

            span_buf.clear();
            span_buf.extend_from_slice(slice);

            let len = span_buf.len();
            let mut n = (len * sample_pct) / 100;
            if n >= len {
                n = len - 1;
            }

            span_buf.select_nth_unstable_by(n, f32::total_cmp);
            let yv = f64::from(span_buf[n]);

            self.baseline_p[i] = [node, yv];

            if i < 3 || i + 3 >= BASELINE_COUNT {}
        }

        let mut v = [[0.0f64; BASELINE_COUNT]; BASELINE_COUNT];
        let mut y = [0.0f64; BASELINE_COUNT];

        for r in 0..BASELINE_COUNT {
            let x = self.baseline_p[r][0];
            y[r] = self.baseline_p[r][1];
            v[r][0] = 1.0;
            for c in 1..BASELINE_COUNT {
                v[r][c] = v[r][c - 1] * x;
            }
        }

        self.baseline_c = solve_square::<BASELINE_COUNT>(&v, &y);

        let last = (size as f32) - 1.0;
        let denom = (ib - ia) as f32;

        self.savg.fill(0.0);

        if denom != 0.0 {
            let nsps_i32 = Mode::NSPS as i32;
            let start_i = ia.max(0);
            let end_i = ib.min(nsps_i32 - 1);

            if start_i <= end_i {
                for i in start_i..=end_i {
                    let x = ((i - ia) as f32) * last / denom;
                    let val = self.evaluate_baseline(x) + 0.65;
                    self.savg[i as usize] = val;
                }
            }
        }
    }

    fn compute_baseband_fft(&mut self) {
        // RustFFT does not have in-place real-to-complex packing like FFTW.
        // Instead, we compute a full complex FFT (imag=0) of length NDFFT1, then keep
        // the non-redundant bins [0..NDFFT1/2].
        let buf = &mut self.ds_time;

        let dd_len = self.dd.len();
        for (i, slot) in buf.iter_mut().enumerate().take(Mode::NDFFT1) {
            let re = if i < dd_len { self.dd[i] } else { 0.0 };
            *slot = Complex32::new(re, 0.0);
        }

        let fft = self
            .plans
            .get_or_create(FftPlanType::BB, Mode::NDFFT1, false);
        fft.process(buf.as_mut_slice());

        let half = Mode::NDFFT1 / 2;
        self.ds_cx[..=half].copy_from_slice(&buf[..=half]);
    }

    fn js8_downsample(&mut self, f0: f32) {
        const ZERO: Complex32 = Complex32 { re: 0.0, im: 0.0 };

        let df = 12000.0 / (Mode::NDFFT1 as f32);
        let baud = 12000.0 / (Mode::NSPS as f32);

        let ft = 8.5f32.mul_add(baud, f0);
        let fb = 1.5f32.mul_add(-baud, f0);

        let i0 = roundf(f0 / df) as i32;
        let it = min(roundf(ft / df) as i32, (Mode::NDFFT1 / 2) as i32);
        let ib = max(0, roundf(fb / df) as i32);

        let ndd_size = Mode::NDD + 1;
        let range_size = (it - ib + 1).max(0) as usize;

        for v in &mut self.cd0[..Mode::NDFFT2] {
            *v = ZERO;
        }

        if range_size != 0 {
            let ib_u = ib as usize;
            self.cd0[..range_size].copy_from_slice(&self.ds_cx[ib_u..ib_u + range_size]);
        }

        {
            let head = 0usize;
            let tail = range_size;

            let n = min(ndd_size, tail);
            for k in 0..n {
                self.cd0[head + k] *= self.taper[0][k];
            }

            let n = min(ndd_size, tail);
            for k in 0..n {
                self.cd0[tail - n + k] *= self.taper[1][k];
            }
        }

        let shift = (i0 - ib) as isize;
        if shift != 0 && Mode::NDFFT2 != 0 {
            rotate_left_complex(&mut self.cd0[..Mode::NDFFT2], shift);
        }

        let ifft = self
            .plans
            .get_or_create(FftPlanType::DS, Mode::NDFFT2, /*inverse=*/ true);
        ifft.process(&mut self.cd0[..Mode::NDFFT2]);

        let factor = 1.0 / sqrtf((Mode::NDFFT1 as f32) * (Mode::NDFFT2 as f32));
        for v in &mut self.cd0[..Mode::NDFFT2] {
            *v *= factor;
        }
    }

    pub(super) fn syncjs8(
        &mut self,
        mut nfa: i32,
        mut nfb: i32,
        spectrum_shift: Option<usize>,
    ) -> [Vec<Sync>; 2] {
        let costas = Self::costas();
        let costas = costas.map(|row| row.map(usize::from));

        let fft = self
            .plans
            .get_or_create(FftPlanType::SD, Mode::NFFT1, false);
        let spectra = (Mode::NMAX - Mode::NFFT1) / Mode::NSTEP + 1;

        #[cfg(feature = "experimental-time")]
        let first = if let Some(shift) = spectrum_shift.filter(|&shift| shift < spectra) {
            for (i, row) in self.s.iter_mut().enumerate() {
                for &power in &row[..shift] {
                    self.spectrum_sum[i] -= power;
                }
                row.copy_within(shift..spectra, 0);
                row[spectra - shift..].fill(0.0);
            }
            self.spectrum_updates += 1;
            spectra - shift
        } else {
            self.spectrum_sum.fill(0.0);
            self.spectrum_updates = 0;
            0
        };
        #[cfg(not(feature = "experimental-time"))]
        let first = {
            let _ = spectrum_shift;
            self.savg.fill(0.0);
            0
        };

        for j in first..spectra {
            let ia = j * Mode::NSTEP;
            let ib = ia + Mode::NFFT1;
            debug_assert!(ib <= Mode::NMAX);

            for k in 0..Mode::NFFT1 {
                let re = self.dd[ia + k] * self.nuttal[k];
                self.sd_time[k] = Complex32::new(re, 0.0);
            }

            fft.process(self.sd_time.as_mut_slice());

            for i in 0..Mode::NSPS {
                let power = self.sd_time[i].norm_sqr();
                self.s[i][j] = power;
                #[cfg(feature = "experimental-time")]
                {
                    self.spectrum_sum[i] += power;
                }
                #[cfg(not(feature = "experimental-time"))]
                {
                    self.savg[i] += power;
                }
            }
        }

        // Bound subtraction error in long-running streams without throwing
        // away the reusable FFT columns.
        #[cfg(feature = "experimental-time")]
        if self.spectrum_updates == 32 {
            for (sum, row) in self.spectrum_sum.iter_mut().zip(&self.s) {
                *sum = row[..spectra].iter().copied().sum();
            }
            self.spectrum_updates = 0;
        }
        #[cfg(feature = "experimental-time")]
        self.savg.copy_from_slice(&self.spectrum_sum);

        let nwin = nfb - nfa;
        let (_orig_nfa, _orig_nfb) = (nfa, nfb);

        if nfa < 100 {
            nfa = 100;
            if nwin < 100 {
                nfb = nfa + nwin;
            }
        }

        if nfb > 4910 {
            nfb = 4910;
            if nwin < 100 {
                nfa = nfb - nwin;
            }
        }

        let ia = max(0, roundf(nfa as f32 / Mode::DF) as i32);
        let ib = roundf(nfb as f32 / Mode::DF) as i32;

        self.baselinejs8(ia, ib);

        let timing_count = (2 * Mode::JZ + 1) as usize;
        let mut costas_power: [Vec<f32>; 3] = std::array::from_fn(|_| vec![0.0; timing_count]);
        let mut total_power: [Vec<f32>; 3] = std::array::from_fn(|_| vec![0.0; timing_count]);
        let mut entries = Vec::with_capacity((ib - ia + 1).max(0) as usize);

        for i in ia..=ib {
            let iu = i as usize;
            if iu >= Mode::NSPS {
                continue;
            }

            // This denominator is independent of the timing hypothesis. Computing it
            // once per frequency avoids summing the same seven rows for every Costas
            // symbol at every candidate start.
            let mut tone_power = self.s[iu];
            for tone in 1..7 {
                let fidx = iu + NFOS * tone;
                debug_assert!(fidx < Mode::NSPS);
                for (sum, power) in tone_power.iter_mut().zip(&self.s[fidx]) {
                    *sum += *power;
                }
            }

            for block in 0..3 {
                costas_power[block].fill(0.0);
                total_power[block].fill(0.0);

                for n in 0..7 {
                    let offset =
                        -Mode::JZ + Mode::JSTRT + NSSY * (n as i32) + (block as i32) * 36 * NSSY;
                    let start = max(0, -offset) as usize;
                    let end = min(timing_count as i32, Mode::NHSYM as i32 - offset).max(0) as usize;
                    if start >= end {
                        continue;
                    }

                    let src_start = (offset + start as i32) as usize;
                    let len = end - start;
                    let tone = costas_tone_index::<Mode>(&costas, block, n);
                    let fidx = iu + NFOS * tone;
                    debug_assert!(fidx < Mode::NSPS);

                    let costas_src = &self.s[fidx][src_start..src_start + len];
                    let total_src = &tone_power[src_start..src_start + len];
                    let costas_dst = &mut costas_power[block][start..end];
                    let total_dst = &mut total_power[block][start..end];

                    for k in 0..len {
                        costas_dst[k] += costas_src[k];
                        total_dst[k] += total_src[k];
                    }
                }
            }

            let mut max_value = f32::NEG_INFINITY;
            let mut max_index = -Mode::JZ;

            for j in 0..timing_count {
                let tx0 = costas_power[0][j];
                let tx1 = costas_power[1][j];
                let tx2 = costas_power[2][j];
                let t00 = total_power[0][j];
                let t01 = total_power[1][j];
                let t02 = total_power[2][j];

                let tx01 = tx0 + tx1;
                let t001 = t00 + t01;
                let tx12 = tx1 + tx2;
                let t012 = t01 + t02;
                let tx012 = tx01 + tx2;
                let t0012 = t001 + t02;

                let s0 = tx012 / ((t0012 - tx012) / 6.0);
                let s1 = tx01 / ((t001 - tx01) / 6.0);
                let s2 = tx12 / ((t012 - tx12) / 6.0);
                let sync_value = s0.max(s1.max(s2));

                if sync_value > max_value {
                    max_value = sync_value;
                    max_index = j as i32 - Mode::JZ;
                }
            }

            entries.push(Sync::new(
                Mode::DF * (i as f32),
                Mode::TSTEP * ((max_index as f32) + 0.5),
                max_value,
            ));
        }

        if entries.is_empty() {
            return [Vec::new(), Vec::new()];
        }

        let mut vals: Vec<f32> = entries.iter().map(|e| e.sync).collect();
        let q_idx = (vals.len() * 4) / 10;
        vals.select_nth_unstable_by(q_idx, f32::total_cmp);
        let q = vals[q_idx];
        if q != 0.0 && q.is_finite() {
            for e in &mut entries {
                e.sync /= q;
            }
        }

        // A single sort followed by greedy suppression is equivalent to
        // repeatedly sorting and removing the selected candidate's neighborhood.
        entries.sort_unstable_by(|a, b| {
            b.sync
                .total_cmp(&a.sync)
                .then_with(|| a.freq.total_cmp(&b.freq))
        });
        let mut candidates: [Vec<Sync>; 2] =
            [Vec::with_capacity(NMAXCAND.min(entries.len())), Vec::new()];
        for entry in entries {
            if entry.sync < 1.20 || entry.sync.is_nan() {
                break;
            }
            let band = usize::from(entry.sync < ASYNCMIN);
            if candidates[band].len() >= if band == 0 { NMAXCAND } else { 24 } {
                break;
            }
            if candidates
                .iter()
                .flatten()
                .all(|sel| (entry.freq - sel.freq).abs() > Mode::AZ)
            {
                candidates[band].push(entry);
            }
        }

        candidates
    }

    #[inline]
    fn wrap_phase(mut phi: f32) -> f32 {
        phi = fmodf(phi, TAU);
        if phi < 0.0 {
            phi += TAU;
        }
        phi
    }

    fn syncjs8d(&self, i0: i32, delf: f32) -> f32 {
        let base_dphi: f32 = TAU * ((Mode::NDOWN as f32) / 12000.0);
        let mut freq_adjust = [Complex32::new(1.0, 0.0); NDOWNSPS];

        if delf != 0.0 {
            let dphi = base_dphi * delf;
            let mut phi = 0.0f32;

            for adjust in freq_adjust.iter_mut().take(Mode::NDOWNSPS) {
                *adjust = Complex32::from_polar(1.0, phi);
                phi = Self::wrap_phase(phi + dphi);
            }
        }

        // Accumulate sync power across 3 Costas blocks x 7 columns.
        let mut sync_power = 0.0f32;

        for block in 0..3usize {
            for col in 0..7usize {
                let offset = 36 * (block as i32) * (Mode::NDOWNSPS as i32)
                    + i0
                    + (col as i32) * (Mode::NDOWNSPS as i32);

                if offset < 0 {
                    continue;
                }

                let offset_u = offset as usize;
                if offset_u + Mode::NDOWNSPS > Mode::NP2 {
                    continue;
                }

                let mut acc = Complex32::new(0.0, 0.0);

                for (k, adjust) in freq_adjust.iter().enumerate().take(Mode::NDOWNSPS) {
                    let rot = *adjust * self.csyncs[block][col][k];
                    acc += self.cd0[offset_u + k] * rot.conj();
                }

                sync_power += acc.norm_sqr();
            }
        }

        sync_power
    }

    fn genjs8refsig(&self, itone: &[u8; NN], f0: f32) -> Vec<Complex32> {
        let bfpi: f32 = TAU * f0 * (1.0 / 12000.0);
        let mut phi: f32 = 0.0;

        let mut cref: Vec<Complex32> = Vec::with_capacity(NN * Mode::NSPS);

        for tone in itone.iter().take(NN) {
            let dphi = bfpi + TAU * f32::from(*tone) / (Mode::NSPS as f32);
            let (step_sin, step_cos) = libm::sincosf(dphi);
            let step = Complex32::new(step_cos, step_sin);
            let (sin, cos) = libm::sincosf(phi);
            let mut oscillator = Complex32::new(cos, sin);

            for i in 0..Mode::NSPS {
                cref.push(oscillator);

                phi += dphi;
                if phi >= TAU {
                    phi -= TAU;
                }
                oscillator *= step;

                if (i & 255) == 255 {
                    let (sin, cos) = libm::sincosf(phi);
                    oscillator = Complex32::new(cos, sin);
                }
            }
        }

        cref
    }

    fn subtractjs8(&mut self, cref: &[Complex32], dt: f32) {
        let nstart = sic::start(dt);
        let cref_start: usize = if nstart < 0 { (-nstart) as usize } else { 0 };
        let dd_start: usize = if nstart > 0 { nstart as usize } else { 0 };

        if cref_start >= cref.len() || dd_start >= self.dd.len() {
            return;
        }

        let size = core::cmp::min(cref.len() - cref_start, self.dd.len() - dd_start);

        for i in 0..size {
            self.cfilt[i] =
                Complex32::new(self.dd[dd_start + i], 0.0) * cref[cref_start + i].conj();
        }
        for i in size..Mode::NMAX {
            self.cfilt[i] = Complex32::new(0.0, 0.0);
        }

        let fft_fwd = self.plans.get_or_create(FftPlanType::CF, Mode::NMAX, false);
        fft_fwd.process(self.cfilt.as_mut_slice());

        for i in 0..Mode::NMAX {
            self.cfilt[i] *= self.filter[i];
        }

        let fft_inv = self.plans.get_or_create(FftPlanType::CB, Mode::NMAX, true);
        fft_inv.process(self.cfilt.as_mut_slice());

        sic::subtract(
            &mut self.dd,
            cref,
            &self.cfilt[..size],
            Mode::NSPS,
            dt,
            &mut self.sic_alt,
        );
    }

    pub fn new() -> Self {
        let mut this = Self::zeroed();

        const A0: f32 = 0.363_581_9;
        const A1: f32 = -0.489_177_5;
        const A2: f32 = 0.136_599_5;
        const A3: f32 = -0.010_641_1;

        let pi = 4.0f32 * atanf(1.0f32);
        let mut sum = 0.0f32;

        let n = Mode::NFFT1 as f32;
        for i in 0..Mode::NFFT1 {
            let i_f = i as f32;
            let mut v = KahanSum::<f32>::new(A0);

            v += A1 * cosf(2.0 * pi * i_f / n);
            v += A2 * cosf(4.0 * pi * i_f / n);
            v += A3 * cosf(6.0 * pi * i_f / n);

            let value = v.value();
            this.nuttal[i] = value;
            sum += value;
        }

        for v in &mut this.nuttal {
            *v = (*v / sum) * (Mode::NFFT1 as f32) / 300.0;
        }

        let costas = Self::costas();
        for (i, ((tone_a, tone_b), tone_c)) in costas[0]
            .iter()
            .zip(costas[1].iter())
            .zip(costas[2].iter())
            .enumerate()
            .take(7usize)
        {
            let dphia = TAU * f32::from(*tone_a) / (Mode::NDOWNSPS as f32);
            let dphib = TAU * f32::from(*tone_b) / (Mode::NDOWNSPS as f32);
            let dphic = TAU * f32::from(*tone_c) / (Mode::NDOWNSPS as f32);

            let mut phia = 0.0f32;
            let mut phib = 0.0f32;
            let mut phic = 0.0f32;

            for j in 0..Mode::NDOWNSPS {
                this.csyncs[0][i][j] = Complex32::from_polar(1.0, phia);
                this.csyncs[1][i][j] = Complex32::from_polar(1.0, phib);
                this.csyncs[2][i][j] = Complex32::from_polar(1.0, phic);

                phia = fmodf(phia + dphia, TAU);
                phib = fmodf(phib + dphib, TAU);
                phic = fmodf(phic + dphic, TAU);
            }
        }

        let mut filt_sum = 0.0f32;
        for j in (-NFILT / 2)..=(NFILT / 2) {
            filt_sum += powf(cosf(pi * j as f32 / NFILT as f32), 2.0);
        }
        // Negative taps wrap across the entire FFT, not the short kernel.
        for j in (-NFILT / 2)..=(NFILT / 2) {
            let idx = if j < 0 {
                Mode::NMAX - (-j) as usize
            } else {
                j as usize
            };
            this.filter[idx] = Complex32::new(
                powf(cosf(pi * j as f32 / NFILT as f32), 2.0) / filt_sum,
                0.0,
            );
        }
        let fft = this.plans.get_or_create(FftPlanType::CF, Mode::NMAX, false);
        fft.process(this.filter.as_mut_slice());
        let factor = 1.0f32 / (Mode::NMAX as f32);
        for v in &mut this.filter {
            *v *= factor;
        }

        this
    }

    /// Decode entry point (C++ `operator()`).
    pub fn decode<E>(
        &mut self,
        data: &DecData<'_>,
        kpos: &usize,
        ksz: &usize,
        stream_start: Option<u64>,
        mut emit_event: E,
    ) -> usize
    where
        E: FnMut(Event),
        Mode: ModeSpec,
    {
        let pos = *kpos;
        let sz = *ksz;

        debug_assert!(sz <= Mode::NMAX, "decode: sz={} > NMAX={}", sz, Mode::NMAX);

        if data.params.sync_stats {
            emit_event(Event::SyncStart(SyncStart {
                sample_position: pos,
                sample_count: sz,
            }));
        }

        self.dd.fill(0.0);

        let wrap = data.d2.len().saturating_sub(pos) < sz;

        if wrap {
            debug_assert!(
                pos <= data.d2.len(),
                "decode: pos={pos} > buffer size={}",
                data.d2.len()
            );

            let first = data.d2.len() - pos;
            let second = sz - first;

            for i in 0..first {
                self.dd[i] = f32::from(data.d2[pos + i]);
            }
            for i in 0..second {
                self.dd[first + i] = f32::from(data.d2[i]);
            }
        } else {
            for i in 0..sz {
                self.dd[i] = f32::from(data.d2[pos + i]);
            }
        }

        let mut decodes: DecodeMap = HashMap::new();
        #[cfg(feature = "experimental-time")]
        let spectrum_shift = match (self.spectrum_start, stream_start) {
            (Some(previous), Some(current)) if current >= previous => {
                let delta = current - previous;
                let step = Mode::NSTEP as u64;
                if delta.is_multiple_of(step) {
                    usize::try_from(delta / step).ok()
                } else {
                    None
                }
            }
            _ => None,
        };
        #[cfg(feature = "experimental-time")]
        {
            self.spectrum_start = stream_start;
        }
        #[cfg(feature = "experimental-time")]
        let mut spectrum_valid = stream_start.is_some();
        #[cfg(not(feature = "experimental-time"))]
        let spectrum_shift = {
            let _ = stream_start;
            None
        };

        let ttl_secs_i32 = Mode::NTXDUR * 2;

        let ttl = Duration::from_secs(ttl_secs_i32 as u64);
        self.m_soft_combiner.flush(ttl);
        self.legacy_combiner.flush(ttl);

        let mut rescue = 4;
        let mut stalled = false;
        self.sic_alt = None;
        for ipass in 1..=4 {
            if stalled && ipass < 4 {
                continue;
            }
            let retry = ipass == 4;
            // uh
            if retry {
                let Some(alt) = self.sic_alt.take() else {
                    break;
                };
                self.dd = alt;
                #[cfg(feature = "experimental-time")]
                {
                    spectrum_valid = false;
                }
            }
            let shift = (ipass == 1).then_some(spectrum_shift).flatten();
            let mut candidates =
                self.syncjs8(data.params.nfa as i32, data.params.nfb as i32, shift);

            if candidates.iter().all(Vec::is_empty) {
                if !retry && self.sic_alt.is_some() {
                    stalled = true;
                    continue;
                }
                break;
            }
            let nfqso = data.params.nfqso;

            for band in &mut candidates {
                band.sort_unstable_by(|a, b| {
                    let a_dist = (a.freq - nfqso as f32).abs();
                    let b_dist = (b.freq - nfqso as f32).abs();

                    if a_dist < 10.0 && b_dist >= 10.0 {
                        return Ordering::Less;
                    }
                    if b_dist < 10.0 && a_dist >= 10.0 {
                        return Ordering::Greater;
                    }

                    match a_dist.total_cmp(&b_dist) {
                        Ordering::Equal => a.freq.total_cmp(&b.freq),
                        other => other,
                    }
                });
            }

            self.compute_baseband_fft();

            let subtract = ipass < 3;

            let mut improved = false;

            for (band, cands) in candidates.into_iter().enumerate() {
                let deep = band == 1;
                if deep && improved && ipass < 3 {
                    continue;
                }
                for cand in cands {
                    let mut f1 = cand.freq;
                    let mut xdt = cand.step;
                    let mut xsnr = 0.0f32;
                    let mut nharderrors = -1i32;

                    let res = self.js8dec(
                        data.params.sync_stats,
                        subtract,
                        &mut f1,
                        &mut xdt,
                        &mut nharderrors,
                        &mut xsnr,
                        &mut rescue,
                        deep,
                        retry,
                        &mut emit_event,
                    );

                    if let Some(decode) = res {
                        #[cfg(feature = "experimental-time")]
                        if subtract {
                            spectrum_valid = false;
                        }
                        let snr = libm::roundf(xsnr) as i32;

                        use std::collections::hash_map::Entry;

                        match decodes.entry(decode) {
                            Entry::Vacant(v) => {
                                improved = true;

                                let quality = 1.0f32 - (nharderrors as f32) / 60.0f32;

                                let (text, msg_type) = {
                                    let dref = v.key();
                                    (
                                        core::str::from_utf8(&dref.data)
                                            .expect("JS8 decode produced non-UTF8 bytes")
                                            .to_owned(),
                                        dref.type_id,
                                    )
                                };

                                v.insert(snr);

                                emit_event(Event::Decoded(Decoded::from_raw(
                                    text,
                                    FrameFlags::from_bits_truncate(msg_type),
                                    Mode::NSUBMODE,
                                    data.params.nutc,
                                    snr,
                                    xdt - Mode::ASTART,
                                    f1,
                                    quality,
                                )));
                            }

                            Entry::Occupied(mut o) => {
                                let prev = *o.get();
                                if prev < snr {
                                    improved = true;
                                    *o.get_mut() = snr;

                                    let dref = o.key();
                                    let quality = 1.0f32 - (nharderrors as f32) / 60.0f32;

                                    emit_event(Event::Decoded(Decoded::from_raw(
                                        core::str::from_utf8(&dref.data)
                                            .expect("JS8 decode produced non-UTF8 bytes")
                                            .to_owned(),
                                        FrameFlags::from_bits_truncate(dref.type_id),
                                        Mode::NSUBMODE,
                                        data.params.nutc,
                                        snr,
                                        xdt - Mode::ASTART,
                                        f1,
                                        quality,
                                    )));
                                }
                            }
                        }
                    }
                }
            }

            if !improved {
                if retry || self.sic_alt.is_none() {
                    break;
                }
                stalled = true;
            }
        }

        #[cfg(feature = "experimental-time")]
        {
            if !spectrum_valid {
                self.spectrum_start = None;
            }
        }

        self.sic_alt = None;
        decodes.len()
    }
}

fn solve_square<const N: usize>(a: &[[f64; N]; N], b: &[f64; N]) -> [f64; N] {
    let mut m = *a;
    let mut rhs = *b;

    for k in 0..N {
        let mut piv = k;
        let mut piv_val = m[k][k].abs();
        let mut i = k + 1;
        while i < N {
            let v = m[i][k].abs();
            if v > piv_val {
                piv = i;
                piv_val = v;
            }
            i += 1;
        }
        if piv != k {
            m.swap(k, piv);
            rhs.swap(k, piv);
        }

        let diag = m[k][k];
        if diag == 0.0 {
            continue;
        }

        let mut i = k + 1;
        while i < N {
            let f = m[i][k] / diag;
            m[i][k] = 0.0;
            let mut j = k + 1;
            while j < N {
                m[i][j] = f.mul_add(-m[k][j], m[i][j]);
                j += 1;
            }
            rhs[i] = f.mul_add(-rhs[k], rhs[i]);
            i += 1;
        }
    }

    let mut x = [0.0f64; N];
    for i in (0..N).rev() {
        let mut s = rhs[i];
        for j in (i + 1)..N {
            s = m[i][j].mul_add(-x[j], s);
        }
        let diag = m[i][i];
        x[i] = if diag == 0.0 { 0.0 } else { s / diag };
    }

    x
}

const fn rotate_left_complex(buf: &mut [Complex32], shift: isize) {
    let len = buf.len();
    if len == 0 {
        return;
    }
    let mut k = shift % (len as isize);
    if k < 0 {
        k += len as isize;
    }
    let k = k as usize;
    if k == 0 {
        return;
    }
    buf.rotate_left(k);
}

#[inline]
fn costas_tone_index<Mode: ModeSpec>(costas: &[[usize; 7]; 3], p: usize, n: usize) -> usize {
    costas[p][n].min(NROWS - 1)
}

pub struct DecoderCore {
    a: Option<Box<DecodeNormal>>,
    b: Option<Box<DecodeFast>>,
    c: Option<Box<DecodeTurbo>>,
    e: Option<Box<DecodeSlow>>,
    i: Option<Box<DecodeUltra>>,
}

pub const ENABLE_NORMAL: u8 = 1 << 0;
pub const ENABLE_FAST: u8 = 1 << 1;
pub const ENABLE_TURBO: u8 = 1 << 2;
pub const ENABLE_SLOW: u8 = 1 << 3;
pub const ENABLE_ULTRA: u8 = 1 << 4;

fn stream_start(data: &DecData<'_>, index: usize) -> Option<u64> {
    #[cfg(feature = "experimental-time")]
    {
        data.params.stream_starts.map(|starts| starts[index])
    }
    #[cfg(not(feature = "experimental-time"))]
    {
        let _ = (data, index);
        None
    }
}

impl DecoderCore {
    pub(crate) fn decode_pass<E>(&mut self, data: &mut DecData<'_>, emit: &mut E) -> usize
    where
        E: FnMut(Event),
    {
        let set = data.params.nsubmodes;
        let modes = DecodeModes::from_bits_truncate(set);
        let mut sum: usize = 0;

        emit(Event::DecodeStarted(DecodeStarted { modes }));

        if (set & ENABLE_ULTRA) == ENABLE_ULTRA {
            let kpos = data.params.kpos_i;
            let ksz = data.params.ksz_i;
            let decoder = self.i.get_or_insert_with(|| Box::new(DecodeUltra::new()));

            sum += decoder.decode(&*data, &kpos, &ksz, stream_start(data, 4), &mut *emit);

            data.params.kpos_i = kpos;
            data.params.ksz_i = ksz;
        }

        if (set & ENABLE_SLOW) == ENABLE_SLOW {
            let kpos = data.params.kpos_e;
            let ksz = data.params.ksz_e;
            let decoder = self.e.get_or_insert_with(|| Box::new(DecodeSlow::new()));

            sum += decoder.decode(&*data, &kpos, &ksz, stream_start(data, 3), &mut *emit);

            data.params.kpos_e = kpos;
            data.params.ksz_e = ksz;
        }

        if (set & ENABLE_TURBO) == ENABLE_TURBO {
            let kpos = data.params.kpos_c;
            let ksz = data.params.ksz_c;
            let decoder = self.c.get_or_insert_with(|| Box::new(DecodeTurbo::new()));

            sum += decoder.decode(&*data, &kpos, &ksz, stream_start(data, 2), &mut *emit);

            data.params.kpos_c = kpos;
            data.params.ksz_c = ksz;
        }

        if (set & ENABLE_FAST) == ENABLE_FAST {
            let kpos = data.params.kpos_b;
            let ksz = data.params.ksz_b;
            let decoder = self.b.get_or_insert_with(|| Box::new(DecodeFast::new()));

            sum += decoder.decode(&*data, &kpos, &ksz, stream_start(data, 1), &mut *emit);

            data.params.kpos_b = kpos;
            data.params.ksz_b = ksz;
        }

        if (set & ENABLE_NORMAL) == ENABLE_NORMAL {
            let kpos = data.params.kpos_a;
            let ksz = data.params.ksz_a;
            let decoder = self.a.get_or_insert_with(|| Box::new(DecodeNormal::new()));

            sum += decoder.decode(&*data, &kpos, &ksz, stream_start(data, 0), &mut *emit);

            data.params.kpos_a = kpos;
            data.params.ksz_a = ksz;
        }

        emit(Event::DecodeFinished(DecodeFinished { decoded: sum }));

        sum
    }
}

impl DecoderCore {
    pub(crate) fn with_modes(modes: DecodeModes) -> Self {
        Self {
            a: modes
                .contains(DecodeModes::NORMAL)
                .then(|| Box::new(DecodeNormal::new())),
            b: modes
                .contains(DecodeModes::FAST)
                .then(|| Box::new(DecodeFast::new())),
            c: modes
                .contains(DecodeModes::TURBO)
                .then(|| Box::new(DecodeTurbo::new())),
            e: modes
                .contains(DecodeModes::SLOW)
                .then(|| Box::new(DecodeSlow::new())),
            i: modes
                .contains(DecodeModes::ULTRA)
                .then(|| Box::new(DecodeUltra::new())),
        }
    }
}
