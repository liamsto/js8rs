// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>

#[cfg(test)]
use crate::protocol::FrameType;
use crate::{
    command::{
        CommandKind, command_prefix, is_buffered_token, is_compound_call, is_grid, is_valid_call,
    },
    protocol::{FrameFlags, Submode},
};
use phf::phf_map;
use std::str;

#[cfg(test)]
use std::collections::VecDeque;

mod jsc;
mod jsc_tables;

pub const NALPHABET: usize = 41;

/// Alphabet to encode into.
pub const ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?";
pub const ALPHABET72: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-+/?.";

const fn build_alphabet72_index() -> [u8; 256] {
    let mut index = [0; 256];
    let bytes = ALPHABET72.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        index[bytes[i] as usize] = i as u8;
        i += 1;
    }
    index
}

const ALPHABET72_INDEX: [u8; 256] = build_alphabet72_index();
// Huffman table: char -> code
pub static HUFFTABLE: phf::Map<&'static str, &'static str> = phf_map! {
    " "  => "01",
    "E"  => "100",
    "T"  => "1101",
    "A"  => "0011",
    "O"  => "11111",
    "I"  => "11100",
    "N"  => "10111",
    "S"  => "10100",
    "H"  => "00011",
    "R"  => "00000",
    "D"  => "111011",
    "L"  => "110011",
    "C"  => "110001",
    "U"  => "101101",
    "M"  => "101011",
    "W"  => "001011",
    "F"  => "001001",
    "G"  => "000101",
    "Y"  => "000011",
    "P"  => "1111011",
    "B"  => "1111001",
    "."  => "1110100",
    "V"  => "1100101",
    "K"  => "1100100",
    "-"  => "1100001",
    "+"  => "1100000",
    "?"  => "1011001",
    "!"  => "1011000",
    "\"" => "1010101",
    "X"  => "1010100",
    "0"  => "0010101",
    "J"  => "0010100",
    "1"  => "0010001",
    "Q"  => "0010000",
    "2"  => "0001001",
    "Z"  => "0001000",
    "3"  => "0000101",
    "5"  => "0000100",
    "4"  => "11110101",
    "9"  => "11110100",
    "8"  => "11110001",
    "6"  => "11110000",
    "7"  => "11101011",
    "/"  => "11101010",
};

// Numeric domain constants
pub const NBASECALL: u32 = 37 * 36 * 10 * 27 * 27 * 27;
pub const NBASEGRID: u16 = 180 * 180;
pub const NUSERGRID: u16 = NBASEGRID + 10;
pub const NMAXGRID: u16 = (1 << 15) - 1;

// Basecalls: special/group calls mapped into the extended numeric space
pub static BASECALLS: phf::Map<&'static str, u32> = phf_map! {
    "<....>"     => NBASECALL + 1,
    "@ALLCALL"   => NBASECALL + 2,
    "@JS8NET"    => NBASECALL + 3,
    "@DX/NA"     => NBASECALL + 4,
    "@DX/SA"     => NBASECALL + 5,
    "@DX/EU"     => NBASECALL + 6,
    "@DX/AS"     => NBASECALL + 7,
    "@DX/AF"     => NBASECALL + 8,
    "@DX/OC"     => NBASECALL + 9,
    "@DX/AN"     => NBASECALL + 10,
    "@REGION/1"  => NBASECALL + 11,
    "@REGION/2"  => NBASECALL + 12,
    "@REGION/3"  => NBASECALL + 13,
    "@GROUP/0"   => NBASECALL + 14,
    "@GROUP/1"   => NBASECALL + 15,
    "@GROUP/2"   => NBASECALL + 16,
    "@GROUP/3"   => NBASECALL + 17,
    "@GROUP/4"   => NBASECALL + 18,
    "@GROUP/5"   => NBASECALL + 19,
    "@GROUP/6"   => NBASECALL + 20,
    "@GROUP/7"   => NBASECALL + 21,
    "@GROUP/8"   => NBASECALL + 22,
    "@GROUP/9"   => NBASECALL + 23,
    "@COMMAND"   => NBASECALL + 24,
    "@CONTROL"   => NBASECALL + 25,
    "@NET"       => NBASECALL + 26,
    "@NTS"       => NBASECALL + 27,
    "@RESERVE/0" => NBASECALL + 28,
    "@RESERVE/1" => NBASECALL + 29,
    "@RESERVE/2" => NBASECALL + 30,
    "@RESERVE/3" => NBASECALL + 31,
    "@RESERVE/4" => NBASECALL + 32,
    "@APRSIS"    => NBASECALL + 33,
    "@RAGCHEW"   => NBASECALL + 34,
    "@JS8"       => NBASECALL + 35,
    "@EMCOMM"    => NBASECALL + 36,
    "@ARES"      => NBASECALL + 37,
    "@MARS"      => NBASECALL + 38,
    "@AMRRON"    => NBASECALL + 39,
    "@RACES"     => NBASECALL + 40,
    "@RAYNET"    => NBASECALL + 41,
    "@RADAR"     => NBASECALL + 42,
    "@SKYWARN"   => NBASECALL + 43,
    "@CQ"        => NBASECALL + 44,
    "@HB"        => NBASECALL + 45,
    "@QSO"       => NBASECALL + 46,
    "@QSOPARTY"  => NBASECALL + 47,
    "@CONTEST"   => NBASECALL + 48,
    "@FIELDDAY"  => NBASECALL + 49,
    "@SOTA"      => NBASECALL + 50,
    "@IOTA"      => NBASECALL + 51,
    "@POTA"      => NBASECALL + 52,
    "@QRP"       => NBASECALL + 53,
    "@QRO"       => NBASECALL + 54,
};

const BASECALL_NAMES: [&str; 54] = [
    "<....>",
    "@ALLCALL",
    "@JS8NET",
    "@DX/NA",
    "@DX/SA",
    "@DX/EU",
    "@DX/AS",
    "@DX/AF",
    "@DX/OC",
    "@DX/AN",
    "@REGION/1",
    "@REGION/2",
    "@REGION/3",
    "@GROUP/0",
    "@GROUP/1",
    "@GROUP/2",
    "@GROUP/3",
    "@GROUP/4",
    "@GROUP/5",
    "@GROUP/6",
    "@GROUP/7",
    "@GROUP/8",
    "@GROUP/9",
    "@COMMAND",
    "@CONTROL",
    "@NET",
    "@NTS",
    "@RESERVE/0",
    "@RESERVE/1",
    "@RESERVE/2",
    "@RESERVE/3",
    "@RESERVE/4",
    "@APRSIS",
    "@RAGCHEW",
    "@JS8",
    "@EMCOMM",
    "@ARES",
    "@MARS",
    "@AMRRON",
    "@RACES",
    "@RAYNET",
    "@RADAR",
    "@SKYWARN",
    "@CQ",
    "@HB",
    "@QSO",
    "@QSOPARTY",
    "@CONTEST",
    "@FIELDDAY",
    "@SOTA",
    "@IOTA",
    "@POTA",
    "@QRP",
    "@QRO",
];

pub static CQS: phf::Map<u32, &'static str> = phf_map! {
    0u32 => "CQ CQ CQ",
    1u32 => "CQ DX",
    2u32 => "CQ QRP",
    3u32 => "CQ CONTEST",
    4u32 => "CQ FIELD",
    5u32 => "CQ FD",
    6u32 => "CQ CQ",
    7u32 => "CQ",
};

use crate::varicode::jsc::{compress_frame, decompress};
use crc::{CRC_16_KERMIT, CRC_32_BZIP2, Crc};
pub fn cq_string(number: i32) -> String {
    if number < 0 {
        return String::new();
    }
    CQS.get(&(number as u32)).copied().unwrap_or("").to_string()
}

pub fn starts_with_cq(text: &str) -> bool {
    text.as_bytes().starts_with(b"CQ")
}

pub fn starts_with_hb(text: &str) -> bool {
    text.as_bytes().starts_with(b"HB")
}

pub fn format_snr(snr: i32) -> String {
    if !(-60..=60).contains(&snr) {
        return String::new();
    }
    if snr >= 0 {
        format!("+{snr:02}")
    } else {
        format!("{snr:03}")
    }
}

pub fn checksum16(input: &str) -> String {
    let crc = Crc::<u16>::new(&CRC_16_KERMIT).checksum(input.as_bytes());
    let mut checksum = pack16bits(crc);
    if checksum.len() < 3 {
        checksum.push_str(&" ".repeat(3 - checksum.len()));
    }
    checksum
}

pub fn checksum16_valid(checksum: &str, input: &str) -> bool {
    let crc = Crc::<u16>::new(&CRC_16_KERMIT).checksum(input.as_bytes());
    pack16bits(crc) == checksum
}

