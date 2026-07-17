const CRC12_BITS: u16 = 12;
const CRC12_TRUNC_POLY: u16 = 0x0C06;
const CRC12_MASK: u16 = 0x0FFF;
const CRC12_XOR_OUT: u16 = 42;

const fn reflect_bits(mut x: u16, len: u16) -> u16 {
    let mut r: u16 = 0;
    let mut i: u16 = 0;
    while i < len {
        r = (r << 1) | (x & 1);
        x >>= 1;
        i += 1;
    }
    r
}

const fn reflect_optionally(x: u16, reflect: bool, len: u16) -> u16 {
    if reflect { reflect_bits(x, len) } else { x }
}

const fn crc_modulo_word_update(
    register_length: u16,
    mut remainder: u16,
    mut new_dividend_bits: u16,
    truncated_divisor: u16,
    word_length: u16,
    reflect: bool,
) -> u16 {
    let high_bit_mask: u16 = 1u16 << (register_length - 1);

    new_dividend_bits = reflect_optionally(new_dividend_bits, !reflect, word_length);

    let mut i: u16 = 0;
    while i < word_length {
        if (new_dividend_bits & 1) != 0 {
            remainder ^= high_bit_mask;
        }

        let quotient = (remainder & high_bit_mask) != 0;
        remainder <<= 1;
        if quotient {
            remainder ^= truncated_divisor;
        }

        new_dividend_bits >>= 1;
        i += 1;
    }

    remainder
}

const fn make_crc12_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut d: u16 = 0;
    while d < 256 {
        table[d as usize] = crc_modulo_word_update(CRC12_BITS, 0u16, d, CRC12_TRUNC_POLY, 8, false);
        d += 1;
    }
    table
}

const CRC12_TABLE: [u16; 256] = make_crc12_table();

#[inline]
const fn augmented_crc12(data: &[u8]) -> u16 {
    let mut remainder: u16 = 0;
    let mut i = 0usize;

    while i < data.len() {
        let index = ((remainder >> (CRC12_BITS - 8)) & 0x00FF) as usize;
        remainder = (remainder << 8) | (data[i] as u16);
        remainder ^= CRC12_TABLE[index];
        i += 1;
    }

    remainder & CRC12_MASK
}

#[inline]
pub const fn crc12(data: &[u8]) -> u16 {
    augmented_crc12(data) ^ CRC12_XOR_OUT
}
