use std::f32;

#[derive(Clone, Copy, Debug)]
pub struct WhiteningResult<const N: usize> {
    pub llr0: [f32; N],
    pub llr1: [f32; N],
    pub erasure_applied: bool,
    pub erasures: usize,
    pub avg_abs_pre: f64,
    pub avg_abs_post: f64,
}

pub struct WhiteningProcessor;

impl WhiteningProcessor {
    #[inline]
    fn median(values: &mut [f32]) -> Option<f32> {
        if values.is_empty() {
            return None;
        }
        let mid = values.len() / 2;
        let (_, m, _) = values.select_nth_unstable_by(mid, |a, b| a.partial_cmp(b).unwrap());
        let mut med = *m;

        if values.len().is_multiple_of(2) && mid > 0 {
            let (_, m2, _) =
                values.select_nth_unstable_by(mid - 1, |a, b| a.partial_cmp(b).unwrap());
            med = 0.5 * (med + *m2);
        }
        Some(med)
    }

    #[inline]
    fn normalize_llr<const N: usize>(llr: &mut [f32; N]) {
        let mut sum = 0.0f32;
        let mut sum_sq = 0.0f32;

        for &v in llr.iter() {
            sum += v;
            sum_sq = v.mul_add(v, sum_sq);
        }

        let n = N as f32;
        let mean = sum / n;
        let mean_sq = sum_sq / n;
        let var = mean.mul_add(-mean, mean_sq);
        let sig = (if var > 0.0 { var } else { mean_sq }).sqrt();

        let inv = if sig > 0.0 && sig.is_finite() {
            1.0 / sig
        } else {
            1.0
        };
        for v in llr.iter_mut() {
            *v = (*v * inv) * 2.83;
        }
    }

    #[inline]
    const fn max4(a: f32, b: f32, c: f32, d: f32) -> f32 {
        a.max(b).max(c).max(d)
    }

    pub(crate) fn process<const NROWS: usize, const ND: usize, const N: usize>(
        s1: &[[f32; ND]; NROWS],
        symbol_winners: &[usize; ND],
        erasure_threshold: f32,
    ) -> WhiteningResult<N> {
        debug_assert!(N == 3 * ND, "expected N == 3 * ND for JS8 (LLR triplets)");
        debug_assert!(NROWS == 8, "this whitening mapping assumes 8 tones (rows)");

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

        let whitening_available =
            tone_noise.is_some() && symbol_noise.as_ref().is_some_and(|v| !v.is_empty());

        let apply_erasure_in_whitening = whitening_available && erasure_threshold > 0.0;

        let mut result = WhiteningResult::<N> {
            llr0: [0.0; N],
            llr1: [0.0; N],
            erasure_applied: apply_erasure_in_whitening,
            erasures: 0,
            avg_abs_pre: 0.0,
            avg_abs_post: 0.0,
        };

        let mut sum_abs_pre = 0.0f64;
        let mut sum_abs_post = 0.0f64;
        let mut erasures = 0usize;

        for j in 0..ND {
            let i1 = 3 * j;
            let i2 = 3 * j + 1;
            let i4 = 3 * j + 2;

            let mut ps = [0.0f32; NROWS];
            for i in 0..NROWS {
                ps[i] = s1[i][j];
            }

            result.llr0[i1] =
                Self::max4(ps[4], ps[5], ps[6], ps[7]) - Self::max4(ps[0], ps[1], ps[2], ps[3]); // r4
            result.llr0[i2] =
                Self::max4(ps[2], ps[3], ps[6], ps[7]) - Self::max4(ps[0], ps[1], ps[4], ps[5]); // r2
            result.llr0[i4] =
                Self::max4(ps[1], ps[3], ps[5], ps[7]) - Self::max4(ps[0], ps[2], ps[4], ps[6]); // r1

            for x in &mut ps {
                *x = (*x + 1e-32).ln();
            }

            result.llr1[i1] =
                Self::max4(ps[4], ps[5], ps[6], ps[7]) - Self::max4(ps[0], ps[1], ps[2], ps[3]); // r4
            result.llr1[i2] =
                Self::max4(ps[2], ps[3], ps[6], ps[7]) - Self::max4(ps[0], ps[1], ps[4], ps[5]); // r2
            result.llr1[i4] =
                Self::max4(ps[1], ps[3], ps[5], ps[7]) - Self::max4(ps[0], ps[2], ps[4], ps[6]); // r1

            if whitening_available {
                let winner = symbol_winners[j];
                let tn = tone_noise.unwrap()[winner].max(0.0);
                let sn = symbol_noise.as_ref().unwrap()[j].max(0.0);
                let local_noise = (tn * sn + 1e-12).sqrt();

                let mut apply = |v: &mut f32| {
                    let pre = f64::from((*v).abs());
                    sum_abs_pre += pre;

                    if local_noise.is_finite() && local_noise > 0.0 {
                        *v /= local_noise;
                    }

                    if apply_erasure_in_whitening && v.abs() < erasure_threshold {
                        *v = 0.0;
                        erasures += 1;
                    }

                    sum_abs_post += f64::from((*v).abs());
                };

                apply(&mut result.llr0[i1]);
                apply(&mut result.llr0[i2]);
                apply(&mut result.llr0[i4]);
                apply(&mut result.llr1[i1]);
                apply(&mut result.llr1[i2]);
                apply(&mut result.llr1[i4]);
            }
        }

        Self::normalize_llr(&mut result.llr0);
        Self::normalize_llr(&mut result.llr1);

        result.erasures = erasures;
        result.avg_abs_pre = sum_abs_pre;
        result.avg_abs_post = sum_abs_post;

        result
    }
}