pub fn checksum32(input: &str) -> String {
    let crc = Crc::<u32>::new(&CRC_32_BZIP2).checksum(input.as_bytes());
    let mut checksum = pack32bits(crc);
    if checksum.len() < 6 {
        checksum.push_str(&" ".repeat(6 - checksum.len()));
    }
    checksum
}

pub fn checksum32_valid(checksum: &str, input: &str) -> bool {
    let crc = Crc::<u32>::new(&CRC_32_BZIP2).checksum(input.as_bytes());
    pack32bits(crc) == checksum
}

pub fn huff_decode(bitvec: &[bool]) -> String {
    let mut text = String::new();
    let mut pos = 0;

    while pos < bitvec.len() {
        let mut found = None;
        for (key, code) in HUFFTABLE.entries() {
            if pos + code.len() <= bitvec.len()
                && code
                    .bytes()
                    .enumerate()
                    .all(|(i, byte)| bitvec[pos + i] == (byte == b'1'))
            {
                text.push_str(key);
                found = Some(code.len());
                break;
            }
        }
        let Some(len) = found else {
            break;
        };
        pos += len;
    }

    text
}

#[cfg(test)]
pub fn int_to_bits(mut value: u64, expected: usize) -> Vec<bool> {
    let mut bits: VecDeque<bool> = VecDeque::new();

    while value != 0 {
        bits.push_front((value & 1) != 0);
        value >>= 1;
    }

    if expected != 0 {
        while bits.len() < expected {
            bits.push_front(false);
        }
    }

    bits.into_iter().collect()
}

pub fn bits_to_int(value: &[bool]) -> u64 {
    value
        .iter()
        .fold(0, |bits, &bit| (bits << 1) | u64::from(bit))
}

/// Packs a 16-bit value into a three character sequence.
pub fn pack16bits(packed: u16) -> String {
    let na = NALPHABET as u32;
    let p = u32::from(packed);

    let mut out = String::with_capacity(3);

    let tmp0 = (p / (na * na)) as usize;
    out.push(ALPHABET.chars().nth(tmp0).unwrap());

    let tmp1 = ((p - (tmp0 as u32) * (na * na)) / na) as usize;
    out.push(ALPHABET.chars().nth(tmp1).unwrap());

    let tmp2 = (p % na) as usize;
    out.push(ALPHABET.chars().nth(tmp2).unwrap());

    out
}

pub fn pack32bits(packed: u32) -> String {
    let a: u16 = ((packed & 0xFFFF_0000) >> 16) as u16;
    let b: u16 = (packed & 0x0000_FFFF) as u16;
    let mut s = String::with_capacity(6);
    s.push_str(&pack16bits(a));
    s.push_str(&pack16bits(b));
    s
}

/// Returns the first 64 bits and sets the last 8 bits in `rem_out`.
pub fn unpack72bits(text: &str, rem_out: Option<&mut u8>) -> u64 {
    let mut encoded = [0u8; 12];
    if text.is_ascii() {
        for (out, &b) in encoded.iter_mut().zip(&text.as_bytes()[..12]) {
            *out = ALPHABET72_INDEX[b as usize];
        }
    } else {
        let mut chars = text.chars();
        for out in &mut encoded {
            let ch = chars.next().expect("unpack72bits requires 12 characters");
            *out = ALPHABET72.find(ch).unwrap_or(0) as u8;
        }
    }

    let mut value: u64 = 0;
    for (i, &v) in encoded[..10].iter().enumerate() {
        value |= u64::from(v) << (58 - 6 * i);
    }

    let rem_high = encoded[10];
    value |= u64::from(rem_high >> 2);

    let rem: u8 = ((rem_high & 0b11) << 6) | encoded[11];

    if let Some(r) = rem_out {
        *r = rem;
    }
    value
}

pub fn pack72bits(mut value: u64, rem: u8) -> String {
    let alphabet = ALPHABET72.as_bytes();
    let mut packed = [0u8; 12];

    let rem_high: u8 = (((value as u8) & 0x0F) << 2) | (rem >> 6);
    let rem_low: u8 = rem & 0x3F;
    value >>= 4;

    packed[11] = alphabet[rem_low as usize];
    packed[10] = alphabet[rem_high as usize];

    for i in 0..10 {
        let idx = (value as u8) & 0x3F;
        packed[9 - i] = alphabet[idx as usize];
        value >>= 6;
    }

    String::from_utf8(packed.to_vec()).expect("ALPHABET72 is ASCII")
}

fn alnum_idx(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => u32::from(byte - b'0'),
        b'A'..=b'Z' => u32::from(byte - b'A') + 10,
        b' ' => 36,
        b'/' => 37,
        b'@' => 38,
        _ => u32::MAX,
    }
}

fn alnum_byte(idx: u32) -> u8 {
    match idx {
        0..=9 => b'0' + idx as u8,
        10..=35 => b'A' + (idx as u8 - 10),
        36 => b' ',
        37 => b'/',
        38 => b'@',
        _ => 0,
    }
}

pub fn pack_alpha_numeric50(value: &str) -> u64 {
    let mut word = [b' '; 11];
    let mut len = 0;
    for &byte in value.as_bytes() {
        if alnum_idx(byte) != u32::MAX {
            word[len] = byte;
            len += 1;
            if len == word.len() {
                break;
            }
        }
    }

    if len > 3 && word[3] != b'/' {
        word.copy_within(3..10, 4);
        word[3] = b' ';
        len = (len + 1).min(word.len());
    }
    if len > 7 && word[7] != b'/' {
        word.copy_within(7..10, 8);
        word[7] = b' ';
    }

    let idx = |i: usize| u64::from(alnum_idx(word[i]));

    let a = 38u64 * 38 * 38 * 2 * 38 * 38 * 38 * 2 * 38 * 38 * idx(0);
    let b = 38u64 * 38 * 38 * 2 * 38 * 38 * 38 * 2 * 38 * idx(1);
    let c = 38u64 * 38 * 38 * 2 * 38 * 38 * 38 * 2 * idx(2);
    let d = 38u64 * 38 * 38 * 2 * 38 * 38 * 38 * u64::from(word[3] == b'/');
    let e = 38u64 * 38 * 38 * 2 * 38 * 38 * idx(4);
    let f = 38u64 * 38 * 38 * 2 * 38 * idx(5);
    let g = 38u64 * 38 * 38 * 2 * idx(6);
    let h = 38u64 * 38 * 38 * u64::from(word[7] == b'/');
    let i = 38u64 * 38 * idx(8);
    let j = 38u64 * idx(9);
    let k = idx(10);

    a + b + c + d + e + f + g + h + i + j + k
}

pub fn unpack_alpha_numeric50(mut packed: u64) -> String {
    let mut word = [0u8; 11];

    let mut tmp = (packed % 38) as u32;
    word[10] = alnum_byte(tmp);
    packed /= 38;

    tmp = (packed % 38) as u32;
    word[9] = alnum_byte(tmp);
    packed /= 38;

    tmp = (packed % 38) as u32;
    word[8] = alnum_byte(tmp);
    packed /= 38;

    word[7] = if packed & 1 != 0 { b'/' } else { b' ' };
    packed /= 2;

    tmp = (packed % 38) as u32;
    word[6] = alnum_byte(tmp);
    packed /= 38;

    tmp = (packed % 38) as u32;
    word[5] = alnum_byte(tmp);
    packed /= 38;

    tmp = (packed % 38) as u32;
    word[4] = alnum_byte(tmp);
    packed /= 38;

    word[3] = if packed & 1 != 0 { b'/' } else { b' ' };
    packed /= 2;

    tmp = (packed % 38) as u32;
    word[2] = alnum_byte(tmp);
    packed /= 38;

    tmp = (packed % 38) as u32;
    word[1] = alnum_byte(tmp);
    packed /= 38;

    tmp = (packed % 39) as u32;
    word[0] = alnum_byte(tmp);

    let mut out = [0u8; 11];
    let mut len = 0;
    for byte in word {
        if byte != b' ' {
            out[len] = byte;
            len += 1;
        }
    }
    String::from_utf8(out[..len].to_vec()).expect("ALPHANUMERIC is ASCII")
}

