// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JSC compression to Rust and replaced dynamic maps with packed tables.

use std::{cmp::Ordering, collections::VecDeque};

use crate::varicode::{
    bits_to_int,
    jsc_tables::{self, LookupSpan},
};

const FRAME_BITS: usize = 72;
const S: u32 = 7;
const C: u32 = 9;

fn codeword(index: u32, separate: bool) -> (u64, usize) {
    let mut word = u64::from(((index % S) << 1) + u32::from(separate));
    let mut len = 5;

    let mut x = index / S;
    while x > 0 {
        x -= 1;
        word |= u64::from((x % C) + S) << len;
        len += 4;
        x /= C;
    }

    (word, len)
}

pub fn compress_frame(text: &str, prefix: &[bool]) -> (u64, u8, usize) {
    let mut bits = 0u128;
    let mut bit_len = prefix.len();
    let mut consumed = 0usize;

    for &bit in prefix {
        bits = (bits << 1) | u128::from(bit);
    }

    let map = jsc_tables::map_table();
    let mut words = text.split(' ').peekable();
    while let Some(raw) = words.next() {
        let is_last_word = words.peek().is_none();
        let is_space = raw.is_empty() && !is_last_word;
        let encoded = (!is_space).then(|| latin1_bytes(raw));
        let word = encoded.as_deref().unwrap_or(b" ");
        let mut pos = 0usize;

        while pos < word.len() {
            let Some(index) = lookup_bytes(&word[pos..]) else {
                break;
            };
            let Some(entry) = map.get(index) else {
                break;
            };
            let size = entry.size as usize;
            if size > word.len() - pos {
                break;
            }

            let is_last = pos + size == word.len();
            let separate = is_last && !is_space && !is_last_word;
            let (code, code_len) = codeword(index, separate);
            if bit_len + code_len >= FRAME_BITS {
                return finish_frame(bits, bit_len, consumed);
            }

            bits = (bits << code_len) | u128::from(code);
            bit_len += code_len;
            consumed += size + usize::from(separate);
            pos += size;
        }
    }

    finish_frame(bits, bit_len, consumed)
}

fn finish_frame(mut bits: u128, bit_len: usize, consumed: usize) -> (u64, u8, usize) {
    let pad = FRAME_BITS - bit_len;
    if pad != 0 {
        bits = (bits << pad) | ((1u128 << (pad - 1)) - 1);
    }
    ((bits >> 8) as u64, bits as u8, consumed)
}

#[inline]
fn latin1_bytes(s: &str) -> Vec<u8> {
    s.chars().map(|ch| ch as u32 as u8).collect()
}

pub fn decompress(bitvec: &[bool]) -> String {
    let b: u32 = 4;
    let s: u64 = 7;
    let c: u64 = (1u64 << b) - s;

    let map = jsc_tables::map_table();
    let size_limit = u64::from(map.count());

    let mut base = [0u64; 8];
    base[0] = 0;
    base[1] = s;
    base[2] = base[1] + s * c;
    base[3] = base[2] + s * c * c;
    base[4] = base[3] + s * c * c * c;
    base[5] = base[4] + s * c * c * c * c;
    base[6] = base[5] + s * c * c * c * c * c;
    base[7] = base[6] + s * c * c * c * c * c * c;

    let mut bytes: Vec<u64> = Vec::new();
    let mut separators: VecDeque<usize> = VecDeque::new();

    let mut i: usize = 0;
    while i < bitvec.len() {
        let end = i + 4;
        if end > bitvec.len() {
            break;
        }

        let byte = bits_to_int(&bitvec[i..end]);
        bytes.push(byte);
        i += 4;

        if byte < s {
            if i < bitvec.len() && bitvec[i] {
                separators.push_back(bytes.len() - 1);
            }
            i += 1;
        }
    }
    let mut out = String::new();

    let mut start: usize = 0;
    while start < bytes.len() {
        let mut k: usize = 0;
        let mut j: u64 = 0;

        while start + k < bytes.len() && bytes[start + k] >= s {
            j = j * c + (bytes[start + k] - s);
            k += 1;
        }

        if j >= size_limit {
            break;
        }
        if start + k >= bytes.len() {
            break;
        }

        j = j * s + bytes[start + k] + base.get(k).copied().unwrap_or(0);

        if j >= size_limit {
            break;
        }

        let t = match map.get(j as u32) {
            Some(v) => v,
            None => break,
        };

        let word_bytes = jsc_tables::BinTable::text_trimmed(t);
        push_latin1_into(&mut out, word_bytes);

        if let Some(&sep_at) = separators.front()
            && sep_at == start + k
        {
            out.push(' ');
            separators.pop_front();
        }

        start += k + 1;
    }

    out
}

fn lookup_bytes(b: &[u8]) -> Option<u32> {
    let first = *b.first()?;
    let lookup = jsc_tables::lookup_table();
    let route = lookup.route(first);
    if route == jsc_tables::NO_ROUTE {
        return None;
    }
    if route & jsc_tables::DIRECT_ROUTE != 0 {
        return Some(route & jsc_tables::INDEX_MASK);
    }

    let first_span = route & 0xffff;
    let span_count = route >> 16;
    for span_index in first_span..first_span + span_count {
        let span = lookup.span(span_index)?;
        let mut sizes = span.sizes;
        let mut best = (u32::MAX, 0);

        while sizes != 0 {
            let len = sizes.trailing_zeros() as usize;
            if len > b.len() {
                break;
            }
            if let Some(candidate) = find_rank(&b[..len], span)
                && candidate.0 < best.0
            {
                best = candidate;
            }
            sizes &= sizes - 1;
        }

        if best.0 != u32::MAX {
            return Some(best.1);
        }
    }

    None
}

