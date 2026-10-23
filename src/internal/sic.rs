// SPDX-License-Identifier: GPL-3.0-only

use super::consts::NN;
use num_complex::{Complex32, Complex64};
use std::f64::consts::TAU;

const SR: f64 = 12_000.0;
const ZERO: Complex32 = Complex32::new(0.0, 0.0);

pub(crate) fn start(dt: f32) -> isize {
    (f64::from(dt) * SR).round() as isize
}

fn cx(v: Complex32) -> Complex64 {
    Complex64::new(f64::from(v.re), f64::from(v.im))
}

fn corr(cref: &[Complex32], dd: &[f32], nsps: usize, st: isize) -> [Complex64; NN] {
    let mut c = [Complex64::new(0.0, 0.0); NN];
    for (sym, chunk) in cref.chunks(nsps).take(NN).enumerate() {
        let k = st + (sym * nsps) as isize;
        let ri = k.saturating_neg().max(0) as usize;
        let di = k.max(0) as usize;
        if ri >= chunk.len() || di >= dd.len() {
            continue;
        }
        let n = (chunk.len() - ri).min(dd.len() - di);
        for (&r, &d) in chunk[ri..ri + n].iter().zip(&dd[di..di + n]) {
            c[sym] += f64::from(d) * cx(r).conj();
        }
    }
    c
}

fn metric(cref: &[Complex32], dd: &[f32], nsps: usize, st: isize) -> f64 {
    corr(cref, dd, nsps, st)
        .iter()
        .map(Complex64::norm_sqr)
        .sum()
}

fn trmetric(
    cref: &[Complex32],
    dd: &[f32],
    tones: &[u8; NN],
    nsps: usize,
    st: isize,
) -> (f64, usize) {
    let win = (nsps / 8).clamp(16, 160) as isize;
    let mut pow = 0.0;
    let mut n = 0;
    for sym in 1..NN {
        if tones[sym] == tones[sym - 1] {
            continue;
        }
        let mut c = Complex64::new(0.0, 0.0);
        let mid = (sym * nsps) as isize;
        let lo = (mid - win).max(0).max(st.saturating_neg());
        let hi = (mid + win)
            .min(cref.len() as isize)
            .min((dd.len() as isize).saturating_sub(st));
        if lo < hi {
            let refs = &cref[lo as usize..hi as usize];
            let pcm = &dd[(st + lo) as usize..(st + hi) as usize];
            for (&r, &d) in refs.iter().zip(pcm) {
                c += f64::from(d) * cx(r).conj();
            }
        }
        pow += c.norm_sqr();
        n += 1;
    }
    (pow, n)
}

fn phase(cref: &mut [Complex32], freq: f64, drift: f64) {
    for (i, r) in cref.iter_mut().enumerate() {
        let t = i as f64 / SR;
        let ph = TAU * (freq * t + 0.5 * drift * t * t);
        *r *= Complex32::new(ph.cos() as f32, ph.sin() as f32);
    }
}

fn warp(cref: &[Complex32], off: f64, slope: f64) -> Vec<Complex32> {
    if cref.len() < 2 || !off.is_finite() || !slope.is_finite() {
        return cref.to_vec();
    }
    let mut ph = Vec::with_capacity(cref.len());
    let mut p = f64::from(cref[0].arg());
    if !cref[0].re.is_finite() || !cref[0].im.is_finite() || !p.is_finite() {
        return cref.to_vec();
    }
    ph.push(p);
    for i in 1..cref.len() {
        let step = f64::from((cref[i] * cref[i - 1].conj()).arg());
        if !cref[i].re.is_finite() || !cref[i].im.is_finite() || !step.is_finite() {
            return cref.to_vec();
        }
        p += step;
        ph.push(p);
    }
    let mut out = vec![ZERO; cref.len()];
    for (i, r) in out.iter_mut().enumerate() {
        let src = i as f64 - (off + slope * i as f64 / SR);
        if !(0.0..=(cref.len() - 1) as f64).contains(&src) {
            continue;
        }
        let a = src as usize;
        let b = (a + 1).min(cref.len() - 1);
        let f = src - a as f64;
        if f == 0.0 {
            *r = cref[a];
        } else {
            let p = ph[a] * (1.0 - f) + ph[b] * f;
            let mag = f64::from(cref[a].norm()) * (1.0 - f) + f64::from(cref[b].norm()) * f;
            *r = Complex32::from_polar(mag as f32, p as f32);
        }
    }
    out
}

