// SPDX-License-Identifier: GPL-3.0-only

use std::f64::consts::{PI, TAU};

use num_complex::{Complex32, Complex64};

use super::kalman::FrequencyTracker;

pub(crate) const MAX_SYND: usize = 4;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Baseline {
    pub start: isize,
    pub hz: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Hyp {
    pub dt: isize,
    pub df: f64,
    pub drift: f64,
    pub metric: f64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Refine {
    pub best: Hyp,
    pub base: f64,
    pub searched: bool,
    pub boundary: bool,
}

impl Default for Refine {
    fn default() -> Self {
        Self {
            best: Hyp {
                dt: 0,
                df: 0.0,
                drift: 0.0,
                metric: f64::NAN,
            },
            base: f64::NAN,
            searched: false,
            boundary: false,
        }
    }
}

impl Refine {
    pub(crate) fn accepted(&self) -> bool {
        self.searched
            && !self.boundary
            && self.base.is_finite()
            && self.best.metric.is_finite()
            && self.best.metric > self.base
            && if self.base > 0.0 {
                self.best.metric >= self.base * 1.005
            } else {
                self.best.metric > 0.0
            }
    }
}

pub(crate) fn is_pilot(k: usize) -> bool {
    k < 7 || (36..43).contains(&k) || (72..79).contains(&k)
}

pub(crate) const fn data_idx(j: usize) -> usize {
    if j < 29 { j + 7 } else { j + 14 }
}

pub(crate) fn tones(cw: &[i8; 174], costas: &[[u8; 7]; 3]) -> [u8; 79] {
    let mut out = [0; 79];
    for (b, row) in costas.iter().enumerate() {
        out[b * 36..b * 36 + 7].copy_from_slice(row);
    }
    for (j, bits) in cw.as_chunks::<3>().0.iter().enumerate() {
        out[data_idx(j)] =
            ((bits[0] != 0) as u8) << 2 | ((bits[1] != 0) as u8) << 1 | (bits[2] != 0) as u8;
    }
    out
}

pub(crate) fn weights(llr: &[f32; 174]) -> [f32; 79] {
    let mut out = [1.0; 79];
    for (j, bits) in llr.as_chunks::<3>().0.iter().enumerate() {
        out[data_idx(j)] = if bits.iter().all(|v| v.is_finite()) {
            let min = bits
                .iter()
                .map(|v| f64::from(v.abs()))
                .fold(f64::INFINITY, f64::min);
            ((min - 0.5) / 2.5).clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
    }
    out
}

pub(crate) fn bounds(bases: &[Baseline; 79], win: usize, len: usize, max_dt: isize) -> bool {
    let (Ok(win), Ok(len)) = (isize::try_from(win), isize::try_from(len)) else {
        return false;
    };
    if win <= 0 || len <= 0 || max_dt < 0 {
        return false;
    }
    bases.iter().all(|b| {
        b.start >= max_dt
            && b.start
                .checked_add(max_dt)
                .and_then(|n| n.checked_add(win))
                .is_some_and(|end| end <= len)
    })
}

pub(crate) fn replay(samples: &mut [Complex32], hz: f64, fs: f64) -> bool {
    if samples.is_empty()
        || !hz.is_finite()
        || !fs.is_finite()
        || fs <= 0.0
        || samples
            .iter()
            .any(|z| !z.re.is_finite() || !z.im.is_finite())
    {
        return false;
    }
    let mut trk = FrequencyTracker::new();
    trk.reset_default(hz, fs);
    trk.apply(samples);
    true
}

pub(crate) fn mid_resid(
    df: f64,
    drift: f64,
    bases: &[Baseline; 79],
    win: usize,
    fs: f64,
    weights: &[f32; 79],
) -> Option<f64> {
    if !df.is_finite() || !drift.is_finite() || win == 0 || !fs.is_finite() || fs <= 0.0 {
        return None;
    }
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let (mut sw, mut st, mut stt, mut sr, mut str) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (b, &w) in bases.iter().zip(weights) {
        if !w.is_finite() || w <= 0.0 {
            continue;
        }
        let t = (b.start as f64 + win as f64 * 0.5) / fs;
        // Tracker correction adds frequency; undo it to recover physical RF.
        let r = df + drift * t - f64::from(b.hz);
        if !t.is_finite() || !r.is_finite() {
            return None;
        }
        lo = lo.min(t);
        hi = hi.max(t);
        let w = f64::from(w);
        sw += w;
        st += w * t;
        stt += w * t * t;
        sr += w * r;
        str += w * t * r;
    }
    if !sw.is_finite() || sw <= 0.0 || !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let det = sw * stt - st * st;
    let resid = if det.is_finite() && det > 0.0 {
        let a = (sr * stt - st * str) / det;
        let b = (sw * str - st * sr) / det;
        a + b * lo.midpoint(hi)
    } else {
        sr / sw
    };
    resid.is_finite().then_some(resid)
}

fn contrast(bins: &[Complex64; 8], tone: usize) -> Option<f64> {
    let exp = bins[tone].norm_sqr();
    if !exp.is_finite() {
        return None;
    }
    let mut sum = 0.0;
    for (i, bin) in bins.iter().enumerate() {
        if i == tone {
            continue;
        }
        let p = bin.norm_sqr();
        if !p.is_finite() {
            return None;
        }
        sum += p;
    }
    let c = exp - sum / 7.0;
    c.is_finite().then_some(c)
}

pub(crate) fn refine(
    samples: &[Complex32],
    bases: &[Baseline; 79],
    win: usize,
    fs: f64,
    tones: &[u8; 79],
    weights: &[f32; 79],
) -> Refine {
    let mut out = Refine::default();

    if win == 0 || win > 32 || !fs.is_finite() || fs <= 0.0 || !bounds(bases, win, samples.len(), 2)
    {
        return out;
    }
    for k in 0..79 {
        if tones[k] >= 8
            || !weights[k].is_finite()
            || weights[k] < 0.0
            || (weights[k] > 0.0 && !bases[k].hz.is_finite())
        {
            return out;
        }
    }
    let tw: [[Complex64; 32]; 8] = std::array::from_fn(|tone| {
        std::array::from_fn(|n| Complex64::from_polar(1.0, -TAU * (tone * n) as f64 / win as f64))
    });
    // Replay each tracked window once; keep the 100 KiB cache off the stack.
    let mut cache = vec![None; 5 * 79];
    let cache_dt = |cache: &mut Vec<Option<[Complex32; 32]>>, dt: isize| {
        for k in 0..79 {
            if weights[k] <= 0.0 {
                continue;
            }
            let start = (bases[k].start + dt) as usize;
            let mut buf = [Complex32::new(0.0, 0.0); 32];
            buf[..win].copy_from_slice(&samples[start..start + win]);
            if replay(&mut buf[..win], f64::from(bases[k].hz), fs) {
                cache[(dt + 2) as usize * 79 + k] = Some(buf);
            }
        }
    };
    let score = |cache: &[Option<[Complex32; 32]>], dt: isize, df: f64, drift: f64| {
        let mut metric = 0.0;
        let accel = Complex64::from_polar(1.0, -TAU * drift / (fs * fs));
        for k in 0..79 {
            if weights[k] <= 0.0 {
                continue;
            }
            let Some(buf) = &cache[(dt + 2) as usize * 79 + k] else {
                continue;
            };
            let start = (bases[k].start + dt) as usize;
            let t = start as f64 / fs;
            // Restart every window (at most 32 samples) to bound accumulated phase error.
            let mut rot = Complex64::from_polar(1.0, -(TAU * df * t + PI * drift * t * t));
            let mut step =
                Complex64::from_polar(1.0, -(TAU * df + PI * drift * (2.0 * t + 1.0 / fs)) / fs);
            let mut bins = [Complex64::new(0.0, 0.0); 8];
            for (n, z) in buf[..win].iter().enumerate() {
                let y = Complex64::new(f64::from(z.re), f64::from(z.im)) * rot;
                for tone in 0..8 {
                    bins[tone] += y * tw[tone][n];
                }
                rot *= step;
                step *= accel;
            }
            if let Some(c) = contrast(&bins, usize::from(tones[k])) {
                metric += f64::from(weights[k]) * c;
            }
        }
        metric
    };
    cache_dt(&mut cache, 0);
    out.base = score(&cache, 0, 0.0, 0.0);
    if !out.base.is_finite() {
        return out;
    }
    out.searched = true;
    out.best.metric = out.base;

    for dt in [-2, -1, 1, 2] {
        cache_dt(&mut cache, dt);
    }
    for drift in [-0.02, 0.0, 0.02] {
        for f in 0..11 {
            let df = -0.5 + f64::from(f) * 0.1;
            for dt in -2..=2 {
                if dt == 0 && df == 0.0 && drift == 0.0 {
                    continue;
                }
                let metric = score(&cache, dt, df, drift);
                if metric.is_finite() && metric > out.best.metric {
                    out.best = Hyp {
                        dt,
                        df,
                        drift,
                        metric,
                    };
                }
            }
        }
    }
    // Drift edges are allowed: this power metric barely observes within-symbol curvature.
    out.boundary =
        out.best.dt.abs() == 2 || out.best.df == -0.5 || out.best.df == -0.5 + 10.0 * 0.1;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::costas;

    fn cw() -> [i8; 174] {
        std::array::from_fn(|i| (((i as u32).wrapping_mul(40_503) >> 7) & 1) as i8)
    }

    fn bases(shift: isize, hz: f32) -> [Baseline; 79] {
        std::array::from_fn(|k| Baseline {
            start: 64 + k as isize * 32 + shift,
            hz,
        })
    }

    fn signal(tones: &[u8; 79], shift: f64, df: f64, drift: f64) -> Vec<Complex32> {
        (0..64 + 79 * 32 + 16)
            .map(|n| {
                let rel = n as f64 - 64.0 - shift;
                let k = (rel / 32.0).floor().clamp(0.0, 78.0) as usize;
                let local = rel - k as f64 * 32.0;
                let t = n as f64 / 200.0;
                let phi = 0.7
                    + TAU * df * t
                    + PI * drift * t * t
                    + TAU * f64::from(tones[k]) * local / 32.0;
                Complex32::from_polar(1.0, phi as f32)
            })
            .collect()
    }

    #[test]
    fn tone_map() {
        let mut bits = [0; 174];
        for j in 0..58 {
            for bit in 0..3 {
                bits[3 * j + bit] = ((j >> (2 - bit)) & 1) as i8;
            }
        }
        for costas in [&costas::ORIGINAL, &costas::MODIFIED] {
            let t = tones(&bits, costas);
            for j in 0..58 {
                assert_eq!(usize::from(t[data_idx(j)]), j % 8);
            }
            for b in 0..3 {
                assert_eq!(&t[b * 36..b * 36 + 7], &costas[b]);
            }
        }
    }

    #[test]
    fn conf_bounds() {
        let mut llr = [3.0; 174];
        llr[0] = 0.49;
        llr[3] = 1.75;
        llr[6] = f32::NAN;
        llr[9] = f32::INFINITY;
        let w = weights(&llr);
        assert_eq!([w[7], w[8], w[9], w[10], w[11]], [0.0, 0.5, 0.0, 0.0, 1.0]);
        for (k, &weight) in w.iter().enumerate() {
            if is_pilot(k) {
                assert_eq!(weight, 1.0);
            }
        }
        let mut b = bases(0, 0.0);
        assert!(bounds(&b, 32, 64 + 79 * 32 + 16, 2));
        b[0].start = 1;
        assert!(!bounds(&b, 32, 64 + 79 * 32 + 16, 2));
        b[0].start = isize::MAX;
        assert!(!bounds(&b, 32, usize::MAX, 2));
    }

    #[test]
    fn recover_freq_tmg() {
        let tones = tones(&cw(), &costas::ORIGINAL);
        for df in [-0.4, 0.4] {
            let samples = signal(&tones, 0.0, df, 0.0);
            let r = refine(&samples, &bases(0, 0.0), 32, 200.0, &tones, &[1.0; 79]);
            assert!(r.searched && r.accepted(), "{r:?}");
            assert!((r.best.df - df).abs() < 0.051);
            assert!(r.best.dt.abs() <= 1);
        }
        for shift in [-2.0, -1.5, 1.5, 2.0] {
            let samples = signal(&tones, shift, 0.0, 0.0);
            let r = refine(&samples, &bases(0, 0.0), 32, 200.0, &tones, &[1.0; 79]);
            assert!(r.searched && (r.best.dt > 0) == (shift > 0.0), "{r:?}");
            assert!((1..=2).contains(&r.best.dt.abs()));
        }
    }

    #[test]
    fn score_ref() {
        let tones = tones(&cw(), &costas::ORIGINAL);
        let bases = bases(0, 0.13);
        let tw: [[Complex64; 32]; 8] = std::array::from_fn(|tone| {
            std::array::from_fn(|n| Complex64::from_polar(1.0, -TAU * (tone * n) as f64 / 32.0))
        });
        for (shift, df, drift) in [(-1.0, -0.4, -0.02), (0.0, 0.0, 0.0), (1.0, 0.4, 0.02)] {
            let samples = signal(&tones, shift, df, drift);
            let r = refine(&samples, &bases, 32, 200.0, &tones, &[1.0; 79]);
            assert!(r.searched);
            for hyp in [
                Hyp {
                    dt: 0,
                    df: 0.0,
                    drift: 0.0,
                    metric: r.base,
                },
                r.best,
            ] {
                let mut want = 0.0;
                for k in 0..79 {
                    let start = (bases[k].start + hyp.dt) as usize;
                    let mut buf = samples[start..start + 32].to_vec();
                    assert!(replay(&mut buf, f64::from(bases[k].hz), 200.0));
                    let mut bins = [Complex64::new(0.0, 0.0); 8];
                    for (n, z) in buf.iter().enumerate() {
                        let t = (start + n) as f64 / 200.0;
                        let ph = TAU * hyp.df * t + PI * hyp.drift * t * t;
                        let y = Complex64::new(f64::from(z.re), f64::from(z.im))
                            * Complex64::from_polar(1.0, -ph);
                        for tone in 0..8 {
                            bins[tone] += y * tw[tone][n];
                        }
                    }
                    want += contrast(&bins, usize::from(tones[k])).unwrap();
                }
                assert!((hyp.metric - want).abs() < 1.0e-10 * want.abs().max(1.0));
            }
        }
    }

    #[test]
    fn tracked_no_gain() {
        let tones = tones(&cw(), &costas::ORIGINAL);
        let samples = signal(&tones, 2.0, 0.4, 0.0);
        let r = refine(&samples, &bases(2, -0.4), 32, 200.0, &tones, &[1.0; 79]);
        assert!(r.searched && !r.accepted());
        assert_eq!(r.best.df, 0.0);
        assert_eq!(r.best.drift, 0.0);
        assert!(r.best.dt.abs() <= 1);
        let samples = signal(&tones, 0.0, 0.0, 0.0);
        let mut wrong = tones;
        for j in 0..58 {
            wrong[data_idx(j)] = (wrong[data_idx(j)] + 4) % 8;
        }
        let r = refine(&samples, &bases(0, 0.0), 32, 200.0, &wrong, &[1.0; 79]);
        assert!(r.searched && !r.accepted());
    }

    #[test]
    fn bad_input() {
        let tones = tones(&cw(), &costas::ORIGINAL);
        let samples = signal(&tones, 0.0, 0.4, 0.0);
        let mut weights = [1.0; 79];
        weights[8] = f32::NAN;
        assert!(!refine(&samples, &bases(0, 0.0), 32, 200.0, &tones, &weights,).searched);
        assert!(
            !refine(
                &samples[..64],
                &bases(0, 0.0),
                32,
                200.0,
                &tones,
                &[1.0; 79],
            )
            .searched
        );
        assert!(
            !refine(
                &samples,
                &bases(0, f32::INFINITY),
                32,
                200.0,
                &tones,
                &[1.0; 79],
            )
            .searched
        );
        let bins = [Complex64::new(3.0, 0.0); 8];
        assert_eq!(contrast(&bins, 3), Some(0.0));
    }

    #[test]
    fn physical_midpoint() {
        let w = std::array::from_fn(|k| if is_pilot(k) { 1.0 } else { 0.0 });
        for df in [-0.4, 0.4] {
            for drift in [-0.02, 0.0, 0.02] {
                for fraction in [0.0, 0.4, 1.0] {
                    let mut b = bases(0, 0.0);
                    for b in &mut b {
                        let t = (b.start as f64 + 16.0) / 200.0;
                        b.hz = (-fraction * (df + drift * t)) as f32;
                    }
                    let r = mid_resid(
                        (1.0 - fraction) * df,
                        (1.0 - fraction) * drift,
                        &b,
                        32,
                        200.0,
                        &w,
                    )
                    .unwrap();
                    let mid = (64.0 + 39.0 * 32.0 + 16.0) / 200.0;
                    assert!((r - (df + drift * mid)).abs() < 1.0e-6);
                }
            }
        }
        assert!(mid_resid(0.0, 0.0, &bases(0, 0.0), 32, 200.0, &[0.0; 79]).is_none());
    }
}
