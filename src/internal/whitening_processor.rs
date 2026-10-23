// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2025 Punk Kaos <punk.kaos@gmail.com>

#[derive(Clone, Copy, Debug)]
pub struct WhiteningResult<const N: usize> {
    pub llr0: [f32; N],
    pub llr1: [f32; N],
    pub erasure_applied: bool,
    pub erasures: usize,
    pub avg_abs_pre: f64,
    pub avg_abs_post: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Blend<const ROWS: usize, const ND: usize> {
    pub nums: [[f64; ND]; ROWS],
    pub amp: f64,
    pub alpha: f32,
}

pub struct WhiteningProcessor;

impl WhiteningProcessor {
    #[inline]
    fn median(values: &mut [f32]) -> Option<f32> {
        if values.is_empty() {
            return None;
        }
        let mid = values.len() / 2;
        let (_, m, _) = values.select_nth_unstable_by(mid, f32::total_cmp);
        let mut med = *m;

        if values.len().is_multiple_of(2) && mid > 0 {
            let (_, m2, _) = values.select_nth_unstable_by(mid - 1, f32::total_cmp);
            med = 0.5 * (med + *m2);
        }
        Some(med)
    }

    fn noise<const NROWS: usize, const ND: usize>(
        s1: &[[f32; ND]; NROWS],
        symbol_winners: &[usize; ND],
    ) -> Option<([f32; NROWS], [f32; ND])> {
        let tone_noise: Option<[f32; NROWS]> = (|| {
            let mut tone_samples = [[0.0f32; ND]; NROWS];
            let mut tone_lens = [0usize; NROWS];
            for j in 0..ND {
                let w = symbol_winners[j];
                for i in 0..NROWS {
                    if i != w {
                        tone_samples[i][tone_lens[i]] = s1[i][j];
                        tone_lens[i] += 1;
                    }
                }
            }

            let mut noise = [0.0f32; NROWS];
            for i in 0..NROWS {
                let m = Self::median(&mut tone_samples[i][..tone_lens[i]])?;
                noise[i] = m;
            }
            Some(noise)
        })();

        let symbol_noise: Option<[f32; ND]> = (|| {
            let mut out = [0.0f32; ND];
            for j in 0..ND {
                let w = symbol_winners[j];
                let mut bins = [0.0f32; NROWS];
                let mut len = 0usize;
                for (i, row) in s1.iter().enumerate().take(NROWS) {
                    if i != w {
                        bins[len] = row[j];
                        len += 1;
                    }
                }
                out[j] = Self::median(&mut bins[..len])?;
            }
            if out.is_empty() { None } else { Some(out) }
        })();

        Some((tone_noise?, symbol_noise?))
    }

    // Very close signals can survive max-tone llr
    pub(crate) fn fallback<const ROWS: usize, const ND: usize, const N: usize>(
        mags: &[[f32; ND]; ROWS],
        winners: &[usize; ND],
    ) -> ([f32; N], [f32; N]) {
        let noise = Self::noise(mags, winners);
        let mut llr0 = [0.0; N];
        let mut llr1 = [0.0; N];
        for j in 0..ND {
            let p: [f32; ROWS] = std::array::from_fn(|i| mags[i][j]);
            let log = p.map(|v| (v + 1e-32).ln());
            for bit in 0..3 {
                let shift = 2 - bit;
                let idx = 3 * j + bit;
                llr0[idx] = Self::group_max(&p, shift, 1) - Self::group_max(&p, shift, 0);
                llr1[idx] = Self::group_max(&log, shift, 1) - Self::group_max(&log, shift, 0);
                if let Some((tone, sym)) = noise.as_ref() {
                    let sigma = (tone[winners[j]].max(0.0) * sym[j].max(0.0) + 1e-12).sqrt();
                    for v in [&mut llr0[idx], &mut llr1[idx]] {
                        if sigma.is_finite() && sigma > 0.0 {
                            *v /= sigma;
                        }
                        if v.abs() < 0.25 {
                            *v = 0.0;
                        }
                    }
                }
            }
        }
        for llr in [&mut llr0, &mut llr1] {
            let mean = llr.iter().sum::<f32>() / N as f32;
            let mean_sq = llr.iter().fold(0.0, |sum, &v| v.mul_add(v, sum)) / N as f32;
            let var = mean.mul_add(-mean, mean_sq);
            let sigma = (if var > 0.0 { var } else { mean_sq }).sqrt();
            let inv = if sigma.is_finite() && sigma > 0.0 {
                1.0 / sigma
            } else {
                1.0
            };
            for v in llr {
                *v = (*v * inv) * 2.83;
            }
        }
        (llr0, llr1)
    }

