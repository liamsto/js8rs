// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use anyhow::{Result, bail};
use js8rs::codec::{BuildFramesOptions, build_frames, encode_tones};
use js8rs::protocol::{DecodeModes, Submode};
use js8rs::rx::{DecodeConfig, Decoder, Detector, Event, InputFormat};
use js8rs::tx::{Channel, Modulator};
use std::sync::OnceLock;
use std::time::Duration;
use tracing::info;

static TRACING: OnceLock<()> = OnceLock::new();

fn init_tracing() {
    TRACING.get_or_init(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("info".parse().unwrap()),
            )
            .with_test_writer()
            .try_init();
    });
}

#[test]
fn roundtrip_normal() -> Result<()> {
    run_roundtrip(Submode::Normal, DecodeModes::NORMAL)
}

#[test]
fn roundtrip_fast() -> Result<()> {
    run_roundtrip(Submode::Fast, DecodeModes::FAST)
}

#[test]
fn roundtrip_turbo() -> Result<()> {
    run_roundtrip(Submode::Turbo, DecodeModes::TURBO)
}

#[test]
fn roundtrip_slow() -> Result<()> {
    run_roundtrip(Submode::Slow, DecodeModes::SLOW)
}

#[test]
fn roundtrip_ultra() -> Result<()> {
    run_roundtrip(Submode::Ultra, DecodeModes::ULTRA)
}

fn run_roundtrip(submode: Submode, decode_mode: DecodeModes) -> Result<()> {
    init_tracing();

    let original_message = "HELLO WORLD".to_string();

    let built = build_frames(&BuildFramesOptions::new(&original_message, submode));
    if built.frames.is_empty() {
        bail!("build_frames returned no frames");
    }

    for frame in &built.frames {
        info!("Packed message: {}", frame.encoded)
    }

    let mut decoder = Decoder::with_modes(decode_mode);
    let mut decoded_fragments: Vec<String> = Vec::new();

    for (frame_idx, frame) in built.frames.iter().enumerate() {
        let frame12 = frame_to_12_bytes(&frame.encoded)?;
        let itones = encode_tones(frame.flags, submode, &frame12)?;
        let detector = Detector::new(1, 1024);

        modulate_into_detector(&itones, submode, 0, &detector)?;

        let decoded = detector.with_samples(|samples, kin_end| {
            let config = decode_config(decode_mode);
            decode_once(
                &mut decoder,
                samples,
                kin_end,
                &config,
                submode,
                &frame.encoded,
                frame_idx,
            )
        })?;

        decoded_fragments.push(decoded.frame.message.clone());
        info!("{}", decoded.log_line())
    }

    let reconstructed = decoded_fragments.concat();
    let reconstructed_norm = reconstructed.trim_end_matches(' ').to_string();
    let original_norm = original_message.trim_end_matches(' ').to_string();

    if reconstructed_norm != original_norm {
        bail!(
            "unpacked message mismatch:\n  expected: {original_message:?}\n  got:      {reconstructed:?}"
        );
    }

    info!("Unpacked message: {}", reconstructed_norm);

    Ok(())
}

fn modulate_into_detector(
    itones: &[u8; 79],
    submode: Submode,
    start_utc_ms: u64,
    detector: &Detector,
) -> Result<()> {
    let mut modulator = Modulator::new();
    modulator.start_tones(
        itones,
        submode,
        start_utc_ms,
        1500.0,
        Duration::ZERO,
        Channel::Mono,
    );

    let mut pcm_stereo = vec![0i16; 1024 * 2];
    let mut pcm_mono = vec![0i16; 1024];

    while !modulator.is_idle() {
        let frames = modulator.render_stereo(&mut pcm_stereo);
        if frames == 0 {
            break;
        }

        for i in 0..frames {
            pcm_mono[i] = pcm_stereo[2 * i];
        }

        detector.write_i16(&pcm_mono[..frames], InputFormat::Mono);
    }

    Ok(())
}

fn decode_config(modes: DecodeModes) -> DecodeConfig {
    DecodeConfig::default()
        .with_modes(modes)
        .with_nominal_frequency(1500)
        .with_frequency_range(0, 5_000)
}

fn frame_to_12_bytes(frame: &str) -> Result<[u8; 12]> {
    let bytes = frame.as_bytes();
    if bytes.len() > 12 {
        bail!("frame longer than 12 bytes: {frame:?}");
    }
    let mut out = [b' '; 12];
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

fn decode_once(
    decoder: &mut Decoder,
    samples: &[i16],
    valid_samples: usize,
    config: &DecodeConfig,
    expected_mode: Submode,
    expected_data: &str,
    frame_idx: usize,
) -> anyhow::Result<js8rs::rx::Decoded> {
    let mut matched = None;
    let mut saw_finished = false;

    let _ = decoder.decode(samples, valid_samples, config, |event| match event {
        Event::Decoded(decoded) => {
            if decoded.frame.submode == expected_mode && decoded.frame.encoded == expected_data {
                matched = Some(decoded);
            }
        }
        Event::DecodeFinished(_) => {
            saw_finished = true;
        }
        _ => {}
    });

    if let Some(decoded) = matched {
        return Ok(decoded);
    }
    if saw_finished {
        bail!(
            "frame {} decode finished without matching packed data {:?} (mode {})",
            frame_idx,
            expected_data,
            expected_mode as u8
        );
    }

    bail!("frame {frame_idx} decode produced no DecodeFinished event for {expected_data:?}")
}