fn find_rank(needle: &[u8], span: LookupSpan) -> Option<(u32, u32)> {
    let ranks = jsc_tables::rank_table();
    let map = jsc_tables::map_table();
    let mut low = span.start;
    let mut high = span.start + span.count;

    // Find the first key <= needle in a descending span. Continuing left on
    // equality also preserves the earliest rank if a generated table has duplicates.
    while low < high {
        let mid = low + (high - low) / 2;
        let rank = ranks.get(mid)?;
        let entry = map.get(rank.index)?;
        let key = entry.text.get(..rank.size as usize)?;
        if key.cmp(needle) == Ordering::Greater {
            low = mid + 1;
        } else {
            high = mid;
        }
    }

    let rank = ranks.get(low)?;
    let entry = map.get(rank.index)?;
    (entry.text.get(..rank.size as usize)? == needle).then_some((low, rank.index))
}

#[inline]
fn push_latin1_into(out: &mut String, latin1: &[u8]) {
    for &b in latin1 {
        out.push(b as char); // 0x00..0xFF -> U+0000..U+00FF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linear_lookup(b: &[u8]) -> Option<u32> {
        let first = *b.first()?;
        let lookup = jsc_tables::lookup_table();
        let route = lookup.route(first);
        if route == jsc_tables::NO_ROUTE {
            return None;
        }
        if route & jsc_tables::DIRECT_ROUTE != 0 {
            return Some(route & jsc_tables::INDEX_MASK);
        }

        let ranks = jsc_tables::rank_table();
        let map = jsc_tables::map_table();
        let first_span = route & 0xffff;
        let span_count = route >> 16;
        for span_index in first_span..first_span + span_count {
            let span = lookup.span(span_index).unwrap();
            for rank_index in span.start..span.start + span.count {
                let rank = ranks.get(rank_index).unwrap();
                let entry = map.get(rank.index).unwrap();
                let len = rank.size as usize;
                if b.len() >= len && b[..len] == entry.text[..len] {
                    return Some(rank.index);
                }
            }
        }
        None
    }

    fn assert_matches_oracle(query: &[u8]) {
        assert_eq!(lookup_bytes(query), linear_lookup(query), "query={query:?}");
    }

    #[test]
    fn full_prefix_needs_no_padding() {
        let prefix = [true; FRAME_BITS];
        assert_eq!(compress_frame("", &prefix), (u64::MAX, u8::MAX, 0));
    }

    #[test]
    fn packed_index_is_structurally_valid() {
        let ranks = jsc_tables::rank_table();
        let map = jsc_tables::map_table();
        let lookup = jsc_tables::lookup_table();
        assert_eq!(ranks.count(), map.count());

        let mut seen = vec![false; map.count() as usize];
        for rank_index in 0..ranks.count() {
            let rank = ranks.get(rank_index).unwrap();
            let entry = map.get(rank.index).unwrap();
            assert!((rank.size as usize) <= jsc_tables::BinTable::text_trimmed(entry).len());
            assert!(
                !seen[rank.index as usize],
                "duplicate map index {}",
                rank.index
            );
            seen[rank.index as usize] = true;
        }
        assert!(seen.into_iter().all(|present| present));

        let mut referenced = vec![false; lookup.span_count() as usize];
        for byte in 0..=u8::MAX {
            let route = lookup.route(byte);
            if route == jsc_tables::NO_ROUTE || route & jsc_tables::DIRECT_ROUTE != 0 {
                continue;
            }
            let first_span = route & 0xffff;
            let span_count = route >> 16;
            for span_index in first_span..first_span + span_count {
                let span = lookup.span(span_index).unwrap();
                assert!(span.start + span.count <= ranks.count());
                assert!(!referenced[span_index as usize]);
                referenced[span_index as usize] = true;

                let mut sizes = 0u32;
                let mut previous: Option<&[u8]> = None;
                for rank_index in span.start..span.start + span.count {
                    let rank = ranks.get(rank_index).unwrap();
                    let entry = map.get(rank.index).unwrap();
                    let key = &entry.text[..rank.size as usize];
                    if let Some(previous) = previous {
                        assert!(previous >= key);
                    }
                    sizes |= 1 << rank.size;
                    previous = Some(key);
                }
                assert_eq!(span.sizes, sizes);
            }
        }
        assert!(referenced.into_iter().all(|present| present));

        let rosids = ranks.get(61_062).unwrap();
        let rosids_map = map.get(rosids.index).unwrap();
        assert_eq!(
            (rosids.index, rosids.size, rosids_map.size),
            (262_143, 6, 1)
        );
        assert_eq!(lookup_bytes(b"ROSIDS"), Some(262_143));

        let allcall = ranks.get(256_644).unwrap();
        let allcall_map = map.get(allcall.index).unwrap();
        assert_eq!((allcall.index, allcall.size), (81, 7));
        assert_eq!(jsc_tables::BinTable::text_trimmed(allcall_map), b"@ALLCALL");
        assert_eq!(lookup_bytes(b"@ALLCALL"), Some(54));
    }

    #[test]
    fn ranked_lookup_matches_linear_oracle() {
        for byte in 0..=u8::MAX {
            assert_matches_oracle(&[byte, b'X']);
        }

        let ranks = jsc_tables::rank_table();
        let map = jsc_tables::map_table();
        for rank_index in (0..ranks.count()).step_by(257) {
            let rank = ranks.get(rank_index).unwrap();
            let entry = map.get(rank.index).unwrap();
            let key = &entry.text[..rank.size as usize];
            assert_matches_oracle(key);

            let mut extended = key.to_vec();
            extended.extend_from_slice(b"XYZ");
            assert_matches_oracle(&extended);

            if key.len() > 1 {
                assert_matches_oracle(&key[..key.len() - 1]);
            }
        }
    }
}
