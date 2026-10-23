// SPDX-License-Identifier: GPL-3.0-only

use std::f64::consts::{PI, TAU};

use num_complex::{Complex32, Complex64};

use super::whitening_processor::Blend;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Pilot {
    pub time: f64,
    pub val: Complex64,
    pub tone: usize,
    pub sym: usize,
    pub shift: f64,
    pub hz: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Data {
    pub time: f64,
    pub shift: f64,
    pub hz: f64,
    pub bins: [Complex32; 8],
}

struct Fit {
    tref: f64,
    phi: f64,
    df: f64,
    drift: f64,
    amp: f64,
    rms: f64,
}

impl Fit {
    fn phase(&self, time: f64) -> f64 {
        let dt = time - self.tref;
        self.phi + TAU * self.df * dt + PI * self.drift * dt * dt
    }

    fn alpha(&self) -> f64 {
        if !self.rms.is_finite() {
            return 0.0;
        }
        ((0.60 - self.rms) / (0.60 - 0.20)).clamp(0.0, 1.0)
    }
}

fn wrap(phi: f64) -> f64 {
    (phi + PI).rem_euclid(TAU) - PI
}

fn finite(z: Complex64) -> bool {
    z.re.is_finite() && z.im.is_finite()
}

fn rot(phi: f64) -> Complex64 {
    Complex64::from_polar(1.0, phi)
}

fn time(base: f64, shift: f64, fs: f64) -> Option<f64> {
    let t = base + shift / fs;
    (base.is_finite() && shift.is_finite() && fs.is_finite() && fs > 0.0 && t.is_finite())
        .then_some(t)
}

fn trk_rot(hz: f64, fs: f64, win: usize) -> Option<Complex64> {
    if !hz.is_finite() || !fs.is_finite() || fs <= 0.0 || win == 0 {
        return None;
    }
    Some(rot(-TAU * hz / fs * (win as f64 + 1.0) * 0.5))
}

fn norm_tmg(z: Complex64, tone: usize, shift: f64, win: usize) -> Option<Complex64> {
    if !finite(z) || !shift.is_finite() || win == 0 || tone >= win {
        return None;
    }
    let z = z * rot(-TAU * tone as f64 * shift / win as f64);
    finite(z).then_some(z)
}

fn solve(rows: impl Iterator<Item = (f64, f64, f64, f64)>) -> Option<(f64, f64)> {
    let (mut s00, mut s01, mut s11, mut s0y, mut s1y) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (x0, x1, y, w) in rows {
        if !x0.is_finite() || !x1.is_finite() || !y.is_finite() || !w.is_finite() || w <= 0.0 {
            return None;
        }
        s00 += w * x0 * x0;
        s01 += w * x0 * x1;
        s11 += w * x1 * x1;
        s0y += w * x0 * y;
        s1y += w * x1 * y;
    }
    let det = s00 * s11 - s01 * s01;
    let scale = 1.0_f64.max(s00.abs()).max(s01.abs()).max(s11.abs());
    if !det.is_finite() || det.abs() <= 1.0e-12 * scale * scale {
        return None;
    }
    let a = (s11 * s0y - s01 * s1y) / det;
    let b = (s00 * s1y - s01 * s0y) / det;
    (a.is_finite() && b.is_finite()).then_some((a, b))
}

fn fit(pilots: &[Pilot], min_pilots: usize, min_span: f64, win: usize, fs: f64) -> Option<Fit> {
    let mut valid: Vec<_> = pilots
        .iter()
        .filter_map(|p| {
            let t = time(p.time, p.shift, fs)?;
            let z = norm_tmg(p.val * trk_rot(p.hz, fs, win)?, p.tone, p.shift, win)?;
            (z.norm() > 0.0).then_some((t, z, p.sym, 0.0))
        })
        .collect();
    if min_pilots == 0 || valid.len() < min_pilots || !min_span.is_finite() || min_span <= 0.0 {
        return None;
    }
    valid.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let span = valid.last()?.0 - valid[0].0;
    if !span.is_finite() || span < min_span {
        return None;
    }
    let mut mags: Vec<_> = valid.iter().map(|p| p.1.norm()).collect();
    let mid = mags.len() / 2;
    let amp = *mags.select_nth_unstable_by(mid, f64::total_cmp).1;
    if !amp.is_finite() || amp <= 0.0 {
        return None;
    }
    let (mut sw, mut st) = (0.0, 0.0);
    for p in &mut valid {
        p.3 = (p.1.norm() / amp).min(1.0);
        if !p.3.is_finite() || p.3 <= 0.0 {
            return None;
        }
        sw += p.3;
        st += p.3 * p.0;
    }
    if !sw.is_finite() || sw <= 0.0 {
        return None;
    }
    let tref = st / sw;
    // Adjacent pilots avoid phase unwrapping across the gaps between Costas blocks.
    let (mut df, mut drift) = solve(valid.windows(2).filter_map(|p| {
        if p[0].2.checked_add(1) != Some(p[1].2) {
            return None;
        }
        let t0 = p[0].0 - tref;
        let t1 = p[1].0 - tref;
        Some((
            TAU * (t1 - t0),
            PI * (t1 * t1 - t0 * t0),
            wrap((p[1].1 * p[0].1.conj()).arg()),
            p[0].3.min(p[1].3),
        ))
    }))?;

    let mut phi = 0.0;
    let mut prev_phi = 0.0;
    let mut phases = vec![0.0; valid.len()];
    for _ in 0..2 {
        let mut prev = 0.0;
        let mut mean = Complex64::new(0.0, 0.0);
        for (i, p) in valid.iter().enumerate() {
            let dt = p.0 - tref;
            let phase = (p.1 * rot(-(TAU * df * dt + PI * drift * dt * dt))).arg();
            if !phase.is_finite() {
                return None;
            }
            let phase = if i == 0 {
                phase
            } else {
                prev + wrap(phase - prev)
            };
            phases[i] = phase;
            prev = phase;
            mean += p.3 * rot(phase);
        }
        if !finite(mean) || mean.norm() <= 0.0 {
            return None;
        }
        phi = mean.arg();
        let (step_f, step_d) = solve(valid.iter().zip(&phases).map(|(p, phase)| {
            let dt = p.0 - tref;
            (TAU * dt, PI * dt * dt, wrap(*phase - phi), p.3)
        }))?;
        df += step_f;
        drift += step_d;
        if step_f.abs() < 1.0e-12 && step_d.abs() < 1.0e-12 && (phi - prev_phi).abs() < 1.0e-12 {
            break;
        }
        prev_phi = phi;
    }
    if df.abs() >= 1.0 || drift.abs() >= 0.05 {
        return None;
    }
    let mut fit = Fit {
        tref,
        phi,
        df,
        drift,
        amp,
        rms: 0.0,
    };
    let mut err = 0.0;
    for p in &valid {
        let resid = wrap(p.1.arg() - fit.phase(p.0));
        if !resid.is_finite() {
            return None;
        }
        err += p.3 * resid * resid;
    }
    fit.rms = (err / sw).sqrt();
    Some(fit)
}

pub(crate) fn scores<const D: usize>(
    pilots: &[Pilot],
    data: &[Data; D],
    min_pilots: usize,
    min_span: f64,
    win: usize,
    fs: f64,
) -> Option<Blend<8, D>> {
    let fit = fit(pilots, min_pilots, min_span, win, fs)?;
    let alpha = fit.alpha() as f32;
    if alpha <= 0.0 {
        return None;
    }
    let mut out = Blend {
        nums: [[0.0; D]; 8],
        amp: fit.amp,
        alpha,
    };
    for (j, sym) in data.iter().enumerate() {
        let phase = fit.phase(time(sym.time, sym.shift, fs)?);
        if !phase.is_finite() {
            return None;
        }
        let trk = trk_rot(sym.hz, fs, win)?;
        let carrier = rot(-phase);
        for (tone, bin) in sym.bins.iter().enumerate() {
            let bin = Complex64::new(f64::from(bin.re), f64::from(bin.im));
            let bin = norm_tmg(bin * trk, tone, sym.shift, win)?;
            let proj = (bin * carrier).re;
            let num = (fit.amp * proj - 0.5 * fit.amp * fit.amp) as f32;
            if !num.is_finite() {
                return None;
            }
            out.nums[tone][j] = f64::from(num);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::internal::kalman::FrequencyTracker;

    fn pilots(phi: f64, df: f64, drift: f64, secs: f64, amp: f64) -> Vec<Pilot> {
        (0..3)
            .flat_map(|b| (0..7).map(move |c| b * 36 + c))
            .map(|sym| {
                let time = sym as f64 * secs;
                Pilot {
                    time,
                    val: amp * rot(phi + TAU * df * time + PI * drift * time * time),
                    tone: sym % 8,
                    sym,
                    shift: 0.0,
                    hz: 0.0,
                }
            })
            .collect()
    }

    #[test]
    fn fit_modes() {
        for (win, fs) in [
            (12, 375.0),
            (12, 240.0),
            (20, 200.0),
            (32, 200.0),
            (32, 100.0),
        ] {
            let secs = win as f64 / fs;
            for phi in [-2.8, -0.7, 0.0, 1.9] {
                for df in [-0.25, 0.0, 0.25] {
                    for drift in [-0.02, 0.0, 0.02] {
                        let p = pilots(phi, df, drift, secs, 2.0);
                        let f = fit(&p, 12, 39.0 * secs, win, fs).unwrap();
                        assert!((f.df - (df + drift * f.tref)).abs() < 1.0e-8);
                        assert!((f.drift - drift).abs() < 1.0e-8);
                        assert!(wrap(f.phase(0.0) - phi).abs() < 1.0e-8);
                        assert_eq!(f.alpha(), 1.0);
                        assert!((f.amp - 2.0).abs() < 1.0e-10);
                    }
                }
            }
        }
    }

    #[test]
    fn tone_scale() {
        let p = pilots(0.0, 0.0, 0.0, 0.16, 2.3);
        let data = std::array::from_fn::<_, 58, _>(|j| {
            let mut d = Data {
                time: (j + 7) as f64 * 0.16,
                ..Data::default()
            };
            d.bins[j % 8] = Complex32::from_polar(2.3, 0.4);
            d
        });
        let s = scores(&p, &data, 12, 5.0, 32, 200.0).unwrap();
        assert_eq!(s.alpha, 1.0);
        for j in 0..58 {
            for tone in 0..8 {
                let expected = if tone == j % 8 {
                    2.3 * 2.3 * 0.4_f64.cos() - 0.5 * 2.3 * 2.3
                } else {
                    -0.5 * 2.3 * 2.3
                };
                assert!((s.nums[tone][j] - expected).abs() < 1.0e-5);
            }
        }
    }

    #[test]
    fn reject_bad_fit() {
        let p = pilots(0.7, 0.0, 0.0, 0.16, 2.0);
        assert!(fit(&p[..7], 12, 5.0, 32, 200.0).is_none());
        assert!(fit(&p, 12, 20.0, 32, 200.0).is_none());
        assert!(fit(&p, 12, 5.0, 0, 200.0).is_none());
        assert!(fit(&p, 12, 5.0, 32, f64::INFINITY).is_none());
        assert!(fit(&pilots(0.0, 1.1, 0.0, 0.16, 2.0), 12, 5.0, 32, 200.0).is_none());
        let mut bad = p.clone();
        for (i, p) in bad.iter_mut().enumerate() {
            p.val = rot((i as f64 * 21.37).sin() * PI);
        }
        assert!(scores(&bad, &[Data::default()], 12, 5.0, 32, 200.0).is_none());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut data = [Data::default()];
            data[0].bins[3].re = bad;
            assert!(scores(&p, &data, 12, 5.0, 32, 200.0).is_none());
            data[0] = Data {
                hz: f64::from(bad),
                ..Data::default()
            };
            assert!(scores(&p, &data, 12, 5.0, 32, 200.0).is_none());
        }
        for rms in [0.0, 0.2, 0.4, 0.6, f64::NAN] {
            let mut f = fit(&p, 12, 5.0, 32, 200.0).unwrap();
            f.rms = rms;
            let expected = if rms <= 0.2 {
                1.0
            } else if rms == 0.4 {
                0.5
            } else {
                0.0
            };
            assert!((f.alpha() - expected).abs() < 1.0e-10);
        }
    }

    fn bin(samples: &[Complex32], tone: usize) -> Complex64 {
        samples
            .iter()
            .enumerate()
            .map(|(n, z)| {
                Complex64::new(f64::from(z.re), f64::from(z.im))
                    * rot(-TAU * (tone * n) as f64 / samples.len() as f64)
            })
            .sum()
    }

    #[test]
    fn trk_tmg_phase() {
        for hz in [-2.5, 2.5] {
            for tone in [0, 3, 7] {
                for shift in [-2, -1, 0, 1, 2] {
                    let mut samples: Vec<_> = (0..32)
                        .map(|n| {
                            Complex32::from_polar(
                                1.0,
                                (0.7 + TAU * tone as f64 * f64::from(n + shift) / 32.0) as f32,
                            )
                        })
                        .collect();
                    let mut trk = FrequencyTracker::new();
                    trk.reset_default(hz, 375.0);
                    trk.apply(&mut samples);
                    let z = bin(&samples, tone) * trk_rot(hz, 375.0, 32).unwrap();
                    let z = norm_tmg(z, tone, f64::from(shift), 32).unwrap();
                    assert!(wrap(z.arg() - 0.7).abs() < 1.0e-3);
                }
            }
        }
    }

    #[test]
    fn tmg_resid_freq() {
        for (df, hz, phi) in [(0.8, 2.0, 0.6), (-0.8, -2.0, -0.9)] {
            let p: Vec<_> = (0..21)
                .map(|sym| {
                    let tone = sym % 8;
                    let shift = (4.0 * sym as f64 / 20.0).round();
                    let start = (1000 + sym * 32) as f64;
                    let mut samples: Vec<_> = (0..32)
                        .map(|n| {
                            let t = start + shift + f64::from(n);
                            Complex32::from_polar(
                                1.0,
                                (phi + TAU * df * t / 375.0
                                    + TAU * tone as f64 * (shift + f64::from(n)) / 32.0)
                                    as f32,
                            )
                        })
                        .collect();
                    let mut trk = FrequencyTracker::new();
                    trk.reset_default(hz, 375.0);
                    trk.apply(&mut samples);
                    Pilot {
                        time: start / 375.0,
                        val: bin(&samples, tone),
                        tone,
                        sym,
                        shift,
                        hz,
                    }
                })
                .collect();
            let f = fit(&p, 12, 1.0, 32, 375.0).unwrap();
            assert!((f.df - df).abs() < 0.03);
            assert!(f.rms < 0.02);
            let expected = phi + TAU * df * (f.tref + 31.0 / 750.0);
            assert!(wrap(f.phi - expected).abs() < 0.02);
            let mut nom = p.clone();
            for p in &mut nom {
                p.time -= p.shift / 375.0;
            }
            let wrong = fit(&nom, 12, 1.0, 32, 375.0).unwrap();
            assert!((wrong.df - df).abs() > 0.002);
            assert!(wrong.rms > 3.0 * f.rms);
        }
    }
}