fn wrap(ph: f64) -> f64 {
    (ph + TAU / 2.0).rem_euclid(TAU) - TAU / 2.0
}

fn solve3(mut a: [[f64; 4]; 3]) -> Option<[f64; 3]> {
    let scale = a
        .iter()
        .flat_map(|r| r[..3].iter())
        .fold(0.0_f64, |s, v| s.max(v.abs()));
    if scale <= 0.0 || !scale.is_finite() {
        return None;
    }
    for col in 0..3 {
        let mut pivot = col;
        for row in col + 1..3 {
            if a[row][col].abs() > a[pivot][col].abs() {
                pivot = row;
            }
        }
        a.swap(col, pivot);
        if a[col][col].abs() <= 1.0e-12 * scale {
            return None;
        }
        let pivot = a[col];
        for (row, vals) in a.iter_mut().enumerate() {
            if row == col {
                continue;
            }
            let f = vals[col] / pivot[col];
            for (v, p) in vals[col..].iter_mut().zip(&pivot[col..]) {
                *v -= f * p;
            }
        }
    }
    let x = std::array::from_fn(|i| a[i][3] / a[i][i]);
    x.iter().all(|v| v.is_finite()).then_some(x)
}

struct Pair {
    x: [f64; 3],
    ph: f64,
    wt: f64,
    t: f64,
    reg: usize,
}

#[derive(Clone, Copy, Default)]
struct Reg {
    n: usize,
    num: f64,
    den: f64,
    t: f64,
}

