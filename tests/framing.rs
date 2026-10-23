// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use js8rs::codec::{BuildFramesOptions, build_frames, parse_frame};
use js8rs::protocol::{FrameFlags, FrameType, Submode};

#[test]
fn frames_always_mark_first_and_last() {
    let built = build_frames(
        &BuildFramesOptions::new("HELLO WORLD", Submode::Fast).with_station("K1ABC", "EM73"),
    );

    assert!(!built.frames.is_empty());
    assert!(
        built
            .frames
            .first()
            .unwrap()
            .flags
            .contains(FrameFlags::FIRST)
    );
    assert!(
        built
            .frames
            .last()
            .unwrap()
            .flags
            .contains(FrameFlags::LAST)
    );
}

#[test]
fn lowercase_text_is_normalized_before_framing() {
    let lowercase = build_frames(&BuildFramesOptions::new("Hello, world!", Submode::Normal));
    let uppercase = build_frames(&BuildFramesOptions::new("HELLO, WORLD!", Submode::Normal));

    assert_eq!(lowercase, uppercase);
    assert!(!lowercase.frames.is_empty());
    assert!(lowercase.encode().is_ok());
}

#[test]
fn force_data_produces_data_flagged_frames() {
    let built = build_frames(
        &BuildFramesOptions::new(
            "THIS IS AN INTENTIONALLY LONG MESSAGE TO FORCE DATA MODE",
            Submode::Normal,
        )
        .with_station("K1ABC", "EM73")
        .with_data(true),
    );

    #[cfg(not(feature = "legacy_pack_data"))]
    assert!(
        built
            .frames
            .iter()
            .any(|f| f.flags.contains(FrameFlags::DATA))
    );
    #[cfg(feature = "legacy_pack_data")]
    assert!(
        built
            .frames
            .iter()
            .any(|f| !f.flags.contains(FrameFlags::DATA))
    );
}

#[test]
fn force_identify_injects_callsign_for_likely_data_frame() {
    let mycall = "K1ABC";
    let built = build_frames(
        &BuildFramesOptions::new(
            "THIS IS A LONG FREE TEXT BLOB EXPECTED TO GO OUT AS DATA",
            Submode::Fast,
        )
        .with_station(mycall, "EM73")
        .with_identify(true),
    );

    let mut payload = String::new();
    for frame in &built.frames {
        let parsed = parse_frame(&frame.encoded, frame.flags, Submode::Fast);
        if parsed.frame_type == FrameType::FrameData {
            payload.push_str(&parsed.message);
        }
    }

    assert!(payload.contains("K1ABC:"));
}

#[test]
fn compound_from_or_to_messages_are_buildable() {
    let from_compound = build_frames(
        &BuildFramesOptions::new("K2XYZ MSG HELLO", Submode::Fast).with_station("K1ABC/P", "EM73"),
    );

    assert!(!from_compound.frames.is_empty());
    assert!(
        from_compound
            .frames
            .first()
            .unwrap()
            .flags
            .contains(FrameFlags::FIRST)
    );
    assert!(
        from_compound
            .frames
            .last()
            .unwrap()
            .flags
            .contains(FrameFlags::LAST)
    );

    let to_compound = build_frames(
        &BuildFramesOptions::new("K2XYZ/P MSG HELLO", Submode::Fast).with_station("K1ABC", "EM73"),
    );

    assert!(!to_compound.frames.is_empty());
    assert!(
        to_compound
            .frames
            .first()
            .unwrap()
            .flags
            .contains(FrameFlags::FIRST)
    );
    assert!(
        to_compound
            .frames
            .last()
            .unwrap()
            .flags
            .contains(FrameFlags::LAST)
    );
}

#[test]
fn selected_call_is_not_prepended_to_base_call_lines() {
    let built = build_frames(
        &BuildFramesOptions::new("CQ CQ CQ EM73", Submode::Normal)
            .with_station("K1ABC", "EM73")
            .with_selected_call("K2XYZ"),
    );

    let first = parse_frame(
        &built.frames[0].encoded,
        built.frames[0].flags,
        Submode::Normal,
    );

    assert!(matches!(
        first.frame_type,
        FrameType::FrameHeartbeat | FrameType::FrameCompound
    ));
    assert!(!first.message.contains("K2XYZ"));
}

#[test]
fn jsc_cmds() {
    // from varicode
    let cases = &[
        ("K1ABC", "@ALLCALL SNR?", &[("Vk64SVGGOq00", 3)][..]),
        (
            "K1ABC",
            "@APRSIS MSG HELLO WORLD",
            &[("Vk64SVGGPoa0", 1), ("UBXRtA7+++++", 6)],
        ),
        (
            "K1ABC",
            "K2XYZ> K3ABC HELLO WORLD",
            &[
                ("Vk64SUugOyK0", 1),
                ("abiUETRnSBV+", 4),
                ("UvJQg9R7++++", 6),
            ],
        ),
        (
            "K1ABC",
            "K2XYZ > K3ABC HELLO WORLD",
            &[
                ("Vk64SUugOyK0", 1),
                ("abiUETRnSBV+", 4),
                ("UvJQg9R7++++", 6),
            ],
        ),
        (
            "K1ABC",
            "K2XYZ QUERY MSG 123",
            &[("Vk64SUugOyi0", 1), ("bqs2pMsc2C++", 6)],
        ),
        (
            "K1ABC",
            "K2XYZ QUERY MSG HELLO",
            &[("Vk64SUugOyi0", 1), ("bql5mkHGZ1F+", 6)],
        ),
        (
            "K1ABC",
            "K2XYZ CQ CQ CQ",
            &[("Vk64SUugOzy0", 1), ("VYlnNuZ+++++", 6)],
        ),
        (
            "K1ABC",
            "K2XYZ HB EM73",
            &[("Vk64SUugOzy0", 1), ("S4BYhe3+++++", 6)],
        ),
        (
            "K1ABC",
            "K2XYZ HEARTBEAT",
            &[("Vk64SUugOzy0", 1), ("+CSJ++++++++", 6)],
        ),
        (
            "K1ABC",
            "K2XYZ SNR -32",
            &[("Vk64SUugOzaS", 1), ("ip++++++++++", 6)],
        ),
        (
            "K1ABC",
            "F/K2XYZ CQ",
            &[
                ("AURtg4DOOkfO", 1),
                ("H-S6H8F+G+N8", 0),
                ("VYF+++++++++", 6),
            ],
        ),
        (
            "F/K1ABC",
            "K2XYZ> K3ABC HELLO WORLD",
            &[
                ("9-S5oVx5ukfO", 1),
                ("IUnN3JQHO+Ju", 0),
                ("abiUETRnSBV+", 4),
                ("UvJQg9R7++++", 6),
            ],
        ),
    ];
    for &(call, msg, want) in cases {
        for mode in [Submode::Fast, Submode::Turbo, Submode::Slow, Submode::Ultra] {
            let frs = build_frames(&BuildFramesOptions::new(msg, mode).with_station(call, "EM73"));
            let got: Vec<_> = frs
                .frames
                .iter()
                .map(|f| (f.encoded.as_str(), f.flags.bits()))
                .collect();
            assert_eq!(got, want, "{call}: {msg} ({mode:?})");
        }
    }
}
