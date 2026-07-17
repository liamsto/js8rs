#![allow(dead_code)]

use anyhow::{Result, bail};
use js8rs::codec::{BuildFramesOptions, build_frames, encode_tones};
use js8rs::protocol::{DecodeModes, FrameFlags, Submode};
use js8rs::rx::{DecodeConfig, Decoder, Detector, Event, InputFormat, SAMPLE_BUFFER_SIZE};
use js8rs::tx::{Channel, Modulator};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct DecodeFixture {
    pub(crate) d2: Vec<i16>,
    pub(crate) config: DecodeConfig,
    pub(crate) valid_samples: usize,
    pub(crate) encoded: String,
}

pub(crate) struct EncodeFixture {
    pub(crate) submode: Submode,
    pub(crate) flags: FrameFlags,
    pub(crate) encoded: String,
    pub(crate) frame12: [u8; 12],
}

fn decode_mode(submode: Submode) -> DecodeModes {
    submode.into()
}

pub(crate) fn bench_config(submode: Submode) -> DecodeConfig {
    DecodeConfig::default()
        .with_modes(decode_mode(submode))
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

pub(crate) fn modulate_into_detector_with_buffers(
    itones: &[u8; 79],
    submode: Submode,
    detector: &Detector,
    pcm_stereo: &mut [i16],
    pcm_mono: &mut [i16],
) {
    assert_eq!(pcm_stereo.len(), pcm_mono.len() * 2);

    let mut modulator = Modulator::new();
    modulator.start_tones(itones, submode, 0, 1500.0, Duration::ZERO, Channel::Mono);

    while !modulator.is_idle() {
        let frames = modulator.render_stereo(pcm_stereo);
        if frames == 0 {
            break;
        }

        for i in 0..frames {
            pcm_mono[i] = pcm_stereo[2 * i];
        }
        detector.write_i16(&pcm_mono[..frames], InputFormat::Mono);
    }
}

pub(crate) fn modulate_into_detector(itones: &[u8; 79], submode: Submode, detector: &Detector) {
    let mut pcm_stereo = vec![0i16; 1024 * 2];
    let mut pcm_mono = vec![0i16; 1024];
    modulate_into_detector_with_buffers(itones, submode, detector, &mut pcm_stereo, &mut pcm_mono);
}

pub(crate) fn synth_encode_fixture(submode: Submode, message: &str) -> EncodeFixture {
    let built = build_frames(&BuildFramesOptions::new(message, submode));
    let frame = built
        .frames
        .first()
        .expect("build_frames should produce at least one frame");

    let frame12 = frame_to_12_bytes(&frame.encoded).expect("frame should fit 12 bytes");
    EncodeFixture {
        submode,
        flags: frame.flags,
        encoded: frame.encoded.clone(),
        frame12,
    }
}

pub(crate) fn synth_decode_fixture(submode: Submode, message: &str) -> DecodeFixture {
    let enc = synth_encode_fixture(submode, message);
    let itones = encode_tones(enc.flags, submode, &enc.frame12).expect("encode_tones should work");

    let detector = Detector::new(1, 1024);
    modulate_into_detector(&itones, submode, &detector);
    let mut d2 = vec![0; SAMPLE_BUFFER_SIZE];
    let kin_end = detector.copy_samples(&mut d2);

    DecodeFixture {
        d2,
        config: bench_config(submode),
        valid_samples: kin_end,
        encoded: enc.encoded,
    }
}

pub(crate) fn validate_decode_fixture(fixture: &DecodeFixture) {
    let mut decoder = Decoder::new();
    let mut matched = false;
    let decoded = decoder.decode(
        &fixture.d2,
        fixture.valid_samples,
        &fixture.config,
        |event| {
            if let Event::Decoded(frame) = event {
                matched |= frame.frame.encoded == fixture.encoded;
            }
        },
    );

    assert!(decoded > 0, "benchmark fixture did not decode");
    assert!(matched, "benchmark fixture decoded an unexpected frame");
}