/// Pack a callsign into a 28-bit value and a boolean portable flag.
pub fn pack_callsign(value: &str, portable_out: Option<&mut bool>) -> u32 {
    let callsign = value.trim();
    let bytes = callsign.as_bytes();

    if matches!(bytes.first().copied(), Some(b'@' | b'<')) && bytes.len() <= 10 && bytes.is_ascii()
    {
        let mut upper = [0u8; 10];
        for (out, &byte) in upper.iter_mut().zip(bytes) {
            *out = byte.to_ascii_uppercase();
        }
        let name = str::from_utf8(&upper[..bytes.len()]).expect("basecalls are ASCII");
        if let Some(&packed) = BASECALLS.get(name) {
            return packed;
        }
    }

    let portable = bytes.len() >= 2
        && bytes[bytes.len() - 2] == b'/'
        && matches!(bytes[bytes.len() - 1], b'P' | b'p');
    let bytes = if portable {
        if let Some(p) = portable_out {
            *p = true;
        }
        &bytes[..bytes.len() - 2]
    } else {
        bytes
    };

    if bytes.len() > 7 || !bytes.is_ascii() {
        return 0;
    }

    let mut call = [0u8; 7];
    for (out, &byte) in call.iter_mut().zip(bytes) {
        *out = byte.to_ascii_uppercase();
    }
    let mut len = bytes.len();

    if call[..len].starts_with(b"3DA0") {
        call[2] = b'0';
        call.copy_within(4..len, 3);
        len -= 1;
    }
    if len >= 3 && call[0] == b'3' && call[1] == b'X' && call[2].is_ascii_uppercase() {
        call[0] = b'Q';
        call.copy_within(2..len, 1);
        len -= 1;
    }

    if !(2..=6).contains(&len) {
        return 0;
    }

    let offset = if len >= 3 && call[2].is_ascii_digit() {
        0
    } else if len <= 5 && call[1].is_ascii_digit() {
        1
    } else {
        return 0;
    };
    let mut word = [b' '; 6];
    word[offset..offset + len].copy_from_slice(&call[..len]);

    if !(word[0] == b' ' || word[0].is_ascii_alphanumeric())
        || !word[1].is_ascii_alphanumeric()
        || !word[2].is_ascii_digit()
        || !word[3..]
            .iter()
            .all(|b| *b == b' ' || b.is_ascii_uppercase())
    {
        return 0;
    }

    let mut packed = alnum_idx(word[0]);
    packed = packed * 36 + alnum_idx(word[1]);
    packed = packed * 10 + alnum_idx(word[2]);
    packed = packed * 27 + alnum_idx(word[3]) - 10;
    packed = packed * 27 + alnum_idx(word[4]) - 10;
    packed = packed * 27 + alnum_idx(word[5]) - 10;

    packed
}

pub fn unpack_callsign(value: u32, portable: bool) -> String {
    if let Some(idx) = value.checked_sub(NBASECALL + 1)
        && let Some(name) = BASECALL_NAMES.get(idx as usize)
    {
        return (*name).to_owned();
    }

    let mut v = value;
    let mut word = [0u8; 6];

    let mut tmp = (v % 27).wrapping_add(10);
    word[5] = alnum_byte(tmp);
    v /= 27;

    tmp = (v % 27).wrapping_add(10);
    word[4] = alnum_byte(tmp);
    v /= 27;

    tmp = (v % 27).wrapping_add(10);
    word[3] = alnum_byte(tmp);
    v /= 27;

    tmp = v % 10;
    word[2] = alnum_byte(tmp);
    v /= 10;

    tmp = v % 36;
    word[1] = alnum_byte(tmp);
    v /= 36;

    word[0] = alnum_byte(v);

    let mut call = [b' '; 7];
    let len = if word.starts_with(b"3D0") {
        call[..4].copy_from_slice(b"3DA0");
        call[4..].copy_from_slice(&word[3..]);
        7
    } else if word[0] == b'Q' && word[1].is_ascii_uppercase() {
        call[..2].copy_from_slice(b"3X");
        call[2..].copy_from_slice(&word[1..]);
        7
    } else {
        call[..6].copy_from_slice(&word);
        6
    };

    let start = call[..len]
        .iter()
        .position(|&byte| byte != b' ')
        .unwrap_or(len);
    let end = call[..len]
        .iter()
        .rposition(|&byte| byte != b' ')
        .map_or(start, |idx| idx + 1);

    let mut out = [0u8; 9];
    let mut out_len = end - start;
    out[..out_len].copy_from_slice(&call[start..end]);
    if portable {
        out[out_len..out_len + 2].copy_from_slice(b"/P");
        out_len += 2;
    }

    String::from_utf8(out[..out_len].to_vec()).expect("callsigns are ASCII")
}

/// Pack a 4-digit maidenhead grid locator into a 15-bit value.
pub fn pack_grid(value: &str) -> u16 {
    let grid = value.trim();
    let bytes = grid.as_bytes();
    if bytes.len() < 4 {
        return (1u16 << 15) - 1;
    }

    let a = i32::from(bytes[0].to_ascii_uppercase()) - i32::from(b'A');
    let b = i32::from(bytes[1].to_ascii_uppercase()) - i32::from(b'A');
    let c = i32::from(bytes[2]) - i32::from(b'0');
    let d = i32::from(bytes[3]) - i32::from(b'0');

    // Match the reference's float-to-int and signed-division behavior for the
    // extended A-X field range without doing floating-point grid conversion.
    let ilong = 178 - 20 * a - 2 * c + i32::from(a >= 9);
    let lon = ilong.midpoint(180);
    (lon * 180 + 10 * b + d) as u16
}

pub fn unpack_grid(value: u16) -> String {
    if value > NBASEGRID {
        return String::new();
    }

    if value == NBASEGRID {
        return "RA90".to_owned();
    }

    let lat = (value % 180) as i16;
    let lon = 179 - (value / 180) as i16;
    let grid = [
        b'A'.wrapping_add((lon / 10) as u8),
        b'A'.wrapping_add((lat / 10) as u8),
        b'0'.wrapping_add((lon % 10) as u8),
        b'0'.wrapping_add((lat % 10) as u8),
    ];
    String::from_utf8(grid.to_vec()).expect("grid alphabet is ASCII")
}

fn call_end(text: &str, start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = start;
    if bytes.get(end) == Some(&b'@') {
        end += 1;
    }
    let first = end;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'/')
    {
        end += 1;
    }
    (end > first).then_some(end)
}

fn skip_space(text: &str, pos: usize) -> usize {
    text[pos..]
        .chars()
        .next()
        .filter(|ch| ch.is_whitespace())
        .map_or(pos, |ch| pos + ch.len_utf8())
}

fn parse_cmd(text: &str, pos: usize) -> Option<(CommandKind, &str, usize)> {
    let start = skip_space(text, pos);
    if let Some((kind, _, end)) = command_prefix(text, start) {
        return Some((kind, &text[pos..end], end));
    }
    (text.as_bytes().get(pos) == Some(&b' ')).then_some((CommandKind::FreeText, " ", pos + 1))
}

