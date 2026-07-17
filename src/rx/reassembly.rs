// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Extracted buffered-message reassembly into sync Rust state.

//! Reassembly of buffered directed commands across decoded frames.

use crate::codec::DecodedFrame;
use crate::protocol::{FrameFlags, Submode};
use crate::rx::Decoded;
use crate::varicode::{
    checksum16_valid, checksum32_valid, is_command_buffered, is_command_checksumed,
};
use std::collections::HashMap;

/// Key separating independent buffered message streams.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferKey {
    /// Receive submode.
    pub submode: Submode,
    /// Truncated audio frequency in hertz.
    pub frequency_offset: u32,
}

/// Checksum removed from a completed buffered payload.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferedChecksum {
    /// Checksum width in bits.
    pub size_bits: u8,
    /// Checksum text received over the air.
    pub value: String,
    /// Whether the checksum matched the payload.
    pub valid: bool,
}

/// Completed directed command after optional multi-frame reassembly.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferedCommandResult {
    /// Stream key used during reassembly.
    pub key: BufferKey,
    /// Source callsign.
    pub from: String,
    /// Destination callsign.
    pub to: String,
    /// Directed command token.
    pub command: String,
    /// Reassembled payload without checksum text.
    pub payload: String,
    /// Parsed checksum when the command requires one.
    pub checksum: Option<BufferedChecksum>,
    /// Packed UTC receive time.
    pub utc: u32,
    /// Receive signal-to-noise ratio in decibels.
    pub snr: i32,
    /// Combined frame flags.
    pub flags: FrameFlags,
    /// Whether multiple frames contributed to the result.
    pub was_buffered: bool,
}

/// State transition produced while reassembling a decoded frame.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReassemblyEvent {
    /// A new buffered stream was created.
    BufferedStarted(BufferKey),
    /// A payload fragment was appended.
    BufferedAppended(BufferKey),
    /// A command completed successfully.
    Completed(BufferedCommandResult),
    /// A completed command failed checksum validation.
    DroppedInvalidChecksum(BufferKey),
}

/// Stateful assembler for buffered directed commands.
#[derive(Debug, Clone, Default)]
pub struct MessageBufferAssembler {
    buffers: HashMap<BufferKey, BufferState>,
}

#[derive(Debug, Clone)]
struct BufferState {
    from: String,
    to: String,
    command: String,
    utc: u32,
    snr: i32,
    flags: FrameFlags,
    fragments: Vec<String>,
}

impl MessageBufferAssembler {
    #[must_use]
    /// Creates an empty assembler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Discards every partially assembled message.
    pub fn clear(&mut self) {
        self.buffers.clear();
    }

    /// Consumes one decoded frame and returns any reassembly transition.
    pub fn push_decoded(&mut self, decoded: &Decoded) -> Option<ReassemblyEvent> {
        let parsed = &decoded.frame;
        let key = BufferKey {
            submode: parsed.submode,
            frequency_offset: decoded.frequency_hz.max(0.0) as u32,
        };
        let is_last = parsed.flags.contains(FrameFlags::LAST);

        if parsed.is_directed() {
            return self.handle_directed(key, decoded, parsed, is_last);
        }

        if parsed.is_compound() {
            return None;
        }

        if let Some(state) = self.buffers.get_mut(&key) {
            state.fragments.push(parsed.message.clone());
            if !is_last {
                return Some(ReassemblyEvent::BufferedAppended(key));
            }

            let state = self.buffers.remove(&key).expect("buffer key exists");
            return Some(self.finish_state(key, state));
        }

        None
    }

    fn handle_directed(
        &mut self,
        key: BufferKey,
        decoded: &Decoded,
        parsed: &DecodedFrame,
        is_last: bool,
    ) -> Option<ReassemblyEvent> {
        if parsed.directed.len() < 3 {
            return None;
        }

        let from = parsed.directed[0].clone();
        let to = parsed.directed[1].clone();
        let command = parsed.directed[2].clone();

        let has_placeholder_call = from == "<....>" || to == "<....>";
        let buffered = is_command_buffered(&command);

        if buffered && (!is_last || has_placeholder_call) {
            self.buffers.insert(
                key,
                BufferState {
                    from,
                    to,
                    command,
                    utc: decoded.utc,
                    snr: decoded.snr,
                    flags: decoded.frame.flags,
                    fragments: Vec::new(),
                },
            );
            return Some(ReassemblyEvent::BufferedStarted(key));
        }

        let payload = if parsed.directed.len() > 3 {
            parsed.directed[3..].join(" ")
        } else {
            String::new()
        };

        let (payload, checksum) = finalize_buffered_payload(&command, payload);
        let result = BufferedCommandResult {
            key,
            from,
            to,
            command,
            payload,
            checksum: checksum.clone(),
            utc: decoded.utc,
            snr: decoded.snr,
            flags: decoded.frame.flags,
            was_buffered: false,
        };

        if let Some(c) = checksum
            && !c.valid
        {
            return Some(ReassemblyEvent::DroppedInvalidChecksum(key));
        }

        Some(ReassemblyEvent::Completed(result))
    }

