// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use js8rs::protocol::{DecodeModes, Submode, decode_nmax_frames};
use js8rs::rx::{DecodeConfig, Decoder, Event, SAMPLE_BUFFER_SIZE};

fn parse_fixture_name(path: &Path) -> Option<(Submode, usize, usize)> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(".wav") {
        return None;
    }

    let stem = &name[..name.len() - 4];
    let mut parts = stem.split('_');
    let mode = parts.next()?;
    let depth = parts.next()?.parse::<usize>().ok()?;
    let expected = parts.next()?.parse::<usize>().ok()?;

    let submode = match mode {
        "A" => Submode::Normal,
        "B" => Submode::Fast,
        "C" => Submode::Turbo,
        "E" => Submode::Slow,
        "I" => Submode::Ultra,
        _ => return None,
    };

    Some((submode, depth, expected))
}

fn decode_mode_for(submode: Submode) -> DecodeModes {
    submode.into()
}

fn decode_count_for(
    decoder: &mut Decoder,
    samples: &[i16],
    submode: Submode,
    depth: usize,
) -> usize {
    let nmax = decode_nmax_frames(submode).max(1usize);
    let kin = samples.len().min(SAMPLE_BUFFER_SIZE).min(nmax);
    let mut ring = vec![0i16; SAMPLE_BUFFER_SIZE];
    ring[..kin].copy_from_slice(&samples[..kin]);

    let config = DecodeConfig::default()
        .with_modes(decode_mode_for(submode))
        .with_nominal_frequency(1500)
        .with_frequency_range(0, 5_000);

    let mut unique = HashSet::new();
    for _ in 0..depth.max(1) {
        let _ = decoder.decode(&ring, kin, &config, |event| {
            if let Event::Decoded(done) = event {
                unique.insert((
                    done.frame.submode,
                    done.frequency_hz.to_bits(),
                    done.frame.encoded,
                ));
            }
        });
    }
    unique.len()
}

fn read_wav_pcm16_mono(path: &Path) -> anyhow::Result<Vec<i16>> {
    let bytes = fs::read(path)?;
    anyhow::ensure!(bytes.len() >= 44, "wav file too small: {}", path.display());
    anyhow::ensure!(&bytes[0..4] == b"RIFF", "missing RIFF header");
    anyhow::ensure!(&bytes[8..12] == b"WAVE", "missing WAVE header");

    let mut pos = 12usize;
    let mut channels = None;
    let mut bits_per_sample = None;
    let mut data = None;

    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let sz = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        pos += 8;
        if pos + sz > bytes.len() {
            break;
        }

        match id {
            b"fmt " => {
                anyhow::ensure!(sz >= 16, "invalid fmt chunk");
                channels = Some(u16::from_le_bytes([bytes[pos + 2], bytes[pos + 3]]));
                bits_per_sample = Some(u16::from_le_bytes([bytes[pos + 14], bytes[pos + 15]]));
            }
            b"data" => {
                data = Some((pos, sz));
            }
            _ => {}
        }
        pos += sz + (sz & 1);
    }

    anyhow::ensure!(channels == Some(1), "expected mono wav");
    anyhow::ensure!(bits_per_sample == Some(16), "expected 16-bit wav");
    let (start, len) = data.ok_or_else(|| anyhow::anyhow!("missing data chunk"))?;
    anyhow::ensure!(len % 2 == 0, "data chunk must be aligned to i16");

    let mut out = Vec::with_capacity(len / 2);
    let end = start + len;
    let mut i = start;
    while i + 1 < end {
        out.push(i16::from_le_bytes([bytes[i], bytes[i + 1]]));
        i += 2;
    }
    Ok(out)
}

#[test]
fn fixture_decode_counts_match_expected() -> anyhow::Result<()> {
    let root = PathBuf::from("tests/audio");

    let mut fixtures = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if let Some((submode, depth, expected)) = parse_fixture_name(&path) {
            fixtures.push((path, submode, depth, expected));
        }
    }
    fixtures.sort_by(|a, b| a.0.cmp(&b.0));
    anyhow::ensure!(!fixtures.is_empty(), "no wav fixtures found");

    let mut mismatches = Vec::new();
    for (path, submode, depth, expected) in fixtures {
        let samples = read_wav_pcm16_mono(&path)?;
        let mut decoder = Decoder::new();
        let direct_count = decode_count_for(&mut decoder, &samples, submode, depth);
        let expected_min = if submode == Submode::Normal && depth > 1 {
            expected.saturating_sub(1)
        } else {
            expected
        };
        if !(expected_min..=expected).contains(&direct_count) {
            mismatches.push(format!(
                "{} depth={} got {} expected {}..={}",
                path.display(),
                depth,
                direct_count,
                expected_min,
                expected,
            ));
        }
    }

    anyhow::ensure!(
        mismatches.is_empty(),
        "fixture mismatches:\n{}",
        mismatches.join("\n")
    );
    Ok(())
}