fn timing(
    cref: &[Complex32],
    dd: &[f32],
    tones: &[u8; NN],
    nsps: usize,
    st: isize,
    delta: isize,
) -> Option<(f64, f64)> {
    if cref.len() != NN * nsps {
        return None;
    }
    let mut freq = [None; NN];
    for (sym, chunk) in cref.chunks(nsps).enumerate() {
        let margin = chunk.len() / 4;
        let mid = if chunk.len() - 2 * margin > 1 {
            &chunk[margin..chunk.len() - margin]
        } else {
            chunk
        };
        let sum: Complex64 = mid.windows(2).map(|v| cx(v[1]) * cx(v[0]).conj()).sum();
        if sum.norm() > 0.0 && sum.re.is_finite() && sum.im.is_finite() {
            freq[sym] = Some(sum.arg());
        }
    }
    let c = corr(cref, dd, nsps, st);
    let mag = c.map(|v| v.norm());
    let mut sorted = mag;
    sorted.sort_unstable_by(f64::total_cmp);
    let med = sorted[NN / 2];
    if !med.is_finite() || med <= f64::EPSILON {
        return None;
    }
    let mut pairs = Vec::with_capacity(NN);
    for sym in 0..NN - 1 {
        if tones[sym] == tones[sym + 1] || mag[sym] < 0.25 * med || mag[sym + 1] < 0.25 * med {
            continue;
        }
        let (Some(f0), Some(f1)) = (freq[sym], freq[sym + 1]) else {
            continue;
        };
        let wt = mag[sym].min(mag[sym + 1]).min(4.0 * med);
        let t0 = (sym as f64 + 0.5) * nsps as f64 / SR;
        let t1 = (sym as f64 + 1.5) * nsps as f64 / SR;
        let g = -(f1 - f0);
        let ph = (c[sym + 1] * c[sym].conj()).arg();
        if wt <= 0.0 || !wt.is_finite() || g.abs() <= 1.0e-9 || !ph.is_finite() {
            continue;
        }
        pairs.push(Pair {
            x: [t1 - t0, g, -(f1 * t1 - f0 * t0)],
            ph,
            wt,
            t: t0.midpoint(t1),
            reg: (6 * (2 * sym + 1) / (2 * NN)).min(5),
        });
    }
    let mut fit = [0.0; 3];
    let err = |p: &Pair, fit: &[f64; 3]| {
        wrap(p.ph - (fit[0] * p.x[0] + fit[1] * p.x[1] + fit[2] * p.x[2]))
    };
    for _ in 0..8 {
        let mut a = [[0.0; 4]; 3];
        for p in &pairs {
            let e = err(p, &fit);
            for (row, a) in a.iter_mut().enumerate() {
                a[3] += p.wt * p.x[row] * e;
                for (col, v) in a[..3].iter_mut().enumerate() {
                    *v += p.wt * p.x[row] * p.x[col];
                }
            }
        }
        let step = solve3(a)?;
        for i in 0..3 {
            fit[i] += step[i];
        }
    }
    let mut regs = [Reg::default(); 6];
    for p in &pairs {
        let r = &mut regs[p.reg];
        r.n += 1;
        r.num += p.wt * p.x[1] * err(p, &fit);
        r.den += p.wt * p.x[1] * p.x[1];
        r.t += p.wt * p.x[1] * p.x[1] * p.t;
    }
    let mut pts = Vec::with_capacity(6);
    for r in regs {
        if r.n < 2 || r.den <= 0.0 || !r.num.is_finite() || !r.den.is_finite() || !r.t.is_finite() {
            continue;
        }
        let t = r.t / r.den;
        pts.push((t, fit[1] + fit[2] * t + r.num / r.den, r.den));
    }
    if pts.len() < 4 {
        return None;
    }
    let (mut sw, mut swt, mut swtt, mut swy, mut swty) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(t, y, w) in &pts {
        sw += w;
        swt += w * t;
        swtt += w * t * t;
        swy += w * y;
        swty += w * t * y;
    }
    let det = sw * swtt - swt * swt;
    let scale = 1.0_f64.max(sw.abs()).max(swt.abs()).max(swtt.abs());
    if !det.is_finite() || det.abs() <= 1.0e-12 * scale * scale {
        return None;
    }
    let off = (swy * swtt - swt * swty) / det;
    let slope = (sw * swty - swt * swy) / det;
    let rss: f64 = pts
        .iter()
        .map(|&(t, y, w)| w * (y - off - slope * t).powi(2))
        .sum();
    let rms = (rss / sw).sqrt();
    let spread = swtt - swt * swt / sw;
    let sd = (rss / (pts.len() - 2) as f64 / spread).sqrt();
    let sig = (sd > 0.0 && slope.abs() >= 4.0 * sd) || (sd == 0.0 && slope != 0.0);
    let off = delta as f64 + off;
    let bound = if delta == 0 {
        1.0
    } else {
        (nsps / 16).clamp(8, 120) as f64
    };
    if off.is_finite()
        && slope.is_finite()
        && rms.is_finite()
        && off.abs() <= bound
        && (slope * (NN * nsps) as f64 / SR).abs() <= 2.0_f64.min(0.5 * nsps as f64)
        && rms <= 0.05
        && fit[0].abs() <= TAU
        && sig
    {
        Some((off, slope))
    } else {
        None
    }
}

