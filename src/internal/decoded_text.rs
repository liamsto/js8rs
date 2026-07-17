// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported decoded interpretation to Rust.

use crate::{
    codec::DecodedFrame,
    internal::commons::decode_time,
    protocol::{
        FrameFlags,
        FrameType::{FrameCompound, FrameCompoundDirected, FrameData, FrameUnknown},
        Submode,
    },
    varicode::{self, unpack_compound_message, unpack_data_message, unpack_heartbeat_message},
};

/// Create and return a potentially compound call from the provided parts.
/// The parts are guaranteed to be at least size 2 in the original usage,
/// but any part might be empty.
fn build_compound(parts: &[String]) -> String {
    let mut kept: Vec<&str> = Vec::with_capacity(2);
    for p in parts.iter().take(2) {
        if !p.is_empty() {
            kept.push(p.as_str());
        }
    }
    kept.join("/")
}

impl DecodedFrame {
    #[must_use]
    pub(crate) fn new(encoded: String, flags: FrameFlags, submode: Submode) -> Self {
        let mut this = Self {
            frame_type: FrameUnknown,
            encoded: encoded.clone(),
            is_alt: false,
            is_heartbeat: false,
            compound: String::new(),
            directed: Vec::new(),
            extra: String::new(),
            message: encoded,
            flags,
            submode,
        };

        let m = this.message.trim().to_owned();

        if m.chars().count() < 12 || m.contains(' ') {
            return this;
        }

        // Attempt unpack strategies in order until one succeeds.
        if this.try_unpack_fast_data(&m) {
            return this;
        }
        if this.try_unpack_data(&m) {
            return this;
        }
        if this.try_unpack_heartbeat(&m) {
            return this;
        }
        if this.try_unpack_compound(&m) {
            return this;
        }
        let _ = this.try_unpack_directed(&m);

        this
    }

    fn try_unpack_fast_data(&mut self, m: &str) -> bool {
        if !self.flags.contains(FrameFlags::DATA) {
            return false;
        }
        let data = varicode::unpack_fast_data_message(m);
        if data.is_empty() {
            false
        } else {
            self.message = data;
            self.frame_type = FrameData;
            true
        }
    }

    fn try_unpack_data(&mut self, m: &str) -> bool {
        if self.flags.contains(FrameFlags::DATA) {
            return false;
        }

        let data = unpack_data_message(m);
        if data.is_empty() {
            false
        } else {
            self.message = data;
            self.frame_type = FrameData;
            true
        }
    }

    fn try_unpack_heartbeat(&mut self, m: &str) -> bool {
        if self.flags.contains(FrameFlags::DATA) {
            return false;
        }

        let mut is_alt = false;
        let mut ftype: u8 = FrameUnknown as u8;
        let mut bits3: u8 = 0;

        let parts =
            unpack_heartbeat_message(m, Some(&mut ftype), Some(&mut is_alt), Some(&mut bits3));
        if parts.len() < 2 {
            return false;
        }

        self.frame_type = ftype.try_into().unwrap_or(FrameUnknown);
        self.is_heartbeat = true;
        self.is_alt = is_alt;

        self.extra = parts.get(2).cloned().unwrap_or_default();
        self.compound = build_compound(&parts);

        let mut msg = String::new();
        msg.push_str(&self.compound);
        msg.push_str(": ");

        if is_alt {
            msg.push_str("@ALLCALL ");
            msg.push_str(&varicode::cq_string(bits3.into()));
        } else {
            msg.push_str("@HB ");
            let sbits3 = varicode::hb_string(bits3.into());
            if sbits3 == "HB" {
                msg.push_str("HEARTBEAT");
            } else {
                msg.push_str(&sbits3);
            }
        }

        msg.push(' ');
        msg.push_str(&self.extra);
        msg.push(' ');

        self.message = msg;
        true
    }

    fn try_unpack_compound(&mut self, m: &str) -> bool {
        let mut ftype: u8 = FrameUnknown as u8;
        let mut bits3: u8 = 0;

        let parts = unpack_compound_message(m, Some(&mut ftype), Some(&mut bits3));

        if parts.len() < 2 || self.flags.contains(FrameFlags::DATA) {
            return false;
        }

        self.frame_type = ftype.try_into().unwrap_or(FrameUnknown);
        self.extra = parts.get(2..).map(|s| s.join(" ")).unwrap_or_default();
        self.compound = build_compound(&parts);

        if ftype == FrameCompound as u8 {
            let mut msg = String::new();
            msg.push_str(&self.compound);
            msg.push_str(": ");
            self.message = msg;
        } else if ftype == FrameCompoundDirected as u8 {
            let mut msg = String::new();
            msg.push_str(&self.compound);
            msg.push_str(&self.extra);
            msg.push(' ');
            self.message = msg;

            self.directed = Vec::with_capacity(parts.len());
            self.directed.push("<....>".to_string());
            self.directed.push(self.compound.clone());
            if let Some(rest) = parts.get(2..) {
                self.directed.extend(rest.iter().cloned());
            }
        }

        true
    }

    fn try_unpack_directed(&mut self, m: &str) -> bool {
        if self.flags.contains(FrameFlags::DATA) {
            return false;
        }

        let mut ftype: u8 = FrameUnknown as u8;
        let parts = varicode::unpack_directed_message(m, Some(&mut ftype));

        if parts.is_empty() {
            return false;
        }

        match parts.len() {
            3 | 4 => {
                let mut msg = String::new();
                msg.push_str(&parts[0]);
                msg.push_str(": ");
                msg.push_str(&parts[1]);
                msg.push_str(&parts[2..].join(" "));
                msg.push(' ');
                self.message = msg;
            }
            _ => {
                self.message = parts.join("");
            }
        }

        self.directed = parts;
        self.frame_type = ftype.try_into().unwrap_or(FrameUnknown);
        true
    }