    fn group_max<const ROWS: usize>(scores: &[f32; ROWS], shift: usize, bit: usize) -> f32 {
        scores
            .iter()
            .enumerate()
            .filter(|(i, _)| (i >> shift) & 1 == bit)
            .fold(f32::NEG_INFINITY, |m, (_, &v)| m.max(v))
    }

    fn lse<const ROWS: usize>(scores: &[f32; ROWS], shift: usize, bit: usize) -> f32 {
        let max = Self::group_max(scores, shift, bit);
        if max.is_infinite() {
            return 0.0;
        }
        max + scores
            .iter()
            .enumerate()
            .filter(|(i, _)| (i >> shift) & 1 == bit)
            .map(|(_, &v)| (v - max).exp())
            .sum::<f32>()
            .ln()
    }

    pub(crate) fn process<const NROWS: usize, const ND: usize, const N: usize>(
        s1: &[[f32; ND]; NROWS],
        symbol_winners: &[usize; ND],
        erasure_threshold: f32,
        blend: Option<&Blend<NROWS, ND>>,
        llr_scale: f32,
    ) -> WhiteningResult<N> {
        debug_assert!(N == 3 * ND, "expected N == 3 * ND for JS8 (LLR triplets)");
        debug_assert!(NROWS == 8, "assumes 8 tones");

        let llr_scale = if llr_scale > 0.0 && llr_scale.is_finite() {
            llr_scale
        } else {
            1.0
        };
        let blend = blend.filter(|b| {
            b.alpha > 0.0
                && b.alpha.is_finite()
                && b.amp > 0.0
                && b.amp.is_finite()
                && b.nums.iter().flatten().all(|&v| (v as f32).is_finite())
        });

        let noise = Self::noise(s1, symbol_winners);
        let whitening_available = noise.is_some();

        let apply_erasure_in_whitening = whitening_available && erasure_threshold > 0.0;

        let mut result = WhiteningResult::<N> {
            llr0: [0.0; N],
            llr1: [0.0; N],
            erasure_applied: apply_erasure_in_whitening,
            erasures: 0,
            avg_abs_pre: 0.0,
            avg_abs_post: 0.0,
        };

        for j in 0..ND {
            let inv_var = if whitening_available {
                let (tone, sym) = noise.as_ref().unwrap();
                1.0 / (tone[symbol_winners[j]].max(0.0) * sym[j].max(0.0) + 1e-12)
            } else {
                1.0
            };
            let noncoh: [f32; NROWS] = std::array::from_fn(|i| {
                let power = s1[i][j] * s1[i][j];
                0.5 * power * inv_var
            });
            let mut scores = noncoh;
            if let Some(b) = blend {
                let coh: [f32; NROWS] = std::array::from_fn(|i| b.nums[i][j] as f32 * inv_var);
                if noncoh.iter().chain(&coh).all(|v| v.is_finite()) {
                    let alpha = b.alpha.clamp(0.0, 1.0);
                    for i in 0..NROWS {
                        scores[i] = (1.0 - alpha) * noncoh[i] + alpha * coh[i];
                    }
                }
            }
            for bit in 0..3 {
                let idx = 3 * j + bit;
                let shift = 2 - bit;
                result.llr0[idx] =
                    (Self::lse(&scores, shift, 1) - Self::lse(&scores, shift, 0)) * llr_scale;
                result.llr1[idx] = if blend.is_some() {
                    (Self::lse(&noncoh, shift, 1) - Self::lse(&noncoh, shift, 0)) * llr_scale
                } else {
                    result.llr0[idx]
                };
                if whitening_available {
                    for v in [&mut result.llr0[idx], &mut result.llr1[idx]] {
                        result.avg_abs_pre += f64::from(v.abs());
                        if apply_erasure_in_whitening && v.abs() < erasure_threshold {
                            *v = 0.0;
                            result.erasures += 1;
                        }
                        result.avg_abs_post += f64::from(v.abs());
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mags() -> [[f32; 8]; 8] {
        std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 2.0 } else { 1.0 }))
    }

    #[test]
    fn fallback_overlap() {
        let mut s = mags();
        let winners = std::array::from_fn(|i| i);
        let (a, b) = WhiteningProcessor::fallback::<8, 8, 24>(&s, &winners);
        for i in 0..24 {
            let sign = if (i / 3) >> (2 - i % 3) & 1 == 1 {
                1.0
            } else {
                -1.0
            };
            assert!((a[i] - sign * 2.83).abs() < 1e-6);
            assert!((b[i] - sign * 2.83).abs() < 1e-6);
        }
        s[7][7] = 1.1;
        let (a, b) = WhiteningProcessor::fallback::<8, 8, 24>(&s, &winners);
        assert_eq!(&a[21..], &[0.0; 3]);
        assert_eq!(&b[21..], &[0.0; 3]);
    }

    #[test]
    fn power_llr() {
        let s = mags();
        let winners = std::array::from_fn(|i| i);
        let res = WhiteningProcessor::process::<8, 8, 24>(&s, &winners, 0.0, None, 2.0);
        let expected = ((2.0f32.exp() + 3.0 * 0.5f32.exp()).ln() - (4.0 * 0.5f32.exp()).ln()) * 2.0;
        for sym in 0..8 {
            for bit in 0..3 {
                let sign = if sym >> (2 - bit) & 1 == 1 { 1.0 } else { -1.0 };
                assert!((res.llr0[sym * 3 + bit] - sign * expected).abs() < 1e-6);
            }
        }
        assert_eq!(res.llr0, res.llr1);
        let mut s2 = s;
        s2[7][7] = 4.0;
        let res2 = WhiteningProcessor::process::<8, 8, 24>(&s2, &winners, 0.0, None, 2.0);
        assert_eq!(res2.llr0[..21], res.llr0[..21]);
        assert!(res2.llr0[21] > res.llr0[21] * 2.0);
        let erased =
            WhiteningProcessor::process::<8, 8, 24>(&s, &winners, expected * 0.75, None, 1.0);
        assert!(erased.erasure_applied);
        assert_eq!(erased.erasures, 48);
        assert_eq!(erased.llr0, [0.0; 24]);
    }

    #[test]
    fn coh_fallback() {
        let s = mags();
        let winners = std::array::from_fn(|i| i);
        let base = WhiteningProcessor::process::<8, 8, 24>(&s, &winners, 0.0, None, 2.0);
        let mut b = Blend {
            nums: std::array::from_fn(|i| std::array::from_fn(|j| if i == j { 2.0 } else { -2.0 })),
            amp: 2.0,
            alpha: 0.5,
        };
        let res = WhiteningProcessor::process::<8, 8, 24>(&s, &winners, 0.0, Some(&b), 2.0);
        assert_eq!(res.llr1, base.llr1);
        assert!(res.llr0[21] > base.llr0[21]);
        for (alpha, amp, num) in [
            (0.0, 2.0, 2.0),
            (f32::NAN, 2.0, 2.0),
            (0.5, f64::INFINITY, 2.0),
            (0.5, 0.0, 2.0),
            (0.5, 2.0, f64::NAN),
        ] {
            b.alpha = alpha;
            b.amp = amp;
            b.nums[0][0] = num;
            let res = WhiteningProcessor::process::<8, 8, 24>(&s, &winners, 0.0, Some(&b), 2.0);
            assert_eq!(res.llr0, base.llr0);
            assert_eq!(res.llr1, base.llr1);
        }
    }

    #[test]
    fn no_noise_samples() {
        let s = [[1.0], [1.0], [1.0], [1.0], [1.0], [1.0], [1.0], [2.0]];
        let res = WhiteningProcessor::process::<8, 1, 3>(&s, &[7], 0.0, None, f32::NAN);
        assert!(res.llr0.iter().all(|&v| v > 0.0 && v.is_finite()));
        assert_eq!(res.llr0, res.llr1);
        let zero = WhiteningProcessor::process::<8, 1, 3>(&[[0.0]; 8], &[0], 0.0, None, 1.0);
        assert_eq!(zero.llr0, [0.0; 3]);
    }
}