pub(crate) fn refine(
    orig: Vec<Complex32>,
    dd: &[f32],
    tones: &[u8; NN],
    nsps: usize,
    dt: f32,
) -> Vec<Complex32> {
    let st = start(dt);
    let nom = metric(&orig, dd, nsps, st);
    if !nom.is_finite() || nom <= f64::EPSILON {
        return orig;
    }
    let mut cref = orig.clone();
    let (trnom, ntr) = trmetric(&cref, dd, tones, nsps, st);
    let mut best = nom;
    let mut delta = 0;
    let radius = (nsps / 16).clamp(8, 120) as isize;
    if ntr >= 6 && trnom > 0.0 {
        let mut trbest = trnom;
        for d in (-radius..=radius).step_by(4) {
            let (m, _) = trmetric(&cref, dd, tones, nsps, st + d);
            if m > trbest {
                trbest = m;
                delta = d;
            }
        }
        for d in (-radius).max(delta - 3)..=radius.min(delta + 3) {
            let (m, _) = trmetric(&cref, dd, tones, nsps, st + d);
            if m > trbest {
                trbest = m;
                delta = d;
            }
        }
        let m = metric(&cref, dd, nsps, st + delta);
        if trbest >= trnom * 1.005 && m.is_finite() && m >= nom * 0.999 {
            best = m;
        } else {
            delta = 0;
        }
    }
    let c = corr(&cref, dd, nsps, st + delta);
    let mag = c.map(|v| v.norm());
    let mut sorted = mag;
    sorted.sort_unstable_by(f64::total_cmp);
    let med = sorted[NN / 2];
    if med.is_finite() && med > f64::EPSILON {
        let secs = nsps as f64 / SR;
        let mut pairs = Vec::with_capacity(NN);
        let (mut sw, mut swt, mut swtt, mut swy, mut swty) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for k in 0..NN - 1 {
            if mag[k] < 0.25 * med || mag[k + 1] < 0.25 * med {
                continue;
            }
            let y = (c[k + 1] * c[k].conj()).arg() / (TAU * secs);
            let t = (k + 1) as f64 * secs;
            let w = mag[k].min(mag[k + 1]).min(4.0 * med);
            pairs.push((t, y, w));
            sw += w;
            swt += w * t;
            swtt += w * t * t;
            swy += w * y;
            swty += w * t * y;
        }
        if pairs.len() >= 12 && sw > 0.0 {
            let mean = swy / sw;
            let det = sw * swtt - swt * swt;
            let (freq, drift) = if det.abs() > f64::EPSILON {
                (
                    (swy * swtt - swt * swty) / det,
                    (sw * swty - swt * swy) / det,
                )
            } else {
                (mean, 0.0)
            };
            let rms = (pairs
                .iter()
                .map(|&(t, y, w)| w * (y - freq - drift * t).powi(2))
                .sum::<f64>()
                / sw)
                .sqrt();
            let valid = freq.is_finite()
                && drift.is_finite()
                && rms.is_finite()
                && freq.abs() <= 1.0
                && drift.abs() <= 0.15
                && drift.abs() * NN as f64 * secs <= 1.0
                && rms <= 1.0;
            let mut chosen = None;
            for (f, d, ok) in [
                (mean, 0.0, mean.is_finite() && mean.abs() <= 1.0),
                (freq, drift, valid),
            ] {
                if !ok {
                    continue;
                }
                let mut cand = cref.clone();
                phase(&mut cand, f, d);
                let m = metric(&cand, dd, nsps, st + delta);
                if m.is_finite() && m >= best * 1.0001 {
                    chosen = Some(cand);
                    best = m;
                }
            }
            if let Some(cand) = chosen {
                cref = cand;
            }
        }
    }
    let whole = metric(&cref, dd, nsps, st + delta);
    let (tr, ntr) = trmetric(&cref, dd, tones, nsps, st + delta);
    let mut warped = false;
    if whole.is_finite()
        && whole > 0.0
        && tr.is_finite()
        && tr > 0.0
        && ntr >= 6
        && let Some((off, slope)) = timing(&cref, dd, tones, nsps, st + delta, delta)
    {
        let cand = warp(&cref, off, slope);
        let (tw, _) = trmetric(&cand, dd, tones, nsps, st);
        let w = metric(&cand, dd, nsps, st);
        if tw.is_finite() && tw > tr && w.is_finite() && w >= whole * 0.999 {
            cref = cand;
            warped = true;
        }
    }
    if !warped && delta != 0 {
        let n = delta.unsigned_abs().min(cref.len());
        if delta > 0 {
            cref.rotate_right(n);
            cref[..n].fill(ZERO);
        } else {
            cref.rotate_left(n);
            let len = cref.len();
            cref[len - n..].fill(ZERO);
        }
    }
    let m = metric(&cref, dd, nsps, st);
    if !m.is_finite() || m < nom * 0.999 {
        orig
    } else {
        cref
    }
}