fn snr_end(text: &str, pos: usize) -> usize {
    let mut end = skip_space(text, pos);
    let start = end;
    if matches!(text.as_bytes().get(end), Some(b'+' | b'-')) {
        end += 1;
    }
    let digits = end;
    while end < text.len() && end - digits < 2 && text.as_bytes()[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits {
        return pos;
    }
    text[start..end]
        .parse::<i32>()
        .ok()
        .filter(|value| (-31..=31).contains(value))
        .map_or(pos, |_| end)
}

fn heartbeat_parts(text: &str) -> Option<(&str, &str, usize)> {
    let mut pos = text.len() - text.trim_start().len();
    for prefix in ["@ALLCALL", "@HB"] {
        if text[pos..].starts_with(prefix) {
            let end = pos + prefix.len();
            let next = skip_space(text, end);
            if next == end {
                return None;
            }
            pos = next;
            while skip_space(text, pos) != pos {
                pos = skip_space(text, pos);
            }
            break;
        }
    }

    const TYPES: [&str; 10] = [
        "CQ CQ CQ",
        "CQ CONTEST",
        "CQ FIELD",
        "CQ QRP",
        "CQ DX",
        "CQ FD",
        "CQ CQ",
        "CQ",
        "HEARTBEAT",
        "HB",
    ];
    let ty = TYPES.into_iter().find(|ty| {
        text[pos..].starts_with(ty)
            && text
                .as_bytes()
                .get(pos + ty.len())
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
    })?;
    let mut end = pos + ty.len();
    if ty == "HEARTBEAT" && text[end..].trim_start().starts_with("SNR") {
        return None;
    }

    let grid_start = skip_space(text, end);
    let grid_end = grid_start + 4;
    let grid = if grid_start != end
        && text.get(grid_start..grid_end).is_some_and(is_grid)
        && text
            .as_bytes()
            .get(grid_end)
            .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
    {
        end = grid_end;
        &text[grid_start..grid_end]
    } else {
        ""
    };
    Some((ty, grid, end))
}

fn cqs_key_by_value(v: &str, default_key: u32) -> u32 {
    let mut best: Option<u32> = None;
    for (key, candidate) in CQS.entries() {
        if *candidate == v {
            best = Some(best.map_or(*key, |current| current.min(*key)));
        }
    }
    best.unwrap_or(default_key)
}

/// Pack a number or SNR into an integer 0..=62.
pub fn pack_num(num: &str, ok_out: Option<&mut bool>) -> u8 {
    if num.is_empty() {
        if let Some(ok) = ok_out {
            *ok = false;
        }
        return 0;
    }

    let parsed = num.parse::<i32>();
    let (ok, mut inum) = parsed.map_or((false, 0), |v| (true, v));

    // qMax(-30, qMin(inum, 31))
    inum = inum.clamp(-30, 31);

    if let Some(okp) = ok_out {
        *okp = ok;
    }

    (inum + 30 + 1) as u8
}

/// Pack a reduced fidelity command and a number into the extra bits provided between `nbasegrid` and `nmaxgrid`.
pub fn pack_cmd(cmd: u8, num: u8, packed_num_out: Option<&mut bool>) -> u8 {
    let kind = CommandKind::from_wire_code(i32::from(cmd));

    if kind.is_some_and(CommandKind::has_snr) {
        // [1][X][6] where X=0 => SNR, X=1 => HEARTBEAT SNR
        let mut value: u8 = ((1u8 << 1) | u8::from(kind == Some(CommandKind::HeartbeatSnr))) << 6;
        value = value.wrapping_add(num & ((1u8 << 6) - 1));
        if let Some(p) = packed_num_out {
            *p = true;
        }
        value
    } else {
        if let Some(p) = packed_num_out {
            *p = false;
        }
        cmd & ((1u8 << 7) - 1)
    }
}

pub fn unpack_cmd(value: u8, num_out: Option<&mut u8>) -> u8 {
    if (value & (1u8 << 7)) != 0 {
        if let Some(pn) = num_out {
            *pn = value & ((1u8 << 6) - 1);
        }

        if (value & (1u8 << 6)) != 0 {
            CommandKind::HeartbeatSnr as u8
        } else {
            CommandKind::Snr as u8
        }
    } else {
        if let Some(pn) = num_out {
            *pn = 0;
        }
        value & ((1u8 << 7) - 1)
    }
}

pub fn pack_heartbeat_message(text: &str, callsign: &str, n_out: Option<&mut usize>) -> String {
    let Some((ty, extra, consumed)) = heartbeat_parts(text) else {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    };
    let is_alt = ty.starts_with("CQ");

    if callsign.is_empty() {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    }

    let mut packed_extra: u16 = NMAXGRID;

    if !extra.is_empty() {
        packed_extra = pack_grid(extra);
    }

    let mut cq_number = 0;

    if is_alt {
        packed_extra |= 1u16 << 15;
        cq_number = cqs_key_by_value(ty, 0) as u8;
    }

    let frame = pack_compound_frame(
        callsign,
        0u8, /* FrameHeartbeat */
        packed_extra,
        cq_number,
    );
    if frame.is_empty() {
        if let Some(n) = n_out {
            *n = 0;
        }
        return frame;
    }

    if let Some(n) = n_out {
        *n = consumed;
    }
    frame
}

pub fn unpack_heartbeat_message(
    text: &str,
    ty_out: Option<&mut u8>,
    is_alt_out: Option<&mut bool>,
    bits3_out: Option<&mut u8>,
) -> Vec<String> {
    let mut ty: u8 = 0u8;
    let mut num: u16 = NMAXGRID;
    let mut bits3: u8 = 0;

    let mut unpacked = unpack_compound_frame(text, Some(&mut ty), Some(&mut num), Some(&mut bits3));
    if unpacked.is_empty() || ty != 0u8 {
        return Vec::new();
    }

    unpacked.push(unpack_grid(num & ((1u16 << 15) - 1)));

    if let Some(p) = is_alt_out {
        *p = (num & (1u16 << 15)) != 0;
    }
    if let Some(p) = ty_out {
        *p = ty;
    }
    if let Some(p) = bits3_out {
        *p = bits3;
    }

    unpacked
}

pub fn pack_compound_message(text: &str, n_out: Option<&mut usize>) -> String {
    let mut pos = text.len() - text.trim_start().len();
    if text.as_bytes().get(pos) != Some(&b'`') {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    }
    pos += 1;
    let Some(end) = call_end(text, pos) else {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    };
    let callsign = &text[pos..end];
    pos = end;

    let grid_start = skip_space(text, pos);
    let grid_end = grid_start + 4;
    let grid = if text.get(grid_start..grid_end).is_some_and(is_grid) {
        pos = grid_end;
        &text[grid_start..grid_end]
    } else {
        ""
    };
    let command = parse_cmd(text, pos);
    let (kind, cmd, mut consumed) = command.unwrap_or((CommandKind::FreeText, "", pos));
    let num_end = kind
        .has_snr()
        .then(|| snr_end(text, consumed))
        .unwrap_or(consumed);
    let num_str = &text[consumed..num_end];
    consumed = num_end;

    let mut ty: u8 = 1u8;
    let mut extra: u16 = NMAXGRID;

    if !cmd.is_empty() {
        let inum = pack_num(num_str.trim(), None);
        let packed_cmd = pack_cmd(kind.packed_code() as u8, inum, None);
        extra = NUSERGRID.wrapping_add(u16::from(packed_cmd));
        ty = 2u8;
    } else if !grid.is_empty() {
        extra = pack_grid(grid);
    }

    let frame = pack_compound_frame(callsign, ty, extra, 0);

    if let Some(n) = n_out {
        *n = consumed;
    }
    frame
}

pub fn unpack_compound_message(
    text: &str,
    ty_out: Option<&mut u8>,
    bits3_out: Option<&mut u8>,
) -> Vec<String> {
    let mut ty: u8 = 1u8;
    let mut extra: u16 = NMAXGRID;
    let mut bits3: u8 = 0;

    let mut unpacked =
        unpack_compound_frame(text, Some(&mut ty), Some(&mut extra), Some(&mut bits3));
    if unpacked.is_empty() || (ty != 1u8 && ty != 2u8) {
        return Vec::new();
    }

    if extra <= NBASEGRID {
        unpacked.push(format!(" {}", unpack_grid(extra)));
    } else if (NUSERGRID..NMAXGRID).contains(&extra) {
        let mut num: u8 = 0;
        let cmd = unpack_cmd((extra - NUSERGRID) as u8, Some(&mut num));
        let kind = CommandKind::from_wire_code(i32::from(cmd));
        let cmd_str = kind.and_then(CommandKind::wire_token).unwrap_or("");

        unpacked.push(cmd_str.to_string());

        if kind.is_some_and(CommandKind::has_snr) {
            unpacked.push(format_snr(i32::from(num) - 31));
        }
    }

    if let Some(p) = ty_out {
        *p = ty;
    }
    if let Some(p) = bits3_out {
        *p = bits3;
    }

    unpacked
}

pub const FRAME_COMPOUND: u8 = 1;
pub const FRAME_DIRECTED: u8 = 3;
pub const FRAME_DATA: u8 = 4;

pub fn pack_compound_frame(callsign: &str, ty: u8, num: u16, bits3: u8) -> String {
    let frame = String::new();

    // needs to be a compound type...
    if ty == FRAME_DATA || ty == FRAME_DIRECTED {
        return frame;
    }

    let packed_flag: u8 = ty;
    let packed_callsign: u64 = pack_alpha_numeric50(callsign);
    if packed_callsign == 0 {
        return frame;
    }

    let mask11: u16 = ((1u16 << 11) - 1) << 5;
    let mask5: u8 = (1u8 << 5) - 1;

    let packed_11: u16 = (num & mask11) >> 5;
    let packed_5: u8 = (num as u8) & mask5;
    let packed_8: u8 = (packed_5 << 3) | (bits3 & ((1u8 << 3) - 1));

    // [3][50][11],[5][3] = 72
    let value = (u64::from(packed_flag) << 61)
        | ((packed_callsign & ((1u64 << 50) - 1)) << 11)
        | u64::from(packed_11);
    pack72bits(value, packed_8)
}

pub fn unpack_compound_frame(
    text: &str,
    ty_out: Option<&mut u8>,
    num_out: Option<&mut u16>,
    bits3_out: Option<&mut u8>,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if text.chars().count() < 12 || text.contains(' ') {
        return out;
    }

    let mut packed_8: u8 = 0;
    let bits = unpack72bits(text, Some(&mut packed_8));

    let packed_5: u8 = packed_8 >> 3;
    let packed_3: u8 = packed_8 & ((1u8 << 3) - 1);

    let packed_flag: u8 = (bits >> 61) as u8;

    // needs to be a ping type...
    if packed_flag == FRAME_DATA || packed_flag == FRAME_DIRECTED {
        return out;
    }

    let packed_callsign: u64 = (bits >> 11) & ((1u64 << 50) - 1);
    let packed_11: u16 = (bits & 0x07FF) as u16;

    let callsign = unpack_alpha_numeric50(packed_callsign);
    let num: u16 = (packed_11 << 5) | u16::from(packed_5);

    if let Some(p) = ty_out {
        *p = packed_flag;
    }
    if let Some(p) = num_out {
        *p = num;
    }
    if let Some(p) = bits3_out {
        *p = packed_3;
    }

    out.push(callsign);
    out.push(String::new());
    out
}

pub fn pack_directed_message(
    text: &str,
    mycall: &str,
    to_out: Option<&mut String>,
    to_compound_out: Option<&mut bool>,
    cmd_out: Option<&mut String>,
    num_out: Option<&mut String>,
    n_out: Option<&mut usize>,
) -> String {
    let Some(to_end) = call_end(text, 0) else {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    };
    let Some((parsed_kind, cmd, cmd_end)) = parse_cmd(text, to_end) else {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    };
    let num_end = if parsed_kind.has_snr() {
        snr_end(text, cmd_end)
    } else {
        cmd_end
    };
    let num = &text[cmd_end..num_end];
    let from = if is_compound_call(mycall) {
        "<....>"
    } else {
        mycall
    };
    let raw_to = &text[..to_end];

    // ensure we have a valid callsign
    let valid_to_callsign = raw_to != mycall && is_valid_call(raw_to);
    if !valid_to_callsign {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    }

    if let Some(p) = to_out {
        *p = raw_to.to_owned();
    }
    let is_to_compound = is_compound_call(raw_to);
    if let Some(p) = to_compound_out {
        *p = is_to_compound;
    }

    // If compound, replace with placeholder; caller will send actual "to" elsewhere.
    let to = if is_to_compound { "<....>" } else { raw_to };

    // validate command (allow trimmed version as well)
    let Some(kind) = CommandKind::from_wire(cmd).or_else(|| CommandKind::from_wire(cmd.trim()))
    else {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    };

    // packing general number...
    let mut num_ok = false;
    let inum = pack_num(num.trim(), Some(&mut num_ok));
    if num_ok && let Some(p) = num_out {
        *p = num.to_owned();
    }

    let mut portable_from = false;
    let packed_from = pack_callsign(from, Some(&mut portable_from));

    let mut portable_to = false;
    let packed_to = pack_callsign(to, Some(&mut portable_to));

    if packed_from == 0 || packed_to == 0 {
        if let Some(n) = n_out {
            *n = 0;
        }
        return String::new();
    }

    let cmd_out_s = if CommandKind::from_wire(cmd.trim()).is_some() {
        cmd.trim()
    } else {
        cmd
    };
    let packed_cmd = kind.packed_code() as u8;

    let packed_flag: u8 = FRAME_DIRECTED;
    let packed_extra: u8 =
        ((u8::from(portable_from) << 7) + (u8::from(portable_to) << 6)).wrapping_add(inum);

    // [3][28][28][5],[2][6] = 72
    let bits = (u64::from(packed_flag) << 61)
        | (u64::from(packed_from) << 33)
        | (u64::from(packed_to) << 5)
        | u64::from(packed_cmd % 32);

    if let Some(p) = cmd_out {
        *p = cmd_out_s.to_owned();
    }
    if let Some(n) = n_out {
        *n = num_end;
    }

    pack72bits(bits, packed_extra)
}

pub fn unpack_directed_message(text: &str, ty_out: Option<&mut u8>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if text.chars().count() < 12 || text.contains(' ') {
        return out;
    }

    // [3][28][28][5],[2][6] = 72
    let mut extra: u8 = 0;
    let bits = unpack72bits(text, Some(&mut extra));

    let packed_flag: u8 = (bits >> 61) as u8;
    if packed_flag != FRAME_DIRECTED {
        return out;
    }

    let packed_from: u32 = ((bits >> 33) & 0x0FFF_FFFF) as u32;
    let packed_to: u32 = ((bits >> 5) & 0x0FFF_FFFF) as u32;
    let packed_cmd: u8 = (bits & 0x1F) as u8;

    let portable_from = ((extra >> 7) & 1) == 1;
    let portable_to = ((extra >> 6) & 1) == 1;
    extra %= 64;

    let from = unpack_callsign(packed_from, portable_from);
    let to = unpack_callsign(packed_to, portable_to);
    let kind = CommandKind::from_wire_code(i32::from(packed_cmd % 32));
    let cmd = kind
        .and_then(CommandKind::wire_token)
        .unwrap_or("")
        .to_string();

    out.push(from);
    out.push(to);
    out.push(cmd.clone());

    if extra != 0 {
        let v = i32::from(extra) - 31;
        if kind.is_some_and(CommandKind::has_snr) {
            out.push(format_snr(v));
        } else {
            out.push(v.to_string());
        }
    }

    if let Some(p) = ty_out {
        *p = packed_flag;
    }
    out
}

/// Last index of `false`, `QVector`<bool>`::lastIndexOf(0)`.
fn last_index_of_zero(bits: &[bool]) -> Option<usize> {
    (0..bits.len()).rev().find(|&i| !bits[i])
}

fn bits72(value: u64, rem: u8) -> [bool; 72] {
    let mut bits = [false; 72];
    for (index, bit) in bits[..64].iter_mut().enumerate() {
        *bit = value & (1u64 << (63 - index)) != 0;
    }
    for (index, bit) in bits[64..].iter_mut().enumerate() {
        *bit = rem & (1u8 << (7 - index)) != 0;
    }
    bits
}

/// Pack a Huffman-coded message into a 72-bit frame (prefix included if provided).
#[cfg_attr(not(feature = "legacy_pack_data"), allow(dead_code))]
pub fn pack_huff_message(input: &str, prefix: &[bool], n_out: Option<&mut usize>) -> String {
    const FRAME_SIZE: usize = 72;
    let mut bits = 0u128;
    let mut len = 0;

    for &bit in prefix {
        bits = (bits << 1) | u128::from(bit);
        len += 1;
    }

    let mut consumed = 0;
    for ch in input.chars() {
        if !ch.is_ascii() {
            if let Some(n) = n_out {
                *n = 0;
            }
            return String::new();
        }
        let byte = [ch.to_ascii_uppercase() as u8];
        let key = str::from_utf8(&byte).expect("ASCII character");
        let Some(code) = HUFFTABLE.get(key) else {
            if let Some(n) = n_out {
                *n = 0;
            }
            return String::new();
        };
        if len + code.len() >= FRAME_SIZE {
            break;
        }
        for bit in code.bytes() {
            bits = (bits << 1) | u128::from(bit == b'1');
            len += 1;
        }
        consumed += 1;
    }

    let pad = FRAME_SIZE - len;
    if pad != 0 {
        bits <<= pad;
        if pad > 1 {
            bits |= (1u128 << (pad - 1)) - 1;
        }
    }

    if let Some(n) = n_out {
        *n = consumed;
    }
    pack72bits((bits >> 8) as u64, bits as u8)
}

/// Pack a compressed (dense-coded) message into a 72-bit frame (prefix included if provided).
pub fn pack_compressed_message(input: &str, prefix: &[bool], n_out: Option<&mut usize>) -> String {
    let (value, rem, consumed) = compress_frame(input, prefix);
    let frame = pack72bits(value, rem);

    if let Some(n) = n_out {
        *n = consumed;
    }
    frame
}

// DEPRECATED in 2.2: pack data message using 70 bits available flagged as data by the first 2 bits
#[cfg(feature = "legacy_pack_data")]
pub fn pack_data_message(input: &str, n_out: Option<&mut usize>) -> String {
    let mut huff_chars: usize = 0;
    let huff_frame = pack_huff_message(input, &[true, false], Some(&mut huff_chars));

    let mut compressed_chars: usize = 0;
    let compressed_frame =
        pack_compressed_message(input, &[true, true], Some(&mut compressed_chars));

    if huff_chars > compressed_chars {
        if let Some(n) = n_out {
            *n = huff_chars;
        }
        huff_frame
    } else {
        if let Some(n) = n_out {
            *n = compressed_chars;
        }
        compressed_frame
    }
}

// DEPRECATED in 2.2: unpack legacy 70-bit data message (flagged in first 2 bits)
pub fn unpack_data_message(text: &str) -> String {
    let mut unpacked = String::new();

    if text.chars().count() < 12 || text.contains(' ') {
        return unpacked;
    }

    let mut rem: u8 = 0;
    let value: u64 = unpack72bits(text, Some(&mut rem));

    let bits = bits72(value, rem);

    let is_data = bits[0];
    if !is_data {
        return unpacked;
    }

    let bits = &bits[1..];

    let compressed = bits[0];
    let n = match last_index_of_zero(bits) {
        Some(v) => v,
        None => return unpacked,
    };

    // trim off the pad bits: mid(1, n-1)
    if n < 2 {
        return unpacked;
    }
    let bits = &bits[1..n];

    if compressed {
        unpacked = decompress(bits);
    } else {
        unpacked = huff_decode(bits);
    }

    unpacked
}

/// Pack data message using the full 72 bits available (with the data flag in the i3bit header)
pub fn pack_fast_data_message(input: &str, n_out: Option<&mut usize>) -> String {
    pack_compressed_message(input, &[], n_out)
}

/// Unpack data message using the full 72 bits available (with the data flag in the i3bit header)
pub fn unpack_fast_data_message(text: &str) -> String {
    let mut unpacked = String::new();

    if text.chars().count() < 12 || text.contains(' ') {
        return unpacked;
    }

    let mut rem: u8 = 0;
    let value: u64 = unpack72bits(text, Some(&mut rem));
    let bits = bits72(value, rem);

    let n = match last_index_of_zero(&bits) {
        Some(v) => v,
        None => return unpacked,
    };

    unpacked = decompress(&bits[..n]);

    unpacked
}

pub const ITYPE_JS8CALL: u8 = FrameFlags::NONE.bits();
pub const ITYPE_JS8CALL_FIRST: u8 = FrameFlags::FIRST.bits();
pub const ITYPE_JS8CALL_LAST: u8 = FrameFlags::LAST.bits();
pub const ITYPE_JS8CALL_DATA: u8 = FrameFlags::DATA.bits();

fn ascii_trim_start(text: &str) -> Result<&[u8], ()> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    if bytes.get(index).is_some_and(|byte| !byte.is_ascii()) {
        Err(())
    } else {
        Ok(&bytes[index..])
    }
}

