use anyhow::Result;
use js8rs::codec::{BuildFramesOptions, EncodeError, build_frames, encode_tones, parse_frame};
use js8rs::protocol::{Submode, decode_nmax_frames};
use js8rs::rx::{
    DecodeCursor, MessageBufferAssembler, ReassemblyEvent, next_decode_window, window_from_kin,
};

fn frame_to_12_bytes(frame: &str) -> [u8; 12] {
    let bytes = frame.as_bytes();
    let mut out = [b' '; 12];
    out[..bytes.len()].copy_from_slice(bytes);
    out
}

#[test]
fn build_result_encode_matches_per_frame_encoding() -> Result<()> {
    let built = build_frames(&BuildFramesOptions::new(
        "HELLO WORLD HELLO WORLD",
        Submode::Fast,
    ));

    let expected: Vec<_> = built
        .frames
        .iter()
        .map(|frame| {
            Ok((
                frame.clone(),
                encode_tones(
                    frame.flags,
                    Submode::Fast,
                    &frame_to_12_bytes(&frame.encoded),
                )?,
            ))
        })
        .collect::<std::result::Result<_, EncodeError>>()?;

    let actual = built.encode()?;
    assert_eq!(actual.len(), expected.len());
    for (encoded_frame, (frame, tones)) in actual.iter().zip(expected.iter()) {
        assert_eq!(&encoded_frame.frame, frame);
        assert_eq!(&encoded_frame.tones, tones);
    }

    Ok(())
}

#[test]
fn decode_window_clamps_each_selected_mode_span() {
    let kin = 200_000;
    let expected_a = window_from_kin(kin, decode_nmax_frames(Submode::Normal));
    let expected_b = window_from_kin(kin, decode_nmax_frames(Submode::Fast));

    assert_eq!(expected_a, (20_000, 180_000));
    assert_eq!(expected_b, (80_000, 120_000));
}

#[test]
fn decode_scheduler_emits_windows_when_ready() {
    let mut cursor = DecodeCursor::new();

    assert!(next_decode_window(Submode::Normal, 100_000, 90_000, &mut cursor).is_none());

    let first = next_decode_window(Submode::Normal, 170_000, 100_000, &mut cursor)
        .expect("normal window should become ready");
    assert_eq!(first.start, 0);
    assert!(first.size >= 163_680);

    let second = next_decode_window(Submode::Normal, 350_000, 170_000, &mut cursor)
        .expect("second window should become ready");
    assert_eq!(second.start, 180_000);
}

#[test]
fn buffered_command_reassembler_completes_and_strips_checksum() {
    let built = build_frames(
        &BuildFramesOptions::new("MSG HELLO THIS IS A LONGER MESSAGE", Submode::Normal)
            .with_station("K1ABC", "EM73")
            .with_selected_call("K2XYZ"),
    );
    let mut assembler = MessageBufferAssembler::new();
    let mut completed = None;

    for frame in built.frames {
        let parsed = parse_frame(&frame.encoded, frame.flags, Submode::Normal);
        let decoded = js8rs::rx::Decoded::new(parsed, 0, 0, 0.0, 1500.0, 1.0);
        if let Some(ReassemblyEvent::Completed(done)) = assembler.push_decoded(&decoded) {
            completed = Some(done);
        }
    }

    let done = completed.expect("expected buffered command completion event");
    assert_eq!(done.command.trim(), "MSG");
    assert_eq!(done.payload.trim(), "HELLO THIS IS A LONGER MESSAGE");
    assert!(done.checksum.as_ref().is_some_and(|c| c.valid));
}