pub(crate) fn subtract(
    dd: &mut [f32],
    cref: &[Complex32],
    lpf: &[Complex32],
    nsps: usize,
    dt: f32,
    alt: &mut Option<Vec<f32>>,
) {
    let st = start(dt);
    let ri = if st < 0 { st.unsigned_abs() } else { 0 };
    let di = st.max(0) as usize;
    if ri >= cref.len() || di >= dd.len() {
        return;
    }
    let n = (cref.len() - ri).min(dd.len() - di).min(lpf.len());
    if n == 0 {
        return;
    }
    let win = 2 * nsps;
    let mut coeff = Vec::with_capacity(n.div_ceil(win));
    let mut centers = Vec::with_capacity(n.div_ceil(win));
    for b in (0..n).step_by(win) {
        let e = (b + win).min(n);
        let sum: Complex64 = (b..e).map(|i| cx(dd[di + i] * cref[ri + i].conj())).sum();
        let v = sum / (e - b) as f64;
        coeff.push(Complex32::new(v.re as f32, v.im as f32));
        centers.push(b as f64 + 0.5 * (e - b - 1) as f64);
    }
    let smooth: Vec<Complex32> = (0..coeff.len())
        .map(|i| {
            let mut sum = Complex64::new(0.0, 0.0);
            let mut wt = 0.0;
            for (j, c) in coeff
                .iter()
                .enumerate()
                .take((i + 3).min(coeff.len()))
                .skip(i.saturating_sub(2))
            {
                let w = 1.0 - i.abs_diff(j) as f64 * 0.25;
                sum += cx(*c) * w;
                wt += w;
            }
            Complex32::new((sum.re / wt) as f32, (sum.im / wt) as f32)
        })
        .collect();
    let mut seg = Vec::with_capacity(n);
    let mut j = 0;
    for i in 0..n {
        let t = i as f64;
        while j + 1 < centers.len() && t > centers[j + 1] {
            j += 1;
        }
        let c = if j + 1 == centers.len() || t <= centers[j] {
            smooth[j]
        } else {
            let f = (t - centers[j]) / (centers[j + 1] - centers[j]);
            let v = cx(smooth[j]) * (1.0 - f) + cx(smooth[j + 1]) * f;
            Complex32::new(v.re as f32, v.im as f32)
        };
        seg.push(c);
    }
    let mut before = [Complex64::new(0.0, 0.0); NN];
    let mut after_lpf = before;
    let mut after_seg = before;
    let mut i = 0;
    while i < n {
        let sym = (ri + i) / nsps;
        let end = ((sym + 1) * nsps - ri).min(n);
        for j in i..end {
            let r = cref[ri + j];
            before[sym] += f64::from(dd[di + j]) * cx(r).conj();
            after_lpf[sym] += f64::from(dd[di + j] - 2.0 * (lpf[j] * r).re) * cx(r).conj();
            after_seg[sym] += f64::from(dd[di + j] - 2.0 * (seg[j] * r).re) * cx(r).conj();
        }
        i = end;
    }
    let before: f64 = before.iter().map(Complex64::norm_sqr).sum();
    let a: f64 = after_lpf.iter().map(Complex64::norm_sqr).sum();
    let b: f64 = after_seg.iter().map(Complex64::norm_sqr).sum();
    let use_seg = a.is_finite() && a > 0.0 && b.is_finite() && b > 0.0 && b < a * 0.999;
    let after = if use_seg { b } else { a };
    if !before.is_finite() || before <= f64::EPSILON || !after.is_finite() || after >= before {
        return;
    }
    // Less decoded-signal residue can also mean erasing a neighboring frame's pilots.
    if !use_seg && alt.is_none() && b.is_finite() && b < before {
        let mut saved = dd.to_vec();
        for i in 0..n {
            saved[di + i] -= 2.0 * (seg[i] * cref[ri + i]).re;
        }
        *alt = Some(saved);
    }
    let chan = if use_seg { seg.as_slice() } else { lpf };
    for i in 0..n {
        dd[di + i] -= 2.0 * (chan[i] * cref[ri + i]).re;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corr_clip() {
        for nsps in [1, 7, 384] {
            let (tones, cref, _) = wave(nsps);
            for len in [0, 13, 777, cref.len() + 100] {
                let dd: Vec<_> = (0..len).map(|i| (i as f32 * 0.31).sin()).collect();
                for st in [-200, -19, 0, 23, 800] {
                    let mut want = [Complex64::new(0.0, 0.0); NN];
                    for (i, &r) in cref.iter().enumerate() {
                        let k = st + i as isize;
                        if k >= 0 && (k as usize) < dd.len() {
                            want[i / nsps] += f64::from(dd[k as usize]) * cx(r).conj();
                        }
                    }
                    assert_eq!(corr(&cref, &dd, nsps, st), want);

                    let win = (nsps / 8).clamp(16, 160) as isize;
                    let mut pow = 0.0;
                    let mut n = 0;
                    for sym in 1..NN {
                        if tones[sym] == tones[sym - 1] {
                            continue;
                        }
                        let mut c = Complex64::new(0.0, 0.0);
                        for off in -win..win {
                            let i = (sym * nsps) as isize + off;
                            let k = st + i;
                            if i >= 0
                                && (i as usize) < cref.len()
                                && k >= 0
                                && (k as usize) < dd.len()
                            {
                                c += f64::from(dd[k as usize]) * cx(cref[i as usize]).conj();
                            }
                        }
                        pow += c.norm_sqr();
                        n += 1;
                    }
                    assert_eq!(trmetric(&cref, &dd, &tones, nsps, st), (pow, n));
                }
            }
        }
    }

    fn wave(nsps: usize) -> ([u8; NN], Vec<Complex32>, Vec<f64>) {
        let tones = std::array::from_fn(|i| ((3 * i + 1) % 8) as u8);
        let mut cref = Vec::with_capacity(NN * nsps);
        let mut ph = Vec::with_capacity(NN * nsps);
        let mut p = 0.0;
        for tone in tones {
            let step = TAU * 1000.0 / SR + TAU * f64::from(tone) / nsps as f64;
            for _ in 0..nsps {
                ph.push(p);
                cref.push(Complex32::from_polar(1.0, (p % TAU) as f32));
                p += step;
            }
        }
        (tones, cref, ph)
    }

    fn samples(ph: &[f64], slope: f64) -> Vec<f32> {
        (0..ph.len())
            .map(|i| {
                let src = (i as f64 - slope * i as f64 / SR).clamp(0.0, (ph.len() - 1) as f64);
                let a = src as usize;
                let b = (a + 1).min(ph.len() - 1);
                let f = src - a as f64;
                (ph[a] * (1.0 - f) + ph[b] * f).cos() as f32
            })
            .collect()
    }

    #[test]
    fn st_grids() {
        for rate in [100, 200, 500, 1000] {
            for i in -6000..=6000 {
                let dt = i as f32 * (1.0 / rate as f32);
                assert_eq!(start(dt), i * (12_000 / rate));
            }
        }
        assert_eq!(start(0.75 / 12_000.0), 1);
        assert_eq!(start(-0.75 / 12_000.0), -1);
    }

    #[test]
    fn warp_phase_edges() {
        let (_, cref, ph) = wave(1920);
        assert_eq!(warp(&cref, 0.0, 0.0), cref);
        for off in [-0.75, 0.75] {
            let out = warp(&cref, off, 0.0);
            assert_eq!(out.len(), cref.len());
            assert!(out.iter().all(|v| v.re.is_finite() && v.im.is_finite()));
            let i = 5 * 1920 + 960;
            let src = i as f64 - off;
            let a = src as usize;
            let f = src - a as f64;
            let want = ph[a] * (1.0 - f) + ph[a + 1] * f;
            assert!(wrap(f64::from(out[i].arg()) - want).abs() < 1.0e-3);
            assert!((out[i].norm() - 1.0).abs() < 1.0e-6);
            assert_eq!(out[if off > 0.0 { 0 } else { out.len() - 1 }], ZERO);
        }
    }

    #[test]
    fn timing_signed_drift() {
        let (tones, cref, ph) = wave(1920);
        for slope in [-0.15, 0.15] {
            let dd = samples(&ph, slope);
            let (_, got) = timing(&cref, &dd, &tones, 1920, 0, 0).expect("bounded timing fit");
            assert!(got * slope > 0.0);
            assert!((got - slope).abs() < 0.06, "slope {slope}, got {got}");
            let out = refine(cref.clone(), &dd, &tones, 1920, 0.0);
            assert_ne!(out, cref);
            assert!(metric(&out, &dd, 1920, 0) >= metric(&cref, &dd, 1920, 0) * 0.999);
            assert!(out.iter().all(|v| v.re.is_finite() && v.im.is_finite()));
        }
    }

    #[test]
    fn timing_rejects_bad_fits() {
        let (tones, cref, ph) = wave(1920);
        assert!(timing(&cref, &samples(&ph, 0.4), &tones, 1920, 0, 0).is_none());
        assert!(timing(&cref, &samples(&ph, 0.15), &[0; NN], 1920, 0, 0).is_none());
        let dd = samples(&ph, 0.0);
        let out = refine(cref.clone(), &dd, &tones, 1920, 0.0);
        assert_eq!(out, cref);
        assert_eq!(
            refine(cref.clone(), &vec![0.0; cref.len()], &tones, 1920, 0.0),
            cref
        );
    }

    #[test]
    fn refine_freq_dt() {
        let (tones, cref, ph) = wave(384);
        let mut dd = vec![0.0; 23 + cref.len()];
        for (i, p) in ph.into_iter().enumerate() {
            let t = i as f64 / SR;
            dd[23 + i] = (p + TAU * (0.5 * t + 0.02 * t * t)).cos() as f32;
        }
        let out = refine(cref.clone(), &dd, &tones, 384, 0.0);
        assert!(metric(&out, &dd, 384, 0) > metric(&cref, &dd, 384, 0) * 1.001);
        assert_eq!(out[0], ZERO);
    }

    #[test]
    fn sub_clips_both_ends() {
        let (_, cref, _) = wave(384);
        for st in [-193_isize, 197] {
            let ri = if st < 0 { st.unsigned_abs() } else { 0 };
            let di = st.max(0) as usize;
            let n = 777;
            let mut dd = vec![0.0; n + di];
            for i in 0..n {
                dd[di + i] = cref[ri + i].re;
            }
            subtract(
                &mut dd,
                &cref,
                &vec![Complex32::new(0.5, 0.0); n],
                384,
                st as f32 / 12_000.0,
                &mut None,
            );
            assert!(dd.iter().all(|&v| v == 0.0));
        }
    }

    #[test]
    fn sub_rejects_nonfinite() {
        let (_, cref, _) = wave(384);
        let mut dd: Vec<_> = cref.iter().map(|v| v.re).collect();
        let orig = dd.clone();
        subtract(
            &mut dd,
            &cref,
            &vec![Complex32::new(f32::NAN, 0.0); cref.len()],
            384,
            0.0,
            &mut None,
        );
        assert_eq!(dd, orig);
        subtract(&mut dd, &cref, &[], 384, 0.0, &mut None);
        assert_eq!(dd, orig);
        for dt in [-50.0, 50.0] {
            subtract(&mut dd, &cref, &vec![ZERO; cref.len()], 384, dt, &mut None);
            assert_eq!(dd, orig);
        }
    }

    #[test]
    fn sub_uses_seg_chan() {
        let (_, cref, _) = wave(384);
        let mut dd: Vec<_> = cref.iter().map(|v| v.re).collect();
        let before = metric(&cref, &dd, 384, 0);
        subtract(&mut dd, &cref, &vec![ZERO; cref.len()], 384, 0.0, &mut None);
        assert!(metric(&cref, &dd, 384, 0) < before * 1.0e-5);
    }

    #[test]
    fn sub_keeps_one_alt() {
        let (_, cref, _) = wave(384);
        let mut dd: Vec<_> = cref.iter().map(|v| v.re).collect();
        let before = metric(&cref, &dd, 384, 0);
        let mut alt = None;
        subtract(
            &mut dd,
            &cref,
            &vec![Complex32::new(0.5, 0.0); cref.len()],
            384,
            0.0,
            &mut alt,
        );
        assert!(dd.iter().all(|&v| v == 0.0));
        let saved = alt.as_ref().expect("valid segmented residual");
        assert_eq!(saved.len(), dd.len());
        assert!(metric(&cref, saved, 384, 0) < before);
        dd = cref.iter().map(|v| v.re * 2.0).collect();
        let orig = alt.clone();
        subtract(
            &mut dd,
            &cref,
            &vec![Complex32::new(1.0, 0.0); cref.len()],
            384,
            0.0,
            &mut alt,
        );
        assert_eq!(alt, orig);
    }
}
