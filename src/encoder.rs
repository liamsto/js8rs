// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2025 Allan Bazinet <w6baz@arrl.net>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 channel encoding to Rust and added constant-time lookup tables.

use crate::{
    codec::EncodeError,
    internal::{costas, local_routines::PARITY_MATRIX},
    protocol::Submode,
    submode,
};

pub const COSTAS_LEN: usize = 7;
pub const COSTAS_COUNT: usize = 3;
/// Number of channel symbols in one encoded frame.
pub const TONES_PER_FRAME: usize = 79;
pub const ALPHABET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-+";
const INVALID_WORD: u8 = 0xFF;

const fn build_alphabet_words() -> [u8; 256] {
    let mut words = [INVALID_WORD; 256];
    let mut i = 0usize;
    while i < 64 {
        words[ALPHABET[i] as usize] = i as u8;
        i += 1;
    }
    words
}

const ALPHABET_WORDS: [u8; 256] = build_alphabet_words();

pub const fn alphabet_word(value: u8) -> Result<u8, EncodeError> {
    let w = ALPHABET_WORDS[value as usize];
    if w == INVALID_WORD {
        Err(EncodeError::InvalidCharacter(value))
    } else {
        Ok(w)
    }
}

/// Equivalent to: `boost::augmented_crc<12, 0xc06>(data, len) ^ 42`.
pub const fn crc12(data: &[u8]) -> u16 {
    crate::internal::crc12::crc12(data)
}

/// Encode a 12-character JS8 message into 79 tones.
///
/// - `typ`: lower 3 bits are the frame type
/// - `costas`: 3 Costas arrays, each 7 symbols
/// - `message`: exactly 12 bytes (ASCII); characters must exist in `ALPHABET`
/// - `tones`: output buffer, 79 symbols
pub(crate) fn encode_with_costas(
    typ: u8,
    costas: &[[u8; COSTAS_LEN]; COSTAS_COUNT],
    message: &[u8],
    tones: &mut [u8; TONES_PER_FRAME],
) -> Result<(), EncodeError> {
    if message.len() != 12 {
        return Err(EncodeError::InvalidMessageLength(message.len()));
    }

    let mut bytes = [0u8; 11];
    for (i, j) in (0..12).step_by(4).zip((0..9).step_by(3)) {
        let w0 = u32::from(alphabet_word(message[i])?);
        let w1 = u32::from(alphabet_word(message[i + 1])?);
        let w2 = u32::from(alphabet_word(message[i + 2])?);
        let w3 = u32::from(alphabet_word(message[i + 3])?);

        let words = (w0 << 18) | (w1 << 12) | (w2 << 6) | w3;
        bytes[j] = (words >> 16) as u8;
        bytes[j + 1] = (words >> 8) as u8;
        bytes[j + 2] = words as u8;
    }

    bytes[9] = (typ & 0b111) << 5;
    let c = crc12(&bytes);
    bytes[9] |= ((c >> 7) as u8) & 0x1F;
    bytes[10] = ((c & 0x7F) as u8) << 1;

    // Costas arrays at offsets 0, 36, 72.
    for (k, pattern) in costas.iter().enumerate().take(COSTAS_COUNT) {
        let base = k * 36;
        tones[base..(COSTAS_LEN + base)].copy_from_slice(&pattern[..COSTAS_LEN]);
    }

    let message_bits = u128::from_be_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
        bytes[9], bytes[10], 0, 0, 0, 0, 0,
    ]);

    for symbol in 0..29 {
        let row = symbol * 3;
        let p0 = (PARITY_MATRIX[row] & message_bits).count_ones() as u8 & 1;
        let p1 = (PARITY_MATRIX[row + 1] & message_bits).count_ones() as u8 & 1;
        let p2 = (PARITY_MATRIX[row + 2] & message_bits).count_ones() as u8 & 1;

        tones[7 + symbol] = (p0 << 2) | (p1 << 1) | p2;
        tones[43 + symbol] = ((message_bits >> (125 - row)) & 0b111) as u8;
    }

    Ok(())
}

pub fn encode(
    typ: u8,
    submode: Submode,
    message: &[u8],
) -> Result<[u8; TONES_PER_FRAME], EncodeError> {
    let data = submode::data(submode);
    let ctype = data.costas_type();
    let c = costas::for_type(ctype);

    let mut tones = [0u8; TONES_PER_FRAME];
    encode_with_costas(typ, c, message, &mut tones)?;
    Ok(tones)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alphabet_sanity() {
        assert_eq!(alphabet_word(b'0').unwrap(), 0);
        assert_eq!(alphabet_word(b'A').unwrap(), 10);
        assert_eq!(alphabet_word(b'a').unwrap(), 36);
        assert_eq!(alphabet_word(b'-').unwrap(), 62);
        assert_eq!(alphabet_word(b'+').unwrap(), 63);
    }

    #[test]
    fn packed_parity_matches_scalar_rows() {
        let costas = costas::for_type(costas::Type::Original);

        for (typ, message) in [
            (0, b"000000000000" as &[u8]),
            (3, b"HELLO-WORLD+"),
            (7, b"0123456789AB"),
            (4, b"zzzzzzzzzzzz"),
        ] {
            let mut tones = [0; TONES_PER_FRAME];
            encode_with_costas(typ, costas, message, &mut tones).unwrap();

            let mut message_bits = 0u128;
            for &word in &tones[43..72] {
                message_bits = (message_bits << 3) | u128::from(word);
            }
            message_bits <<= 128 - 87;

            for row in 0..87 {
                let mut expected = 0u8;
                for col in 0..87 {
                    let shift = 127 - col;
                    expected ^=
                        (((PARITY_MATRIX[row] >> shift) & 1) & ((message_bits >> shift) & 1)) as u8;
                }

                let actual = (tones[7 + row / 3] >> (2 - row % 3)) & 1;
                assert_eq!(actual, expected, "parity row {row}");
            }
        }
    }

    #[test]
    fn fast_tones_match_reference_vector() {
        let expected = [
            0, 6, 2, 3, 5, 4, 1, 1, 0, 2, 1, 4, 2, 2, 0, 0, 5, 5, 5, 2, 0, 7, 5, 0, 3, 1, 0, 4, 3,
            1, 3, 3, 1, 5, 5, 0, 1, 5, 0, 2, 3, 6, 4, 2, 1, 1, 6, 2, 5, 2, 5, 3, 0, 7, 6, 4, 0, 3,
            0, 3, 3, 2, 5, 1, 5, 7, 7, 3, 3, 4, 4, 2, 2, 5, 0, 6, 4, 1, 3,
        ];
        let actual = encode(3, Submode::Fast, b"HELLO-WORLD+").unwrap();
        assert_eq!(actual, expected);
    }
}
