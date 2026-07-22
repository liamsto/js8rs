// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2025 Allan Bazinet <w6baz@arrl.net>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 bit packing, CRC validation, and parity data to Rust.

use crate::internal::crc12::crc12;

pub const ALPHABET: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-+";

// decoded_bits are 0/1 values (i8) representing KK bits.
pub fn check_crc12(decoded_bits: &[i8]) -> bool {
    let mut bytes = [0u8; 11];

    for (i, &b) in decoded_bits.iter().enumerate() {
        if b != 0 {
            let byte = i / 8;
            if byte >= bytes.len() {
                break;
            }
            bytes[byte] |= 1u8 << (7 - (i % 8));
        }
    }

    let received: u16 = (u16::from(bytes[9] & 0x1F) << 7) | (u16::from(bytes[10]) >> 1);

    bytes[9] &= 0xE0;
    bytes[10] = 0x00;

    received == crc12(&bytes)
}

pub fn extract_message_174(decoded_bits: &[i8]) -> String {
    if !check_crc12(decoded_bits) {
        return String::new();
    }

    let mut msg = String::with_capacity(12);

    // Decode 72 data bits into 12x 6-bit words.
    for i in 0..12usize {
        let base = i * 6;
        let w: u8 = ((decoded_bits[base] as u8) << 5)
            | ((decoded_bits[base + 1] as u8) << 4)
            | ((decoded_bits[base + 2] as u8) << 3)
            | ((decoded_bits[base + 3] as u8) << 2)
            | ((decoded_bits[base + 4] as u8) << 1)
            | (decoded_bits[base + 5] as u8);

        msg.push(ALPHABET[w as usize] as char);
    }

    msg
}

pub const PARITY_ROWS: usize = 87;
pub const PARITY_COLS: usize = 87;

const PARITY_DATA: [&str; PARITY_ROWS] = [
    "23bba830e23b6b6f50982e",
    "1f8e55da218c5df3309052",
    "ca7b3217cd92bd59a5ae20",
    "56f78313537d0f4382964e",
    "6be396b5e2e819e373340c",
    "293548a138858328af4210",
    "cb6c6afcdc28bb3f7c6e86",
    "3f2a86f5c5bd225c961150",
    "849dd2d63673481860f62c",
    "56cdaec6e7ae14b43feeee",
    "04ef5cfa3766ba778f45a4",
    "c525ae4bd4f627320a3974",
    "41fd9520b2e4abeb2f989c",
    "7fb36c24085a34d8c1dbc4",
    "40fc3e44bb7d2bb2756e44",
    "d38ab0a1d2e52a8ec3bc76",
    "3d0f929ef3949bd84d4734",
    "45d3814f504064f80549ae",
    "f14dbf263825d0bd04b05e",
    "db714f8f64e8ac7af1a76e",
    "8d0274de71e7c1a8055eb0",
    "51f81573dd4049b082de14",
    "d8f937f31822e57c562370",
    "b6537f417e61d1a7085336",
    "ecbd7c73b9cd34c3720c8a",
    "3d188ea477f6fa41317a4e",
    "1ac4672b549cd6dba79bcc",
    "a377253773ea678367c3f6",
    "0dbd816fba1543f721dc72",
    "ca4186dd44c3121565cf5c",
    "29c29dba9c545e267762fe",
    "1616d78018d0b4745ca0f2",
    "fe37802941d66dde02b99c",
    "a9fa8e50bcb032c85e3304",
    "83f640f1a48a8ebc0443ea",
    "3776af54ccfbae916afde6",
    "a8fc906976c35669e79ce0",
    "f08a91fb2e1f78290619a8",
    "cc9da55fe046d0cb3a770c",
    "d36d662a69ae24b74dcbd8",
    "40907b01280f03c0323946",
    "d037db825175d851f3af00",
    "1bf1490607c54032660ede",
    "0af7723161ec223080be86",
    "eca9afa0f6b01d92305edc",
    "7a8dec79a51e8ac5388022",
    "9059dfa2bb20ef7ef73ad4",
    "6abb212d9739dfc02580f2",
    "f6ad4824b87c80ebfce466",
    "d747bfc5fd65ef70fbd9bc",
    "612f63acc025b6ab476f7c",
    "05209a0abb530b9e7e34b0",
    "45b7ab6242b77474d9f11a",
    "6c280d2a0523d9c4bc5946",
    "f1627701a2d692fd9449e6",
    "8d9071b7e7a6a2eed6965e",
    "bf4f56e073271f6ab4bf80",
    "c0fc3ec4fb7d2bb2756644",
    "57da6d13cb96a7689b2790",
    "a9fa2eefa6f8796a355772",
    "164cc861bdd803c547f2ac",
    "cc6de59755420925f90ed2",
    "a0c0033a52ab6299802fd2",
    "b274db8abd3c6f396ea356",
    "97d4169cb33e7435718d90",
    "81cfc6f18c35b1e1f17114",
    "481a2a0df8a23583f82d6c",
    "081c29a10d468ccdbcecb6",
    "2c4142bf42b01e71076acc",
    "a6573f3dc8b16c9d19f746",
    "c87af9a5d5206abca532a8",
    "012dee2198eba82b19a1da",
    "b1ca4ea2e3d173bad4379c",
    "b33ec97be83ce413f9acc8",
    "5b0f7742bca86b8012609a",
    "37d8e0af9258b9e8c5f9b2",
    "35ad3fb0faeb5f1b0c30dc",
    "6114e08483043fd3f38a8a",
    "cd921fdf59e882683763f6",
    "95e45ecd0135aca9d6e6ae",
    "2e547dd7a05f6597aac516",
    "14cd0f642fc0c5fe3a65ca",
    "3a0a1dfd7eee29c2e827e0",
    "c8b5dffc335095dcdcaf2a",
    "3dd01a59d86310743ec752",
    "8abdb889efbe39a510a118",
    "3f231f212055371cf3e2a2",
];

const fn hex_nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("invalid hex"),
    }
}

const fn build_parity_matrix() -> [u128; PARITY_ROWS] {
    let masks: [u8; 4] = [0x8, 0x4, 0x2, 0x1];
    let mut data = [0u128; PARITY_ROWS];

    let mut row = 0usize;
    while row < PARITY_ROWS {
        let bytes = PARITY_DATA[row].as_bytes();
        let mut col = 0usize;

        let mut i = 0usize;
        while i < bytes.len() {
            let v = hex_nibble(bytes[i]);
            let mut m = 0usize;
            while m < 4 {
                if col >= PARITY_COLS {
                    break;
                }
                if (v & masks[m]) != 0 {
                    data[row] |= 1u128 << (127 - col);
                }
                col += 1;
                m += 1;
            }
            if col >= PARITY_COLS {
                break;
            }
            i += 1;
        }

        row += 1;
    }

    data
}

pub const PARITY_MATRIX: [u128; PARITY_ROWS] = build_parity_matrix();
