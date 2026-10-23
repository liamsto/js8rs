// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use js8rs::protocol::{DecodeModes, Submode};
use js8rs::rx::{DecodeConfig, Decoder, Event, SAMPLE_BUFFER_SIZE};

fn parse_fixture_name(path: &Path) -> Option<(Submode, usize)> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(".wav") {
        return None;
    }

    let stem = &name[..name.len() - 4];
    let mut parts = stem.split('_');
    let mode = parts.next()?;
    let depth = parts.next()?.parse::<usize>().ok()?;

    let submode = match mode {
        "A" => Submode::Normal,
        "B" => Submode::Fast,
        "C" => Submode::Turbo,
        "E" => Submode::Slow,
        "I" => Submode::Ultra,
        _ => return None,
    };

    Some((submode, depth))
}

fn decode_mode_for(submode: Submode) -> DecodeModes {
    submode.into()
}

fn dec_frames(
    decoder: &mut Decoder,
    samples: &[i16],
    submode: Submode,
    depth: usize,
) -> BTreeSet<(u8, String)> {
    let nmax = submode.samples_per_period().max(1usize);
    let kin = samples.len().min(SAMPLE_BUFFER_SIZE).min(nmax);
    let mut ring = vec![0i16; SAMPLE_BUFFER_SIZE];
    ring[..kin].copy_from_slice(&samples[..kin]);

    let config = DecodeConfig::default()
        .with_modes(decode_mode_for(submode))
        .with_nominal_frequency(1500)
        .with_frequency_range(0, 5_000);

    let mut unique = BTreeSet::new();
    for _ in 0..depth.max(1) {
        let _ = decoder.decode(&ring, kin, &config, |event| {
            if let Event::Decoded(done) = event {
                unique.insert((done.frame.flags.bits(), done.frame.encoded));
            }
        });
    }
    unique
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
fn wav_frames() -> anyhow::Result<()> {
    let root = PathBuf::from("tests/audio");

    let mut fixtures = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if let Some((submode, depth)) = parse_fixture_name(&path) {
            fixtures.push((path, submode, depth));
        }
    }
    fixtures.sort_by(|a, b| a.0.cmp(&b.0));
    anyhow::ensure!(!fixtures.is_empty(), "no wav fixtures found");

    for (path, submode, depth) in fixtures {
        let want: &[(u8, &str)] = match path.file_name().and_then(|n| n.to_str()) {
            Some("A_1_4.wav" | "A_2_5.wav") => &[
                (3, "2Y-wUW3FOjFp"),
                (3, "SJWkJnSNwzqH"),
                (3, "Vk4xfHSNwzaX"),
                (3, "SKflsHSNwzqH"),
                (3, "UctD9HSNwzqE"),
            ],
            Some("A_2_1.wav") => &[],
            Some("A_2_3.wav" | "A_3_3.wav") => &[
                (3, "VlPJy-uGBfqF"),
                (3, "Uw2nt-xLZLqO"),
                (1, "SJDRf-vSO3y0"),
            ],
            Some("A_2_6.wav") => &[
                (0, "u3ipItc4eML+"),
                (3, "SQu8I8l+CDqR"),
                (3, "SJff78l+CDqP"),
                (3, "SIUT18l+CDqE"),
                (3, "VkB4eel+CDqS"),
                (3, "VlKXp8l+CDqI"),
            ],
            Some("A_2_9.wav") => &[
                (3, "SQu8IUuGBfqL"),
                (3, "VlPJy-uGBfqI"),
                (3, "SJff7UuGBfqY"),
                (3, "Uw2nt-uGBfqS"),
                (3, "SKgyQ-uGBfqU"),
                (3, "VkNoM-uGBfqK"),
                (3, "SIUT1UuGBfqb"),
                (3, "VkDSPUuGBfqa"),
            ],
            Some("E_1_1.wav" | "E_2_1.wav") => &[(7, "TrMcT8++++++")],
            _ => anyhow::bail!("missing expected frames for {}", path.display()),
        };
        let samples = read_wav_pcm16_mono(&path)?;
        let mut decoder = Decoder::new();
        let got = dec_frames(&mut decoder, &samples, submode, depth);
        let want = want
            .iter()
            .map(|&(flags, txt)| (flags, txt.to_owned()))
            .collect();
        assert_eq!(got, want, "{}", path.display());
    }
    Ok(())
}