fn could_be_heartbeat(text: &str) -> bool {
    let Ok(text) = ascii_trim_start(text) else {
        return true;
    };
    text.starts_with(b"CQ")
        || text.starts_with(b"HB")
        || text.starts_with(b"HEARTBEAT")
        || text.starts_with(b"@ALLCALL")
        || text.starts_with(b"@HB")
}

fn could_be_compound(text: &str) -> bool {
    match ascii_trim_start(text) {
        Ok(text) => text.first() == Some(&b'`'),
        Err(()) => true,
    }
}

fn could_be_directed(text: &str) -> bool {
    let mut previous = 0;
    for &byte in text.as_bytes() {
        if matches!(byte, b'/' | b'@') {
            return true;
        }
        let class = if byte.is_ascii_uppercase() {
            1
        } else if byte.is_ascii_digit() {
            2
        } else {
            break;
        };
        if previous + class == 3 && previous != 0 {
            return true;
        }
        previous = class;
    }
    false
}

fn starts_with_call(text: &str) -> bool {
    call_end(text, 0).is_some_and(|end| end > 3 && is_valid_call(&text[..end]))
}

pub fn build_message_frames(
    mycall: &str,
    mygrid: &str,
    selected_call: &str,
    text: &str,
    force_identify_in: bool,
    force_data: bool,
    submode: Submode,
) -> Vec<(String, u8)> {
    // Enabled:
    // ALLOW_SEND_COMPOUND, ALLOW_SEND_COMPOUND_DIRECTED, AUTO_PREPEND_DIRECTED,
    // AUTO_REMOVE_MYCALL, AUTO_PREPEND_DIRECTED_ALLOW_TEXT_CALLSIGNS, ALLOW_FORCE_IDENTIFY,
    // AUTO_RSTRIP_WHITESPACE, and checksum section (#if 1).
    let mut frames = Vec::new();
    let mut line = text.to_owned();
    let mut data_only = force_data;
    let force_identify = force_identify_in && !force_data;

    // AUTO_REMOVE_MYCALL
    if line.starts_with(mycall) && matches!(line.as_bytes().get(mycall.len()), Some(b':' | b' ')) {
        line = line[mycall.len() + 1..].trim_start().to_owned();
    }

    // AUTO_RSTRIP_WHITESPACE
    if !line.trim_end().is_empty() {
        line.truncate(line.trim_end().len());
    }

    // AUTO_PREPEND_DIRECTED
    if !selected_call.is_empty()
        && !line.starts_with(selected_call)
        && !line.starts_with('`')
        && !force_data
    {
        let starts_with_base =
            line.starts_with("@ALLCALL") || starts_with_cq(&line) || starts_with_hb(&line);
        if !(starts_with_base || starts_with_call(&line)) {
            let sep = if line.starts_with(' ') { "" } else { " " };
            line = format!("{selected_call}{sep}{line}");
        }
    }

    while !line.is_empty() {
        let mut dir_to = String::new();
        if !data_only {
            let mut l: usize = 0;
            let bcn_frame = if could_be_heartbeat(&line) {
                pack_heartbeat_message(&line, mycall, Some(&mut l))
            } else {
                String::new()
            };
            if l > 0 {
                frames.push((bcn_frame, ITYPE_JS8CALL));
                line = line[l..].to_owned();
                continue;
            }

            let mut o: usize = 0;
            let cmp_frame = if could_be_compound(&line) {
                pack_compound_message(&line, Some(&mut o))
            } else {
                String::new()
            };
            if o > 0 {
                frames.push((cmp_frame, ITYPE_JS8CALL));
                line = line[o..].to_owned();
                continue;
            }

            let mut n: usize = 0;
            let mut dir_cmd = String::new();
            let mut dir_num = String::new();
            let mut dir_to_compound = false;
            let dir_frame = if could_be_directed(&line) {
                pack_directed_message(
                    &line,
                    mycall,
                    Some(&mut dir_to),
                    Some(&mut dir_to_compound),
                    Some(&mut dir_cmd),
                    Some(&mut dir_num),
                    Some(&mut n),
                )
            } else {
                String::new()
            };
            if n > 0 {
                // ALLOW_SEND_COMPOUND_DIRECTED true-path
                if is_compound_call(mycall) || dir_to_compound {
                    // Send a DE compound frame first
                    let de_compound_message = format!("`{mycall} {mygrid}");
                    let de_compound_frame = pack_compound_message(&de_compound_message, None);
                    if !de_compound_frame.is_empty() {
                        frames.push((de_compound_frame, ITYPE_JS8CALL));
                    }

                    // Followed by a compound-directed (encoded as compound message) frame
                    let dir_compound_message = format!("`{dir_to}{dir_cmd}{dir_num}");
                    let dir_compound_frame = pack_compound_message(&dir_compound_message, None);
                    if !dir_compound_frame.is_empty() {
                        frames.push((dir_compound_frame, ITYPE_JS8CALL));
                    }
                } else {
                    frames.push((dir_frame, ITYPE_JS8CALL));
                }

                line = line[n..].to_owned();

                // buffered command checksum handling
                if is_buffered_token(&dir_cmd) && !line.is_empty() {
                    line = line.trim_start().to_owned();

                    let skip_aprs_checksum = dir_to.eq_ignore_ascii_case("@APRSIS")
                        && matches!(dir_cmd.as_str(), " MSG" | " MSG TO:");
                    let checksum_size = if skip_aprs_checksum {
                        0
                    } else {
                        CommandKind::from_wire(&dir_cmd).map_or(0, CommandKind::checksum_bits)
                    };

                    if checksum_size == 32 {
                        let cs = checksum32(&line);
                        line = format!("{line} {cs}");
                    } else if checksum_size == 16 {
                        let cs = checksum16(&line);
                        line = format!("{line} {cs}");
                    }
                }
                data_only = true;
                continue;
            }

            if force_identify
                && frames.is_empty()
                && selected_call.is_empty()
                && dir_to.is_empty()
                && !line.contains(mycall)
            {
                line = format!("{mycall}: {line}");
            }
        }

        let mut m = 0;
        #[cfg(feature = "legacy_pack_data")]
        let (frame, fast) = if submode == Submode::Normal {
            (pack_data_message(&line, Some(&mut m)), false)
        } else {
            (pack_fast_data_message(&line, Some(&mut m)), true)
        };
        #[cfg(not(feature = "legacy_pack_data"))]
        let (frame, fast) = {
            let _ = submode;
            (pack_fast_data_message(&line, Some(&mut m)), true)
        };

        if m == 0 {
            break;
        }
        let itype = if fast {
            ITYPE_JS8CALL_DATA
        } else {
            ITYPE_JS8CALL
        };
        frames.push((frame, itype));
        line = line[m..].to_owned();
        data_only = true;
    }

    if !frames.is_empty() {
        frames[0].1 |= ITYPE_JS8CALL_FIRST;
        let last = frames.len() - 1;
        frames[last].1 |= ITYPE_JS8CALL_LAST;
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_has_bit(bits: u8, flag: FrameFlags) -> bool {
        FrameFlags::from_bits_truncate(bits).contains(flag)
    }

    #[test]
    fn checksum_validators_accept_and_reject_expected_values() {
        let msg = "HELLO WORLD";
        let cs16 = checksum16(msg);
        let cs32 = checksum32(msg);

        assert!(checksum16_valid(&cs16, msg));
        assert!(checksum32_valid(&cs32, msg));
        assert!(!checksum16_valid("AAA", msg));
        assert!(!checksum32_valid("AAAAAA", msg));
    }

    #[test]
    fn pack72_unpack72_roundtrip() {
        let value = 0x1234_5678_9ABC_DEF0u64 >> 4;
        let rem = 0b1010_0110u8;
        let packed = pack72bits(value, rem);

        let mut decoded_rem = 0u8;
        let decoded_value = unpack72bits(&packed, Some(&mut decoded_rem));

        assert_eq!(decoded_value, value);
        assert_eq!(decoded_rem, rem);
    }

    #[test]
    fn fixed_72_bit_buffer_matches_dynamic_conversion() {
        let mut value = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..1024 {
            value ^= value << 13;
            value ^= value >> 7;
            value ^= value << 17;
            let rem = value as u8;

            let mut expected = int_to_bits(value, 64);
            expected.extend(int_to_bits(u64::from(rem), 8));
            assert_eq!(bits72(value, rem).as_slice(), expected);
        }
    }

    #[test]
    fn pack_num_clamps_and_reports_ok() {
        let mut ok = false;
        assert_eq!(pack_num("-99", Some(&mut ok)), 1);
        assert!(ok);
        assert_eq!(pack_num("99", Some(&mut ok)), 62);
        assert!(ok);
        assert_eq!(pack_num("abc", Some(&mut ok)), 31);
        assert!(!ok);
        assert_eq!(pack_num("", Some(&mut ok)), 0);
        assert!(!ok);
    }

    #[test]
    fn pack_unpack_cmd_preserves_snr_and_regular_forms() {
        let mut packed_num = false;
        let packed_snr = pack_cmd(25, 40, Some(&mut packed_num));
        assert!(packed_num);

        let mut unpacked_num = 0u8;
        let unpacked_cmd = unpack_cmd(packed_snr, Some(&mut unpacked_num));
        assert_eq!(unpacked_cmd, 25);
        assert_eq!(unpacked_num, 40);

        let packed_regular = pack_cmd(9, 12, Some(&mut packed_num));
        assert!(!packed_num);
        let unpacked_regular = unpack_cmd(packed_regular, Some(&mut unpacked_num));
        assert_eq!(unpacked_regular, 9);
        assert_eq!(unpacked_num, 0);
    }

    #[test]
    fn callsign_pack_unpack_roundtrip_and_portable() {
        for (callsign, packed) in [
            ("000AAA", 0),
            ("3DA0", 23_836_112),
            ("3XA1BC", 186_221_672),
            ("3X1ABC", 27_772_742),
            ("A1", 257_099_345),
            ("K1ABC", 259_047_992),
        ] {
            assert_eq!(pack_callsign(callsign, None), packed);
            assert_eq!(unpack_callsign(packed, false), callsign);
        }

        let mut portable = false;
        let packed = pack_callsign("K1ABC", Some(&mut portable));
        assert!(!portable);
        assert_eq!(unpack_callsign(packed, false), "K1ABC");

        let packed_portable = pack_callsign("K1ABC/P", Some(&mut portable));
        assert!(portable);
        assert_eq!(unpack_callsign(packed_portable, true), "K1ABC/P");

        portable = false;
        assert_eq!(pack_callsign("@ALLCALL/P", Some(&mut portable)), 0);
        assert!(portable);

        portable = true;
        assert_eq!(pack_callsign("K1ABC", Some(&mut portable)), packed);
        assert!(portable);

        assert_eq!(
            unpack_callsign(pack_callsign("3D0ABC", None), false),
            "3DA0ABC"
        );
        assert_eq!(
            unpack_callsign(pack_callsign("QA1BC", None), false),
            "3XA1BC"
        );
        assert_eq!(pack_callsign("3X", None), 0);
        assert_eq!(pack_callsign("3XA", None), 0);
    }

    #[test]
    fn basecall_lookup_matches_numeric_table() {
        assert_eq!(BASECALLS.len(), BASECALL_NAMES.len());
        for (name, value) in BASECALLS.entries() {
            let idx = (*value - NBASECALL - 1) as usize;
            assert_eq!(BASECALL_NAMES[idx], *name);
            assert_eq!(pack_callsign(&name.to_ascii_lowercase(), None), *value);
            assert_eq!(unpack_callsign(*value, false), *name);
            assert_eq!(unpack_callsign(*value, true), *name);
        }
    }

    #[test]
    fn alpha_numeric50_layout_roundtrips() {
        for value in ["KN4CRD/QRP", "VE3/LB9YHX", "@RACES", "K1ABC"] {
            assert_eq!(unpack_alpha_numeric50(pack_alpha_numeric50(value)), value);
        }

        assert_eq!(pack_alpha_numeric50("abc"), pack_alpha_numeric50(""));
        assert_eq!(
            unpack_alpha_numeric50(pack_alpha_numeric50("K!N4-CRD/QRP")),
            "KN4CRD/QRP"
        );
    }

    #[test]
    fn grid_pack_unpack_roundtrip() {
        let packed = pack_grid("EM73");
        assert_eq!(unpack_grid(packed), "EM73");

        for a in b'A'..=b'R' {
            for b in b'A'..=b'R' {
                for c in b'0'..=b'9' {
                    for d in b'0'..=b'9' {
                        let grid = String::from_utf8(vec![a, b, c, d]).unwrap();
                        assert_eq!(unpack_grid(pack_grid(&grid)), grid);
                    }
                }
            }
        }

        assert_eq!(pack_grid("SA00"), 0);
        assert_eq!(unpack_grid(NBASEGRID), "RA90");
        assert!(unpack_grid(NBASEGRID + 1).is_empty());
    }

    #[test]
    fn heartbeat_pack_unpack_regular_and_alt() {
        let hb = pack_heartbeat_message("HB EM73", "K1ABC", None);
        let mut ty = 255u8;
        let mut is_alt = false;
        let mut bits3 = 0u8;
        let unpacked =
            unpack_heartbeat_message(&hb, Some(&mut ty), Some(&mut is_alt), Some(&mut bits3));
        assert!(!unpacked.is_empty());
        assert_eq!(ty, FrameType::FrameHeartbeat as u8);
        assert!(!is_alt);

        let cq = pack_heartbeat_message("CQ CQ CQ EM73", "K1ABC", None);
        let unpacked_cq =
            unpack_heartbeat_message(&cq, Some(&mut ty), Some(&mut is_alt), Some(&mut bits3));
        assert!(!unpacked_cq.is_empty());
        assert!(is_alt);
    }

    #[test]
    fn compound_message_pack_unpack_grid_and_directed() {
        let grid_frame = pack_compound_message("`K1ABC EM73", None);
        assert_eq!(grid_frame, "AURtg4DOOkfO");
        let mut ty = 255u8;
        let unpacked_grid = unpack_compound_message(&grid_frame, Some(&mut ty), None);
        assert_eq!(ty, FrameType::FrameCompound as u8);
        assert!(!unpacked_grid.is_empty());
        assert!(unpacked_grid.iter().any(|p| p.contains("EM73")));

        let directed_frame = pack_compound_message("`K1ABC MSG", None);
        assert_eq!(directed_frame, "IURtg4DOO+KO");
        let unpacked_directed = unpack_compound_message(&directed_frame, Some(&mut ty), None);
        assert_eq!(ty, FrameType::FrameCompoundDirected as u8);
        assert!(!unpacked_directed.is_empty());
        assert!(unpacked_directed.iter().any(|p| p.contains("MSG")));
    }

    #[test]
    fn directed_message_pack_unpack_standard_and_compound_to() {
        let mut to = String::new();
        let mut to_compound = false;
        let mut cmd = String::new();
        let mut num = String::new();
        let mut consumed = 0usize;

        let frame = pack_directed_message(
            "K1ABC MSG HELLO",
            "N0CALL",
            Some(&mut to),
            Some(&mut to_compound),
            Some(&mut cmd),
            Some(&mut num),
            Some(&mut consumed),
        );
        assert_eq!(frame, "Vq46C-uOHma0");
        assert!(!frame.is_empty());
        assert_eq!(to, "K1ABC");
        assert!(!to_compound);
        assert_eq!(cmd.trim(), "MSG");
        assert!(consumed > 0);

        let mut ty = 255u8;
        let unpacked = unpack_directed_message(&frame, Some(&mut ty));
        assert_eq!(ty, FrameType::FrameDirected as u8);
        assert_eq!(unpacked[0], "<....>");
        assert_eq!(unpacked[1], "K1ABC");
        assert_eq!(unpacked[2].trim(), "MSG");

        let compound_to_frame = pack_directed_message(
            "EA8/K1ABC MSG HELLO",
            "N0CALL",
            Some(&mut to),
            Some(&mut to_compound),
            Some(&mut cmd),
            Some(&mut num),
            Some(&mut consumed),
        );
        assert_eq!(compound_to_frame, "Vq46C+GGOoa0");
        assert!(!compound_to_frame.is_empty());
        assert!(to_compound);
        let unpacked_compound = unpack_directed_message(&compound_to_frame, Some(&mut ty));
        assert_eq!(unpacked_compound[1], "<....>");
    }

    #[test]
    fn fast_data_pack_unpack_roundtrip() {
        let input = "HELLO WORLD";
        let frame = pack_fast_data_message(input, None);
        assert_eq!(unpack_fast_data_message(&frame), input);

        for (value, expected_frame, expected_consumed) in [
            ("E", "0+++++++++++", 1),
            ("HELLO WORLD", "UBXRtA7+++++", 11),
            (
                "THIS IS A LONG BENCHMARKING MESSAGE TO TEST JS8RS FRAME PACKING",
                "-beQL+q+lCBV",
                28,
            ),
            ("K1ABC MSG HELLO 123", "abWUETSkb+++", 10),
            ("  REPEATED  SPACES  ", "z7ekxa+eUUyh", 19),
        ] {
            let mut consumed = 0;
            let frame = pack_fast_data_message(value, Some(&mut consumed));
            assert_eq!(frame, expected_frame);
            assert_eq!(consumed, expected_consumed);
        }
    }

    #[cfg(feature = "legacy_pack_data")]
    #[test]
    fn legacy_data_pack_unpack_roundtrip() {
        let input = "HELLO WORLD";
        let frame = pack_data_message(input, None);
        assert_eq!(unpack_data_message(&frame), input);
    }

    #[test]
    fn command_classification_matches_expected_tables() {
        assert_eq!(CommandKind::from_wire(" MSG"), Some(CommandKind::Msg));
        assert!(is_buffered_token(" MSG"));
        assert_eq!(CommandKind::Msg.checksum_bits(), 16);

        assert_eq!(CommandKind::from_wire(" GRID"), Some(CommandKind::Grid));
        assert!(is_buffered_token(" GRID"));
        assert_eq!(CommandKind::Grid.checksum_bits(), 0);

        assert_eq!(CommandKind::from_wire(" NOT_A_CMD"), None);
        assert!(!is_buffered_token(" NOT_A_CMD"));

        for code in -1..=31 {
            let kind = CommandKind::from_wire_code(code).expect("known command code");
            assert_eq!(kind.packed_code(), code, "command code {code}");
            assert!(
                kind.wire_token()
                    .is_some_and(|token| CommandKind::from_wire(token).is_some())
            );
        }
        for code in [i32::MIN, -2, 32, i32::MAX] {
            assert_eq!(CommandKind::from_wire_code(code), None);
        }

        for text in ["", "C", "CQ", "CQX", "CQ CQ CQ EM73", "xCQ", "HB", "HBX"] {
            let expected_cq = CQS.entries().any(|(_, value)| text.starts_with(*value));
            let expected_hb = text.starts_with("HB");
            assert_eq!(starts_with_cq(text), expected_cq, "CQ prefix {text:?}");
            assert_eq!(starts_with_hb(text), expected_hb, "HB prefix {text:?}");
        }
    }

    #[test]
    fn frame_candidate_gates_cover_valid_forms() {
        for text in [
            "CQ EM73",
            "HB EM73",
            "HEARTBEAT",
            "@HB HB",
            "\u{2003}HB EM73",
        ] {
            assert!(could_be_heartbeat(text), "heartbeat {text:?}");
        }
        for text in ["`K1ABC EM73", " \t`K1ABC MSG", "\u{2003}`K1ABC EM73"] {
            assert!(could_be_compound(text), "compound {text:?}");
        }
        for text in [
            "K2XYZ MSG",
            "ABC/DEF MSG",
            "@APRSIS MSG",
            "K2XYZ HELLO",
            "K2XYZ  HELLO",
        ] {
            assert!(could_be_directed(text), "directed {text:?}");
        }
        for text in ["HELLO WORLD", "FOOBAR MSG", " LOWER"] {
            assert!(!could_be_directed(text), "non-directed {text:?}");
        }
    }

    #[test]
    fn build_message_frames_marks_first_and_last_bits() {
        let frames = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "HELLO WORLD",
            false,
            false,
            Submode::Fast,
        );
        assert!(!frames.is_empty());

        let first_bits = frames.first().unwrap().1;
        let last_bits = frames.last().unwrap().1;
        assert!(frame_has_bit(first_bits, FrameFlags::FIRST));
        assert!(frame_has_bit(last_bits, FrameFlags::LAST));
    }

    #[test]
    fn build_message_frames_force_data_uses_data_flagged_frames() {
        let frames = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "THIS IS A LONG MESSAGE THAT SHOULD BE ENCODED AS DATA",
            false,
            true,
            Submode::Normal,
        );
        #[cfg(not(feature = "legacy_pack_data"))]
        assert!(
            frames
                .iter()
                .any(|(_, bits)| frame_has_bit(*bits, FrameFlags::DATA))
        );
        #[cfg(feature = "legacy_pack_data")]
        assert!(
            frames
                .iter()
                .any(|(_, bits)| !frame_has_bit(*bits, FrameFlags::DATA))
        );
    }

    #[test]
    fn build_message_frames_selected_call_auto_prepends_directed_target() {
        let frames = build_message_frames(
            "K1ABC",
            "EM73",
            "K2XYZ",
            "HELLO THERE",
            false,
            false,
            Submode::Fast,
        );
        assert!(!frames.is_empty());

        let directed = unpack_directed_message(&frames[0].0, None);
        assert!(!directed.is_empty());
        assert_eq!(directed[0], "K1ABC");
        assert_eq!(directed[1], "K2XYZ");
    }

    #[test]
    fn build_message_frames_removes_mycall_prefix_when_present() {
        let without_prefix = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "MSG HELLO",
            false,
            false,
            Submode::Fast,
        );
        let with_prefix = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "K1ABC: MSG HELLO",
            false,
            false,
            Submode::Fast,
        );

        assert_eq!(with_prefix, without_prefix);
    }

    #[test]
    fn build_message_frames_strips_trailing_whitespace() {
        let clean = build_message_frames("K1ABC", "EM73", "", "HELLO", false, false, Submode::Fast);
        let spaced = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "HELLO    ",
            false,
            false,
            Submode::Fast,
        );
        assert_eq!(clean, spaced);
    }

    #[test]
    fn build_message_frames_stops_on_unsupported_input() {
        let frames = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "\u{2603}",
            false,
            false,
            Submode::Normal,
        );

        assert!(frames.is_empty());
    }

    #[test]
    fn build_message_frames_only_skips_aprsis_message_checksums() {
        let aprs = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "@APRSIS MSG HELLO",
            false,
            false,
            Submode::Normal,
        );
        let aprs_cmd = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "@APRSIS CMD HELLO",
            false,
            false,
            Submode::Normal,
        );
        let js8net = build_message_frames(
            "K1ABC",
            "EM73",
            "",
            "@JS8NET MSG HELLO",
            false,
            false,
            Submode::Normal,
        );

        let mut aprs_payload = String::new();
        let mut aprs_cmd_payload = String::new();
        let mut js8net_payload = String::new();

        for (frame, bits) in aprs {
            let dt = crate::codec::parse_frame(
                &frame,
                FrameFlags::from_bits_truncate(bits),
                Submode::Normal,
            );
            if dt.frame_type == FrameType::FrameData {
                aprs_payload.push_str(&dt.message);
            }
        }
        for (frame, bits) in aprs_cmd {
            let dt = crate::codec::parse_frame(
                &frame,
                FrameFlags::from_bits_truncate(bits),
                Submode::Normal,
            );
            if dt.frame_type == FrameType::FrameData {
                aprs_cmd_payload.push_str(&dt.message);
            }
        }
        for (frame, bits) in js8net {
            let dt = crate::codec::parse_frame(
                &frame,
                FrameFlags::from_bits_truncate(bits),
                Submode::Normal,
            );
            if dt.frame_type == FrameType::FrameData {
                js8net_payload.push_str(&dt.message);
            }
        }

        assert_eq!(aprs_payload.split_whitespace().count(), 1);
        assert_eq!(
            aprs_cmd_payload.trim_end(),
            format!("HELLO {}", checksum16("HELLO"))
        );
        assert!(js8net_payload.split_whitespace().count() > 1);
    }
}
