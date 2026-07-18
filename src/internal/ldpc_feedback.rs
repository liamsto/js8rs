// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2025 Punk Kaos <punk.kaos@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported LDPC to alloc free Rust.

pub const LLR_ERASURE_THRESHOLD_DEFAULT: f32 = 0.25;
pub const LLR_FEEDBACK_CONFIDENT_MIN: f32 = 3.0;
pub const LLR_FEEDBACK_UNCERTAIN_MAX: f32 = 1.0;
pub const LLR_FEEDBACK_CONFIDENT_BOOST: f32 = 1.2;
pub const LLR_FEEDBACK_UNCERTAIN_SHRINK: f32 = 0.5;
pub const LLR_FEEDBACK_MAX_MAG: f32 = 6.0;
pub const LDPC_FEEDBACK_MAX_PASSES_DEFAULT: u8 = 8;

/// Refines LLRs using decoded codeword feedback.
pub const fn refine_llrs_with_ldpc_feedback<const N: usize>(
    llr_in: &[f32; N],
    cw: &[i8; N],
    erasure_threshold: f32,
    llr_out: &mut [f32; N],
    confident_count: &mut u32,
    uncertain_count: &mut u32,
) {
    *llr_out = *llr_in;
    *confident_count = 0;
    *uncertain_count = 0;
    let mut i = 0;
    while i < N {
        let value = &mut llr_out[i];

        if !value.is_finite() {
            *value = 0.0;
            *uncertain_count += 1;
            i += 1;
            continue;
        }

        let bit_one = cw[i] != 0;
        let mag = value.abs();
        let sign_match = (*value >= 0.0) == bit_one;

        if sign_match && mag >= LLR_FEEDBACK_CONFIDENT_MIN {
            *confident_count += 1;

            let mut boosted = mag * LLR_FEEDBACK_CONFIDENT_BOOST;
            boosted = boosted.clamp(0.0, LLR_FEEDBACK_MAX_MAG);

            *value = if bit_one { boosted } else { -boosted };
        } else if !sign_match || mag <= LLR_FEEDBACK_UNCERTAIN_MAX {
            *uncertain_count += 1;

            let shrunk = mag * LLR_FEEDBACK_UNCERTAIN_SHRINK;

            if erasure_threshold > 0.0 && shrunk < erasure_threshold {
                *value = 0.0;
            } else {
                *value = if bit_one { shrunk } else { -shrunk };
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_llrs_are_erased_without_stalling() {
        let input = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY];
        let codeword = [0, 1, 0];
        let mut output = input;
        let mut confident = 0;
        let mut uncertain = 0;

        refine_llrs_with_ldpc_feedback(
            &input,
            &codeword,
            LLR_ERASURE_THRESHOLD_DEFAULT,
            &mut output,
            &mut confident,
            &mut uncertain,
        );

        assert_eq!(output, [0.0; 3]);
        assert_eq!(confident, 0);
        assert_eq!(uncertain, 3);
    }
}
