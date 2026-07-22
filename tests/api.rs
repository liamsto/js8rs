// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use js8rs::codec::{BuildFramesOptions, build_frames, parse_compound, parse_directed, parse_frame};
use js8rs::protocol::{FrameFlags, FrameType, Submode, SubmodeParseError};

fn data_payload_word_count(text: &str, submode: Submode) -> usize {
    let built = build_frames(&BuildFramesOptions::new(text, submode).with_station("K1ABC", "EM73"));

    let mut payload = String::new();
    for frame in &built.frames {
        let parsed = parse_frame(&frame.encoded, frame.flags, submode);
        if parsed.frame_type == FrameType::FrameData {
            payload.push_str(&parsed.message);
        }
    }

    payload.split_whitespace().count()
}

#[test]
fn aprsis_only_skips_message_checksums() {
    let aprs_words = data_payload_word_count("@APRSIS MSG HELLO", Submode::Normal);
    let aprs_cmd_words = data_payload_word_count("@APRSIS CMD HELLO", Submode::Normal);
    let js8net_words = data_payload_word_count("@JS8NET MSG HELLO", Submode::Normal);

    assert_eq!(aprs_words, 1, "@APRSIS MSG should not append a checksum");
    assert_eq!(
        aprs_cmd_words, 2,
        "@APRSIS CMD should append a buffered checksum"
    );
    assert!(
        js8net_words > 1,
        "non-APRS buffered command should append checksum"
    );
}

#[cfg(not(feature = "legacy_pack_data"))]
#[test]
fn normal_mode_uses_fast_data_policy() {
    let built = build_frames(&BuildFramesOptions::new(
        "THIS IS A LONG MESSAGE THAT SHOULD TRAVEL AS DATA",
        Submode::Normal,
    ));

    assert!(
        built
            .frames
            .iter()
            .any(|f| { f.flags.contains(FrameFlags::DATA) }),
        "normal mode should still emit fast-data flagged frames"
    );
}

#[cfg(feature = "legacy_pack_data")]
#[test]
fn normal_mode_uses_legacy_data_policy() {
    let built = build_frames(&BuildFramesOptions::new(
        "THIS IS A LONG MESSAGE THAT SHOULD TRAVEL AS DATA",
        Submode::Normal,
    ));

    assert!(
        built
            .frames
            .iter()
            .any(|f| { !f.flags.contains(FrameFlags::DATA) }),
        "legacy normal mode should be able to emit unflagged data frames"
    );
}

#[test]
fn invalid_submode_conversion_is_non_panicking() {
    assert_eq!(Submode::try_from(3), Err(SubmodeParseError { value: 3 }));
}

#[test]
fn fixed_frames_parse_through_public_helpers() {
    let directed = parse_directed("Vq46C-uOHma0").expect("directed frame should parse");
    assert_eq!(directed.frame_type, FrameType::FrameDirected);
    assert_eq!(directed.from, "<....>");
    assert_eq!(directed.to, "K1ABC");
    assert_eq!(directed.command, " MSG");
    assert_eq!(directed.number, None);

    let compound = parse_compound("AURtg4DOOkfO").expect("compound frame should parse");
    assert_eq!(compound.frame_type, FrameType::FrameCompound);
    assert_eq!(compound.callsign, "K1ABC");
    assert_eq!(compound.extra.as_deref(), Some(" EM73"));
}