    /// Simple word split for free text messages.
    #[must_use]
    pub fn message_words(&self) -> Vec<String> {
        let space_count = self
            .message
            .as_bytes()
            .iter()
            .filter(|&&b| b == b' ')
            .count();
        let mut words = Vec::with_capacity(space_count + 2);

        words.push(self.message.clone());
        words.extend(
            self.message
                .split(' ')
                .filter(|s| !s.is_empty())
                .map(std::string::ToString::to_string),
        );

        words
    }

    pub(crate) fn log_line(&self, utc: u32, snr: i32, dt: f32, frequency_offset: u32) -> String {
        let hms = decode_time(utc);
        format!(
            "{:02}:{:02}:{:02}{:>3} {:>4.1} {:>4} {}  {}         {}   ",
            hms.hour,
            hms.minute,
            hms.second,
            snr,
            dt,
            frequency_offset,
            self.submode,
            self.encoded,
            self.flags.bits()
        )
    }

    #[must_use]
    /// Returns whether the frame contains a compound callsign.
    pub const fn is_compound(&self) -> bool {
        !self.compound.is_empty()
    }

    #[must_use]
    /// Returns whether the frame contains directed-message fields.
    pub const fn is_directed(&self) -> bool {
        self.directed.len() > 2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FrameFlags, FrameType, Submode};
    use crate::varicode::{pack_compound_message, pack_directed_message, pack_heartbeat_message};

    #[test]
    fn short_or_spaced_frames_are_left_unparsed() {
        let short = DecodedFrame::new("ABC".to_string(), FrameFlags::NONE, Submode::Fast);
        assert_eq!(short.frame_type, FrameType::FrameUnknown);
        assert_eq!(short.message, "ABC");

        let spaced = DecodedFrame::new("AB CD EF GH".to_string(), FrameFlags::NONE, Submode::Fast);
        assert_eq!(spaced.frame_type, FrameType::FrameUnknown);
        assert_eq!(spaced.message, "AB CD EF GH");
    }

    #[test]
    fn fast_data_unpack_takes_priority_when_data_bit_set() {
        let input = "HELLO WORLD";
        let frame = crate::varicode::pack_fast_data_message(input, None);
        let decoded = DecodedFrame::new(frame, FrameFlags::DATA, Submode::Fast);

        assert_eq!(decoded.frame_type, FrameType::FrameData);
        assert_eq!(decoded.message, input);
    }

    #[test]
    fn legacy_data_unpack_path_decodes_non_data_bit_frames() {
        let input = "HELLO WORLD";
        let frame = crate::varicode::pack_huff_message(input, &[true, false], None);
        let decoded = DecodedFrame::new(frame, FrameFlags::NONE, Submode::Normal);

        assert_eq!(decoded.frame_type, FrameType::FrameData);
        assert_eq!(decoded.message, input);
    }

    #[test]
    fn heartbeat_unpack_sets_alt_and_message_shape() {
        let hb_frame = pack_heartbeat_message("HB EM73", "K1ABC", None);
        let hb = DecodedFrame::new(hb_frame, FrameFlags::NONE, Submode::Normal);
        assert!(hb.is_heartbeat);
        assert!(!hb.is_alt);
        assert!(hb.message.contains("@HB HEARTBEAT"));
        assert!(hb.message.contains("EM73"));

        let cq_frame = pack_heartbeat_message("CQ CQ CQ EM73", "K1ABC", None);
        let cq = DecodedFrame::new(cq_frame, FrameFlags::NONE, Submode::Normal);
        assert!(cq.is_heartbeat);
        assert!(cq.is_alt);
        assert!(cq.message.contains("@ALLCALL"));
    }

    #[test]
    fn compound_directed_populates_directed_parts_with_placeholder() {
        let frame = pack_compound_message("`K1ABC MSG", None);
        let dt = DecodedFrame::new(frame, FrameFlags::NONE, Submode::Fast);

        assert_eq!(dt.frame_type, FrameType::FrameCompoundDirected);
        assert_eq!(dt.directed[0], "<....>");
        assert_eq!(dt.directed[1], "K1ABC");
        assert_eq!(dt.directed[2].trim(), "MSG");
        assert!(dt.is_directed());
    }

    #[test]
    fn directed_unpack_formats_message_with_sender_prefix() {
        let frame = pack_directed_message("K1ABC MSG", "N0CALL", None, None, None, None, None);
        assert!(!frame.is_empty());

        let dt = DecodedFrame::new(frame, FrameFlags::NONE, Submode::Fast);
        assert_eq!(dt.frame_type, FrameType::FrameDirected);
        assert_eq!(dt.directed[0], "<....>");
        assert_eq!(dt.directed[1], "K1ABC");
        assert_eq!(dt.directed[2].trim(), "MSG");
        assert!(dt.message.starts_with("<....>: K1ABC"));
        assert!(dt.message.ends_with(' '));
    }

    #[test]
    fn message_words_and_log_line_follow_all_txt_shape() {
        let frame = crate::varicode::pack_fast_data_message("HELLO WORLD", None);
        let dt = DecodedFrame::new(frame, FrameFlags::DATA, Submode::Fast);
        let words = dt.message_words();
        assert_eq!(words[0], "HELLO WORLD");
        assert_eq!(words[1], "HELLO");
        assert_eq!(words[2], "WORLD");

        let rendered = dt.log_line(123_045, -5, 0.1, 1234);
        assert!(rendered.contains("12:30:45"));
        assert!(rendered.contains(&dt.encoded));
    }
}