    fn finish_state(&self, key: BufferKey, state: BufferState) -> ReassemblyEvent {
        let payload = rstrip_spaces(&state.fragments.concat());
        let (payload, checksum) = finalize_buffered_payload(&state.command, payload);

        if let Some(c) = &checksum
            && !c.valid
        {
            return ReassemblyEvent::DroppedInvalidChecksum(key);
        }

        ReassemblyEvent::Completed(BufferedCommandResult {
            key,
            from: state.from,
            to: state.to,
            command: state.command,
            payload,
            checksum,
            utc: state.utc,
            snr: state.snr,
            flags: state.flags | FrameFlags::LAST,
            was_buffered: true,
        })
    }
}

fn lstrip_spaces(input: &str) -> &str {
    input.trim_start_matches(' ')
}

fn rstrip_spaces(input: &str) -> String {
    input.trim_end_matches(' ').to_owned()
}

fn finalize_buffered_payload(command: &str, payload: String) -> (String, Option<BufferedChecksum>) {
    if !is_command_buffered(command) {
        return (payload, None);
    }

    let checksum_size = is_command_checksumed(command);
    if checksum_size == 0 {
        return (payload, None);
    }

    let mut message = lstrip_spaces(&payload).to_owned();

    if checksum_size == 16u8 {
        if message.len() < 4 {
            return (
                message,
                Some(BufferedChecksum {
                    size_bits: 16u8,
                    value: String::new(),
                    valid: false,
                }),
            );
        }

        let checksum = message[message.len() - 3..].to_owned();
        if message.as_bytes()[message.len() - 4] != b' ' {
            return (
                message,
                Some(BufferedChecksum {
                    size_bits: 16u8,
                    value: checksum,
                    valid: false,
                }),
            );
        }

        message.truncate(message.len() - 4);
        let valid = checksum16_valid(&checksum, &message);
        return (
            message,
            Some(BufferedChecksum {
                size_bits: 16u8,
                value: checksum,
                valid,
            }),
        );
    }

    if checksum_size == 32u8 {
        if message.len() < 7 {
            return (
                message,
                Some(BufferedChecksum {
                    size_bits: 32u8,
                    value: String::new(),
                    valid: false,
                }),
            );
        }

        let checksum = message[message.len() - 6..].to_owned();
        if message.as_bytes()[message.len() - 7] != b' ' {
            return (
                message,
                Some(BufferedChecksum {
                    size_bits: 32u8,
                    value: checksum,
                    valid: false,
                }),
            );
        }

        message.truncate(message.len() - 7);
        let valid = checksum32_valid(&checksum, &message);
        return (
            message,
            Some(BufferedChecksum {
                size_bits: 32u8,
                value: checksum,
                valid,
            }),
        );
    }

    (message, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{BuildFramesOptions, build_frames, parse_frame};
    use crate::protocol::{FrameFlags, Submode};
    use crate::varicode::{pack_compound_message, pack_directed_message};

    fn decoded_from(frame: &str, flags: FrameFlags, submode: Submode, frequency: f32) -> Decoded {
        Decoded::new(
            parse_frame(frame, flags, submode),
            123_000,
            -10,
            0.0,
            frequency,
            1.0,
        )
    }

    #[test]
    fn buffered_reassembly_emits_started_appended_completed() {
        let built = build_frames(
            &BuildFramesOptions::new("MSG HELLO THIS IS A LONG BUFFERED MESSAGE", Submode::Normal)
                .with_station("K1ABC", "EM73")
                .with_selected_call("K2XYZ"),
        );

        let mut assembler = MessageBufferAssembler::new();
        let mut saw_started = false;
        let mut saw_appended = false;
        let mut completed = None;

        for frame in built.frames {
            let decoded = decoded_from(&frame.encoded, frame.flags, Submode::Normal, 1500.0);
            if let Some(event) = assembler.push_decoded(&decoded) {
                match event {
                    ReassemblyEvent::BufferedStarted(_) => saw_started = true,
                    ReassemblyEvent::BufferedAppended(_) => saw_appended = true,
                    ReassemblyEvent::Completed(done) => completed = Some(done),
                    ReassemblyEvent::DroppedInvalidChecksum(_) => {}
                }
            }
        }

        assert!(saw_started);
        assert!(saw_appended);
        let done = completed.expect("expected completed reassembly");
        assert_eq!(done.command.trim(), "MSG");
        assert_eq!(done.payload.trim(), "HELLO THIS IS A LONG BUFFERED MESSAGE");
        assert!(done.was_buffered);
        assert!(done.flags.contains(FrameFlags::LAST));
        assert!(done.checksum.as_ref().is_some_and(|c| c.valid));
    }

    #[test]
    fn interleaved_buffers_are_keyed_by_submode_and_frequency() {
        let m1 = build_frames(
            &BuildFramesOptions::new("MSG ALPHA PAYLOAD", Submode::Normal)
                .with_station("K1ABC", "EM73")
                .with_selected_call("K2AAA"),
        );
        let m2 = build_frames(
            &BuildFramesOptions::new("MSG BRAVO PAYLOAD", Submode::Normal)
                .with_station("K1ABC", "EM73")
                .with_selected_call("K2BBB"),
        );

        let mut assembler = MessageBufferAssembler::new();
        let mut done = Vec::new();

        let max_len = m1.frames.len().max(m2.frames.len());
        for idx in 0..max_len {
            if let Some(frame) = m1.frames.get(idx) {
                let decoded = decoded_from(&frame.encoded, frame.flags, Submode::Normal, 1200.0);
                if let Some(ReassemblyEvent::Completed(result)) = assembler.push_decoded(&decoded) {
                    done.push(result);
                }
            }
            if let Some(frame) = m2.frames.get(idx) {
                let decoded = decoded_from(&frame.encoded, frame.flags, Submode::Normal, 2200.0);
                if let Some(ReassemblyEvent::Completed(result)) = assembler.push_decoded(&decoded) {
                    done.push(result);
                }
            }
        }

        assert_eq!(done.len(), 2);
        assert!(
            done.iter()
                .any(|r| r.to == "K2AAA" && r.payload.trim() == "ALPHA PAYLOAD")
        );
        assert!(
            done.iter()
                .any(|r| r.to == "K2BBB" && r.payload.trim() == "BRAVO PAYLOAD")
        );
    }

    #[test]
    fn compound_frames_are_ignored_by_buffer_assembler() {
        let frame = pack_compound_message("`K1ABC EM73", None);
        let decoded = decoded_from(&frame, FrameFlags::NONE, Submode::Normal, 1500.0);

        let mut assembler = MessageBufferAssembler::new();
        assert!(assembler.push_decoded(&decoded).is_none());
    }

    #[test]
    fn single_buffered_directed_last_frame_without_checksum_is_dropped() {
        let frame = pack_directed_message("K2XYZ MSG", "K1ABC", None, None, None, None, None);
        let decoded = decoded_from(&frame, FrameFlags::LAST, Submode::Normal, 1500.0);

        let mut assembler = MessageBufferAssembler::new();
        let event = assembler.push_decoded(&decoded);
        assert!(matches!(
            event,
            Some(ReassemblyEvent::DroppedInvalidChecksum(_))
        ));
    }

    #[test]
    fn finalize_buffered_payload_handles_16_bit_checksum_and_invalid_payload() {
        let msg16 = "HELLO THERE";
        let cs16 = crate::varicode::checksum16(msg16);
        let (out16, c16) = finalize_buffered_payload(" MSG", format!(" {msg16} {cs16}"));
        assert_eq!(out16, msg16);
        assert!(c16.as_ref().is_some_and(|c| c.valid && c.size_bits == 16u8));

        let (bad, bad_cs) = finalize_buffered_payload(" MSG", " HELLO BAD".to_string());
        assert_eq!(bad, "HELLO");
        assert!(bad_cs.is_some());
        assert!(!bad_cs.unwrap().valid);
    }
}
