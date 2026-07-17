// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 command definitions and parsing to Rust.

//! Allocation-conscious parsing for JS8 application commands.

use core::ops::Range;

use crate::varicode;

/// A normalized JS8 command.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandKind {
    /// Signal report query (`SNR?`).
    SnrQuery = 0,
    /// Two bits (`DIT DIT`).
    DitDit = 1,
    /// Negative acknowledgement.
    Nack = 2,
    /// Heard-stations query.
    HearingQuery = 3,
    /// Grid query.
    GridQuery = 4,
    /// Relay-path marker.
    Relay = 5,
    /// Status query.
    StatusQuery = 6,
    /// Status response.
    Status = 7,
    /// Heard-stations response.
    Hearing = 8,
    /// Buffered message.
    Msg = 9,
    /// Buffered third-party message.
    MsgTo = 10,
    /// General query.
    Query = 11,
    /// Stored-messages query.
    QueryMsgs = 12,
    /// Callsign query.
    QueryCall = 13,
    /// Acknowledgement.
    Ack = 14,
    /// Grid response.
    Grid = 15,
    /// Information query.
    InfoQuery = 16,
    /// Information response.
    Info = 17,
    /// Fine-business acknowledgement.
    Fb = 18,
    /// Copy-quality query.
    HowCopyQuery = 19,
    /// End-of-contact marker.
    Sk = 20,
    /// Received acknowledgement.
    Rr = 21,
    /// Receipt confirmation query.
    QslQuery = 22,
    /// Receipt confirmation.
    Qsl = 23,
    /// Buffered command.
    Cmd = 24,
    /// Signal report.
    Snr = 25,
    /// Negative response.
    No = 26,
    /// Affirmative response.
    Yes = 27,
    /// End-of-contact greeting (`73`).
    SeventyThree = 28,
    /// Heartbeat signal report.
    HeartbeatSnr = 29,
    /// Repeat query (`AGN?`).
    AgainQuery = 30,
    /// Unstructured free text.
    FreeText = 31,
    /// Heartbeat announcement.
    Heartbeat = 32,
    /// General call (`CQ`).
    Cq = 33,
    /// Single stored-message query.
    QueryMsg = 34,
}

impl CommandKind {
    /// Parse an exact command token produced by directed-frame decoding.
    #[must_use]
    pub fn from_wire(token: &str) -> Option<Self> {
        match token {
            " HEARTBEAT" | " HB" => Some(Self::Heartbeat),
            " CQ" => Some(Self::Cq),
            " SNR?" | "?" => Some(Self::SnrQuery),
            " DIT DIT" => Some(Self::DitDit),
            " NACK" => Some(Self::Nack),
            " HEARING?" => Some(Self::HearingQuery),
            " GRID?" => Some(Self::GridQuery),
            ">" => Some(Self::Relay),
            " STATUS?" => Some(Self::StatusQuery),
            " STATUS" => Some(Self::Status),
            " HEARING" => Some(Self::Hearing),
            " MSG" => Some(Self::Msg),
            " MSG TO:" => Some(Self::MsgTo),
            " QUERY" => Some(Self::Query),
            " QUERY MSGS" | " QUERY MSGS?" => Some(Self::QueryMsgs),
            " QUERY CALL" => Some(Self::QueryCall),
            " ACK" => Some(Self::Ack),
            " GRID" => Some(Self::Grid),
            " INFO?" => Some(Self::InfoQuery),
            " INFO" => Some(Self::Info),
            " FB" => Some(Self::Fb),
            " HW CPY?" => Some(Self::HowCopyQuery),
            " SK" => Some(Self::Sk),
            " RR" => Some(Self::Rr),
            " QSL?" => Some(Self::QslQuery),
            " QSL" => Some(Self::Qsl),
            " CMD" => Some(Self::Cmd),
            " SNR" => Some(Self::Snr),
            " NO" => Some(Self::No),
            " YES" => Some(Self::Yes),
            " 73" => Some(Self::SeventyThree),
            " HEARTBEAT SNR" => Some(Self::HeartbeatSnr),
            " AGN?" => Some(Self::AgainQuery),
            " " | "  " => Some(Self::FreeText),
            _ => None,
        }
    }

    /// Parse a command token from message text, accepting wire aliases.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        if !token.is_ascii() {
            return None;
        }
        if !token.is_empty() && token.as_bytes().iter().all(|&byte| byte == b' ') {
            return Some(Self::FreeText);
        }

        let token = token.trim_matches(' ');
        const TOKENS: &[(&str, CommandKind)] = &[
            ("HEARTBEAT SNR", CommandKind::HeartbeatSnr),
            ("QUERY MSGS?", CommandKind::QueryMsgs),
            ("QUERY MSGS", CommandKind::QueryMsgs),
            ("QUERY CALL", CommandKind::QueryCall),
            ("QUERY MSG", CommandKind::QueryMsg),
            ("CQ CONTEST", CommandKind::Cq),
            ("CQ CQ CQ", CommandKind::Cq),
            ("HW CPY?", CommandKind::HowCopyQuery),
            ("MSG TO:", CommandKind::MsgTo),
            ("HEARING?", CommandKind::HearingQuery),
            ("HEARING", CommandKind::Hearing),
            ("CQ FIELD", CommandKind::Cq),
            ("STATUS?", CommandKind::StatusQuery),
            ("STATUS", CommandKind::Status),
            ("DIT DIT", CommandKind::DitDit),
            ("HEARTBEAT", CommandKind::Heartbeat),
            ("CQ QRP", CommandKind::Cq),
            ("CQ CQ", CommandKind::Cq),
            ("CQ DX", CommandKind::Cq),
            ("CQ FD", CommandKind::Cq),
            ("SNR?", CommandKind::SnrQuery),
            ("INFO?", CommandKind::InfoQuery),
            ("GRID?", CommandKind::GridQuery),
            ("QUERY", CommandKind::Query),
            ("AGN?", CommandKind::AgainQuery),
            ("QSL?", CommandKind::QslQuery),
            ("NACK", CommandKind::Nack),
            ("INFO", CommandKind::Info),
            ("GRID", CommandKind::Grid),
            ("MSG", CommandKind::Msg),
            ("ACK", CommandKind::Ack),
            ("CMD", CommandKind::Cmd),
            ("SNR", CommandKind::Snr),
            ("QSL", CommandKind::Qsl),
            ("YES", CommandKind::Yes),
            ("NO", CommandKind::No),
            ("RR", CommandKind::Rr),
            ("SK", CommandKind::Sk),
            ("FB", CommandKind::Fb),
            ("73", CommandKind::SeventyThree),
            ("HB", CommandKind::Heartbeat),
            ("CQ", CommandKind::Cq),
            (">", CommandKind::Relay),
            ("?", CommandKind::SnrQuery),
        ];

        TOKENS
            .iter()
            .find_map(|&(name, kind)| token.eq_ignore_ascii_case(name).then_some(kind))
    }

    /// The command's 5-bit wire code. Heartbeats and CQs use heartbeat frames.
    #[must_use]
    pub const fn wire_code(self) -> Option<u8> {
        match self {
            Self::Heartbeat | Self::Cq => None,
            Self::QueryMsg => Some(Self::Query as u8),
            _ => Some(self as u8),
        }
    }

    /// The canonical token emitted by directed-frame decoding.
    #[must_use]
    pub const fn wire_token(self) -> Option<&'static str> {
        Some(match self {
            Self::SnrQuery => " SNR?",
            Self::DitDit => " DIT DIT",
            Self::Nack => " NACK",
            Self::HearingQuery => " HEARING?",
            Self::GridQuery => " GRID?",
            Self::Relay => ">",
            Self::StatusQuery => " STATUS?",
            Self::Status => " STATUS",
            Self::Hearing => " HEARING",
            Self::Msg => " MSG",
            Self::MsgTo => " MSG TO:",
            Self::Query | Self::QueryMsg => " QUERY",
            Self::QueryMsgs => " QUERY MSGS",
            Self::QueryCall => " QUERY CALL",
            Self::Ack => " ACK",
            Self::Grid => " GRID",
            Self::InfoQuery => " INFO?",
            Self::Info => " INFO",
            Self::Fb => " FB",
            Self::HowCopyQuery => " HW CPY?",
            Self::Sk => " SK",
            Self::Rr => " RR",
            Self::QslQuery => " QSL?",
            Self::Qsl => " QSL",
            Self::Cmd => " CMD",
            Self::Snr => " SNR",
            Self::No => " NO",
            Self::Yes => " YES",
            Self::SeventyThree => " 73",
            Self::HeartbeatSnr => " HEARTBEAT SNR",
            Self::AgainQuery => " AGN?",
            Self::FreeText => " ",
            Self::Heartbeat | Self::Cq => return None,
        })
    }

    /// Whether JS8Call-improved treats this command as an automatic reply request.
    #[must_use]
    pub const fn is_autoreply(self) -> bool {
        matches!(
            self.wire_code(),
            Some(0 | 2 | 3 | 4 | 6 | 9 | 10 | 11 | 12 | 13 | 14 | 16 | 30)
        )
    }

    /// Whether the canonical wire token is buffered by JS8Call-improved.
    #[must_use]
    pub const fn is_buffered(self) -> bool {
        !matches!(self, Self::Heartbeat | Self::Cq)
    }

    /// Whether the wire code is in JS8Call-improved's explicit buffered set.
    #[must_use]
    pub const fn is_buffered_code(self) -> bool {
        matches!(self.wire_code(), Some(5 | 9 | 10 | 11 | 12 | 13 | 15 | 24))
    }

    /// The command payload checksum size used on the wire.
    #[must_use]
    pub const fn checksum_bits(self) -> u8 {
        match self.wire_code() {
            Some(5 | 9 | 10 | 11 | 12 | 13 | 24) => 16,
            _ => 0,
        }
    }

    /// Whether the directed-frame numeric field is an SNR value.
    #[must_use]
    pub const fn has_snr(self) -> bool {
        matches!(self, Self::Snr | Self::HeartbeatSnr)
    }
}

/// Test an exact decoded command token using JS8Call-improved's buffering rule.
#[must_use]
pub fn is_buffered_token(token: &str) -> bool {
    CommandKind::from_wire(token)
        .is_some_and(|kind| token.as_bytes().contains(&b' ') || kind.is_buffered_code())
}

/// A validated argument that immediately follows a command.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandArg<'a> {
    /// Validated callsign argument.
    Call(&'a str),
    /// Validated Maidenhead grid argument.
    Grid(&'a str),
    /// Validated numeric message identifier.
    MessageId {
        /// Original decimal text.
        raw: &'a str,
        /// Parsed numeric value.
        value: i32,
    },
}

/// A relay route recognized at the start of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayRoute<'a> {
    path: &'a str,
    recipient: &'a str,
    implicit: Option<&'a str>,
}

impl<'a> RelayRoute<'a> {
    #[must_use]
    /// Returns the complete relay path text.
    pub const fn path(self) -> &'a str {
        self.path
    }

    #[must_use]
    /// Returns the final route recipient.
    pub const fn recipient(self) -> &'a str {
        self.recipient
    }

    #[must_use]
    /// Returns whether the route begins with an implicit hop.
    pub const fn is_partial(self) -> bool {
        self.implicit.is_some()
    }

    #[must_use]
    /// Iterates callsigns in route order.
    pub const fn hops(self) -> RelayHops<'a> {
        RelayHops {
            implicit: self.implicit,
            path: self.path,
            pos: 0,
        }
    }
}

/// Iterator over a relay route, including the implicit first hop when present.
#[derive(Debug, Clone)]
pub struct RelayHops<'a> {
    implicit: Option<&'a str>,
    path: &'a str,
    pos: usize,
}

impl<'a> Iterator for RelayHops<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(call) = self.implicit.take() {
            return Some(call);
        }

        let bytes = self.path.as_bytes();
        while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'>') {
            self.pos += 1;
        }
        if self.pos == bytes.len() {
            return None;
        }

        let start = self.pos;
        while self.pos < bytes.len() && !matches!(bytes[self.pos], b' ' | b'>') {
            self.pos += 1;
        }
        Some(&self.path[start..self.pos])
    }
}

/// The recipient syntax preceding a parsed command.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// Direct callsign target.
    Call(&'a str),
    /// Named group target such as `@ALLCALL`.
    Group(&'a str),
    /// Multi-hop relay target.
    Relay(RelayRoute<'a>),
    /// Target supplied separately by caller context.
    Implicit(&'a str),
}

impl<'a> Target<'a> {
    #[must_use]
    /// Returns the effective recipient callsign or group.
    pub const fn recipient(self) -> &'a str {
        match self {
            Self::Call(call) | Self::Group(call) | Self::Implicit(call) => call,
            Self::Relay(route) => route.recipient(),
        }
    }
}

/// A parsed message command. Every string field borrows its input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand<'a> {
    /// Optional explicit sender prefix.
    pub sender: Option<&'a str>,
    /// Parsed direct, group, relay, or implicit target.
    pub target: Option<Target<'a>>,
    /// Normalized command kind.
    pub kind: CommandKind,
    /// Exact command token found in the input.
    pub command: &'a str,
    /// Typed command argument, when required.
    pub arg: Option<CommandArg<'a>>,
    /// Remaining message payload.
    pub payload: &'a str,
}

#[derive(Clone, Copy)]
enum ArgKind {
    None,
    Call,
    QueryCall,
    MessageId,
    Grid,
}

#[derive(Clone, Copy)]
struct CommandDef {
    text: &'static str,
    kind: CommandKind,
    arg: ArgKind,
    bare: bool,
}

const COMMANDS: &[CommandDef] = &[
    CommandDef {
        text: "AGN?",
        kind: CommandKind::AgainQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "QSL?",
        kind: CommandKind::QslQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "HW CPY?",
        kind: CommandKind::HowCopyQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "SNR?",
        kind: CommandKind::SnrQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "INFO?",
        kind: CommandKind::InfoQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "GRID?",
        kind: CommandKind::GridQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "STATUS?",
        kind: CommandKind::StatusQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "QUERY MSGS?",
        kind: CommandKind::QueryMsgs,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "HEARING?",
        kind: CommandKind::HearingQuery,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "MSG TO:",
        kind: CommandKind::MsgTo,
        arg: ArgKind::Call,
        bare: false,
    },
    CommandDef {
        text: "HEARTBEAT SNR",
        kind: CommandKind::HeartbeatSnr,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "QUERY MSGS",
        kind: CommandKind::QueryMsgs,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "QUERY CALL",
        kind: CommandKind::QueryCall,
        arg: ArgKind::QueryCall,
        bare: false,
    },
    CommandDef {
        text: "QUERY MSG",
        kind: CommandKind::QueryMsg,
        arg: ArgKind::MessageId,
        bare: false,
    },
    CommandDef {
        text: "QUERY",
        kind: CommandKind::Query,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "DIT DIT",
        kind: CommandKind::DitDit,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "STATUS",
        kind: CommandKind::Status,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "HEARING",
        kind: CommandKind::Hearing,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "CMD",
        kind: CommandKind::Cmd,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "MSG",
        kind: CommandKind::Msg,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "NACK",
        kind: CommandKind::Nack,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "ACK",
        kind: CommandKind::Ack,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "SNR",
        kind: CommandKind::Snr,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "QSL",
        kind: CommandKind::Qsl,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "INFO",
        kind: CommandKind::Info,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "GRID",
        kind: CommandKind::Grid,
        arg: ArgKind::Grid,
        bare: false,
    },
    CommandDef {
        text: "73",
        kind: CommandKind::SeventyThree,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "YES",
        kind: CommandKind::Yes,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "NO",
        kind: CommandKind::No,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "RR",
        kind: CommandKind::Rr,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "SK",
        kind: CommandKind::Sk,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "FB",
        kind: CommandKind::Fb,
        arg: ArgKind::None,
        bare: false,
    },
    CommandDef {
        text: "CQ CQ CQ",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ CONTEST",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ FIELD",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ DX",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ QRP",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ FD",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ CQ",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "CQ",
        kind: CommandKind::Cq,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "HB",
        kind: CommandKind::Heartbeat,
        arg: ArgKind::Grid,
        bare: true,
    },
    CommandDef {
        text: "HEARTBEAT",
        kind: CommandKind::Heartbeat,
        arg: ArgKind::Grid,
        bare: true,
    },
];

const ACK_COMMANDS: &[u8] = &[21];
const AGAIN_COMMANDS: &[u8] = &[0];
const CMD_COMMANDS: &[u8] = &[18];
const CQ_COMMANDS: &[u8] = &[33, 32, 34, 36, 38, 35, 37, 39];
const D_COMMANDS: &[u8] = &[15];
const F_COMMANDS: &[u8] = &[31];
const G_COMMANDS: &[u8] = &[5, 25];
const HB_COMMANDS: &[u8] = &[40];
const HEARING_COMMANDS: &[u8] = &[8, 17];
const HEARTBEAT_COMMANDS: &[u8] = &[10, 41];
const HW_COMMANDS: &[u8] = &[2];
const I_COMMANDS: &[u8] = &[4, 24];
const M_COMMANDS: &[u8] = &[9, 19];
const NACK_COMMANDS: &[u8] = &[20];
const NO_COMMANDS: &[u8] = &[28];
const QUERY_COMMANDS: &[u8] = &[7, 12, 11, 13, 14];
const QSL_COMMANDS: &[u8] = &[1, 23];
const R_COMMANDS: &[u8] = &[29];
const SK_COMMANDS: &[u8] = &[30];
const SNR_COMMANDS: &[u8] = &[3, 22];
const STATUS_COMMANDS: &[u8] = &[6, 16];
const Y_COMMANDS: &[u8] = &[27];
const SEVENTY_COMMANDS: &[u8] = &[26];

fn command_candidates(bytes: &[u8]) -> &'static [u8] {
    let first = bytes[0].to_ascii_uppercase();
    let second = bytes
        .get(1)
        .copied()
        .unwrap_or_default()
        .to_ascii_uppercase();
    match (first, second) {
        (b'A', b'C') => ACK_COMMANDS,
        (b'A', b'G') => AGAIN_COMMANDS,
        (b'C', b'M') => CMD_COMMANDS,
        (b'C', b'Q') => CQ_COMMANDS,
        (b'D', _) => D_COMMANDS,
        (b'F', _) => F_COMMANDS,
        (b'G', _) => G_COMMANDS,
        (b'H', b'B') => HB_COMMANDS,
        (b'H', b'E') => match bytes
            .get(4)
            .copied()
            .unwrap_or_default()
            .to_ascii_uppercase()
        {
            b'I' => HEARING_COMMANDS,
            b'T' => HEARTBEAT_COMMANDS,
            _ => &[],
        },
        (b'H', b'W') => HW_COMMANDS,
        (b'I', _) => I_COMMANDS,
        (b'M', _) => M_COMMANDS,
        (b'N', b'A') => NACK_COMMANDS,
        (b'N', b'O') => NO_COMMANDS,
        (b'Q', b'U') => QUERY_COMMANDS,
        (b'Q', b'S') => QSL_COMMANDS,
        (b'R', _) => R_COMMANDS,
        (b'S', b'K') => SK_COMMANDS,
        (b'S', b'N') => SNR_COMMANDS,
        (b'S', b'T') => STATUS_COMMANDS,
        (b'Y', _) => Y_COMMANDS,
        (b'7', _) => SEVENTY_COMMANDS,
        _ => &[],
    }
}

#[derive(Clone, Copy)]
struct CommandMatch<'a> {
    def: CommandDef,
    command: &'a str,
    arg: Option<CommandArg<'a>>,
    end: usize,
}

/// Parse one JS8 message line into its addressing and command components.
///
/// `implicit_target` models JS8Call's selected callsign. It enables commands
/// without an explicit recipient and is also the first hop of a partial relay.
#[must_use]
pub fn parse_command<'a>(
    text: &'a str,
    implicit_target: Option<&'a str>,
) -> Option<ParsedCommand<'a>> {
    if text.is_empty() || text.starts_with('`') {
        return None;
    }
    let implicit_target = implicit_target.filter(|target| !target.is_empty());

    let first = parse_hop(text, 0);
    let (sender_range, content_start) = parse_sender(text, first.as_ref());
    let sender = sender_range.map(|range| &text[range]);
    let content = &text[content_start..];
    let first = if content_start == 0 {
        first
    } else {
        parse_hop(content, 0)
    };

    let mut pos = 0;
    let mut target = None;

    let relay = parse_full_relay(content, first);
    if let Some(relay) = &relay
        && relay.count >= 2
    {
        if relay.valid >= 2 {
            let end = relay.last.end;
            target = Some(Target::Relay(RelayRoute {
                path: &content[..end],
                recipient: &content[relay.last.clone()],
                implicit: None,
            }));
            pos = end;
        } else if relay.valid == 1 {
            let range = relay.first.clone();
            pos = range.end;
            target = Some(Target::Call(&content[range]));
        }
    }

    if target.is_none()
        && let Some(relay) = &relay
        && relay.count == 1
    {
        let end = relay.first.end;
        if end == content.len() || content.as_bytes()[end] == b' ' {
            let address = &content[..end];
            if address.starts_with('@') {
                if is_compound_call(address) {
                    target = Some(Target::Group(&content[..end]));
                    pos = end;
                }
            } else if relay.valid == 1 {
                target = Some(Target::Call(&content[..end]));
                pos = end;
            }
        }
    }

    if target.is_none()
        && content.starts_with('>')
        && implicit_target.is_some_and(is_relay_first)
        && let Some(relay) = parse_partial_relay(content)
        && relay.valid != 0
    {
        let end = relay.last.end;
        target = Some(Target::Relay(RelayRoute {
            path: &content[..end],
            recipient: &content[relay.last],
            implicit: implicit_target,
        }));
        pos = end;
    }

    let explicit = target.is_some();
    if !explicit && let Some(found) = match_command(content, content, 0, true) {
        return Some(build_result(sender, None, content, found));
    }

    if !explicit {
        target = implicit_target.map(Target::Implicit);
    }
    target?;

    while pos < content.len() && content.as_bytes()[pos] == b' ' {
        pos += 1;
    }
    let found = match_command(content, content, pos, false)?;
    Some(build_result(sender, target, content, found))
}

fn build_result<'a>(
    sender: Option<&'a str>,
    target: Option<Target<'a>>,
    content: &'a str,
    found: CommandMatch<'a>,
) -> ParsedCommand<'a> {
    let mut payload_start = found.end;
    let bytes = content.as_bytes();
    while payload_start < bytes.len() && bytes[payload_start] == b' ' {
        payload_start += 1;
    }

    ParsedCommand {
        sender,
        target,
        kind: found.def.kind,
        command: found.command,
        arg: found.arg,
        payload: &content[payload_start..],
    }
}

fn parse_sender(text: &str, first: Option<&Range<usize>>) -> (Option<Range<usize>>, usize) {
    let bytes = text.as_bytes();
    let Some(colon) = first.map(|range| range.end) else {
        return (None, 0);
    };
    if bytes.get(colon) != Some(&b':') {
        return (None, 0);
    }
    if colon == 0 || colon + 1 >= text.len() || text.as_bytes()[colon + 1] != b' ' {
        return (None, 0);
    }

    let sender = &text[..colon];
    if sender.starts_with('@') || !is_valid_call(sender) {
        return (None, 0);
    }
    (Some(0..colon), colon + 2)
}

struct RelayScan {
    first: Range<usize>,
    last: Range<usize>,
    count: usize,
    valid: usize,
}

fn parse_full_relay(text: &str, first: Option<Range<usize>>) -> Option<RelayScan> {
    let first = first?;
    let mut pos = first.end;
    let mut count = 1;
    let mut valid = usize::from(is_relay_first(&text[first.clone()]));
    let mut last = first.clone();

    loop {
        while pos < text.len() && text.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        if pos == text.len() || text.as_bytes()[pos] != b'>' {
            break;
        }
        pos += 1;
        while pos < text.len() && text.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        let Some(hop) = parse_hop(text, pos) else {
            break;
        };
        pos = hop.end;
        count += 1;
        if valid + 1 == count && is_relay_follow(&text[hop.clone()]) {
            valid += 1;
            last = hop;
        }
    }

    Some(RelayScan {
        first,
        last,
        count,
        valid,
    })
}

fn parse_partial_relay(text: &str) -> Option<RelayScan> {
    let mut first = None;
    let mut last = 0..0;
    let mut valid = 0;
    let mut count = 0;
    let mut pos = 0;
    while pos < text.len() && text.as_bytes()[pos] == b'>' {
        pos += 1;
        while pos < text.len() && text.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        let Some(hop) = parse_hop(text, pos) else {
            break;
        };
        pos = hop.end;
        count += 1;
        if first.is_none() {
            first = Some(hop.clone());
        }
        if valid + 1 == count && is_relay_follow(&text[hop.clone()]) {
            valid += 1;
            last = hop;
        }
        while pos < text.len() && text.as_bytes()[pos] == b' ' {
            pos += 1;
        }
    }
    Some(RelayScan {
        first: first?,
        last,
        count,
        valid,
    })
}

fn parse_hop(text: &str, start: usize) -> Option<Range<usize>> {
    let mut end = start;
    let bytes = text.as_bytes();
    if bytes.get(start).is_none_or(|byte| !is_hop_byte(*byte)) {
        return None;
    }
    while end < bytes.len() && is_hop_byte(bytes[end]) {
        end += 1;
    }
    Some(start..end)
}

const fn is_hop_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'@' | b'/')
}

fn is_relay_first(call: &str) -> bool {
    !call.starts_with('@') && is_valid_call(call)
}

fn is_relay_follow(call: &str) -> bool {
    if call.starts_with('@') || !call.is_ascii() {
        return false;
    }
    let mut parts = call.split('/');
    let first = parts.next().unwrap_or_default();
    let second = parts.next();
    let third = parts.next();
    if parts.next().is_some() {
        return false;
    }

    match (second, third) {
        (None, None) => is_base_call(first),
        (Some(base_or_suffix), None) => {
            (is_affix(first) && is_base_call(base_or_suffix))
                || (is_base_call(first) && is_affix(base_or_suffix))
        }
        (Some(base), Some(suffix)) => is_affix(first) && is_base_call(base) && is_affix(suffix),
        _ => false,
    }
}

fn is_affix(text: &str) -> bool {
    (1..=4).contains(&text.len()) && text.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_base_call(call: &str) -> bool {
    let bytes = call.as_bytes();
    if !(2..=6).contains(&bytes.len()) {
        return false;
    }

    [1, 2].into_iter().any(|digit| {
        bytes.get(digit).is_some_and(u8::is_ascii_digit)
            && bytes[..digit].iter().all(u8::is_ascii_alphanumeric)
            && bytes[digit + 1..].iter().all(u8::is_ascii_alphabetic)
            && bytes.len() - digit - 1 <= 3
    })
}

fn is_valid_call(call: &str) -> bool {
    if !call.is_ascii() {
        return false;
    }
    if call.eq_ignore_ascii_case("<....>") {
        return true;
    }

    let base = call
        .strip_suffix("/P")
        .or_else(|| call.strip_suffix("/p"))
        .unwrap_or(call);
    if is_base_call(base) && base.len() > 2 && has_letter_digit_pair(base.as_bytes()) {
        return true;
    }

    is_valid_compound(call) && is_compound_shape(call)
}

fn is_compound_call(call: &str) -> bool {
    if !call.is_ascii() || (!call.starts_with('@') && is_valid_base(call)) {
        return false;
    }
    is_valid_compound(call) && is_compound_shape(call)
}

fn is_valid_base(call: &str) -> bool {
    let base = call
        .strip_suffix("/P")
        .or_else(|| call.strip_suffix("/p"))
        .unwrap_or(call);
    is_base_call(base) && base.len() > 2 && has_letter_digit_pair(base.as_bytes())
}

fn is_valid_compound(call: &str) -> bool {
    let bytes = call.as_bytes();
    let slash_count = bytes.iter().filter(|&&byte| byte == b'/').count();
    if bytes.len().saturating_sub(slash_count) > 9 {
        return false;
    }

    if let Some(index) = bytes.iter().position(|&byte| byte == b'/') {
        let prefix = &call[..index];
        return !varicode::BASECALLS
            .keys()
            .any(|known| prefix.eq_ignore_ascii_case(known));
    }
    call.starts_with('@') || (call.len() > 2 && has_letter_digit_pair(bytes))
}

fn has_letter_digit_pair(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|pair| {
        (pair[0].is_ascii_digit() && pair[1].is_ascii_alphabetic())
            || (pair[0].is_ascii_alphabetic() && pair[1].is_ascii_digit())
    })
}

fn is_compound_shape(call: &str) -> bool {
    fn matches_extended(bytes: &[u8], allow_at: bool) -> bool {
        if bytes.is_empty() || !bytes.last().is_some_and(u8::is_ascii_alphanumeric) {
            return false;
        }

        let valid = |bytes: &[u8], first: bool| {
            bytes.iter().enumerate().all(|(index, &byte)| {
                byte.is_ascii_alphanumeric()
                    || byte == b'/'
                    || (allow_at && first && index == 0 && byte == b'@')
            })
        };

        for a in 1..=3.min(bytes.len()) {
            if !valid(&bytes[..a], true) {
                continue;
            }
            for sep_a in 0..=1 {
                let mut pos = a;
                if sep_a != 0 {
                    if bytes.get(pos) != Some(&b'/') {
                        continue;
                    }
                    pos += 1;
                }
                for b in 0..=3.min(bytes.len() - pos) {
                    if !valid(&bytes[pos..pos + b], false) {
                        continue;
                    }
                    for sep_b in 0..=1 {
                        let mut end = pos + b;
                        if sep_b != 0 {
                            if bytes.get(end) != Some(&b'/') {
                                continue;
                            }
                            end += 1;
                        }
                        let tail = bytes.len() - end;
                        if tail <= 3 && valid(&bytes[end..], false) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    let bytes = call.as_bytes();
    matches_extended(bytes, true)
        || bytes
            .strip_prefix(b"@")
            .is_some_and(|rest| matches_extended(rest, true))
}

fn match_command<'a>(
    upper: &str,
    original: &'a str,
    start: usize,
    bare_only: bool,
) -> Option<CommandMatch<'a>> {
    if start >= upper.len() {
        return None;
    }

    let mut best = None;
    for &index in command_candidates(&upper.as_bytes()[start..]) {
        let def = COMMANDS[index as usize];
        let end = start + def.text.len();
        if end > upper.len()
            || !upper.as_bytes()[start..end].eq_ignore_ascii_case(def.text.as_bytes())
        {
            continue;
        }
        if !matches!(def.text.as_bytes().last(), Some(b'?' | b':'))
            && end < upper.len()
            && upper.as_bytes()[end] != b' '
        {
            continue;
        }

        best = Some(def);
        break;
    }

    let Some(def) = best else {
        if !bare_only && upper.as_bytes()[start] == b'?' {
            return Some(CommandMatch {
                def: CommandDef {
                    text: "?",
                    kind: CommandKind::SnrQuery,
                    arg: ArgKind::None,
                    bare: false,
                },
                command: &original[start..start + 1],
                arg: None,
                end: start + 1,
            });
        }
        return None;
    };
    if bare_only && !def.bare {
        return None;
    }

    let command_end = start + def.text.len();
    let (arg, final_end) = if matches!(def.arg, ArgKind::None) {
        (None, command_end)
    } else {
        let mut arg_start = command_end;
        while arg_start < upper.len() && upper.as_bytes()[arg_start] == b' ' {
            arg_start += 1;
        }
        let arg_end = upper.as_bytes()[arg_start..]
            .iter()
            .position(|&byte| byte == b' ')
            .map_or(upper.len(), |len| arg_start + len);
        parse_arg(def.arg, upper, original, arg_start..arg_end)
            .map_or((None, command_end), |arg| (Some(arg), arg_end))
    };

    Some(CommandMatch {
        def,
        command: &original[start..command_end],
        arg,
        end: final_end,
    })
}

fn parse_arg<'a>(
    kind: ArgKind,
    upper: &str,
    original: &'a str,
    mut range: Range<usize>,
) -> Option<CommandArg<'a>> {
    if matches!(kind, ArgKind::None) || range.is_empty() {
        return None;
    }

    match kind {
        ArgKind::None => None,
        ArgKind::Call => {
            is_valid_call(&upper[range.clone()]).then(|| CommandArg::Call(&original[range]))
        }
        ArgKind::QueryCall => {
            if upper.as_bytes().get(range.end.wrapping_sub(1)) == Some(&b'?') {
                range.end -= 1;
            }
            (!range.is_empty() && is_valid_call(&upper[range.clone()]))
                .then(|| CommandArg::Call(&original[range]))
        }
        ArgKind::MessageId => {
            let raw = &upper[range.clone()];
            if !raw.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let value = raw.parse::<i32>().ok()?;
            Some(CommandArg::MessageId {
                raw: &original[range],
                value,
            })
        }
        ArgKind::Grid => is_grid(&upper[range.clone()]).then(|| CommandArg::Grid(&original[range])),
    }
}

fn is_grid(grid: &str) -> bool {
    let bytes = grid.as_bytes();
    matches!(bytes.len(), 4 | 6 | 8 | 10 | 12)
        && bytes.chunks_exact(2).enumerate().all(|(pair, bytes)| {
            if pair % 2 == 0 {
                let max = if pair == 0 { b'R' } else { b'X' };
                bytes
                    .iter()
                    .all(|&byte| (b'A'..=max).contains(&byte.to_ascii_uppercase()))
            } else {
                bytes.iter().all(u8::is_ascii_digit)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_callsign_checks_match_varicode() {
        for call in [
            "K1ABC",
            "K1ABC/P",
            "3DA0XYZ",
            "3XABC",
            "@ALLCALL",
            "@ARESGA",
            "@RESERVE/0",
            "@@A1",
            "@@@A1",
            "FOO/K1ABC",
            "K1ABC/FOO",
            "FOO/BAR",
            "HELLO",
            "<....>",
            "@",
            "/",
        ] {
            assert_eq!(
                is_valid_call(call),
                varicode::is_valid_callsign(call, None),
                "validity differs for {call:?}"
            );
            assert_eq!(
                is_compound_call(call),
                varicode::is_compound_callsign(call),
                "compound classification differs for {call:?}"
            );
        }

        const ALPHABET: &[u8] = b"ABXZ019/P@";
        let mut state = 0xA5A5_1234_u32;
        for _ in 0..20_000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let len = (state as usize % 12) + 1;
            let mut bytes = [0_u8; 12];
            for byte in &mut bytes[..len] {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *byte = ALPHABET[state as usize % ALPHABET.len()];
            }
            let call = core::str::from_utf8(&bytes[..len]).unwrap();
            assert_eq!(
                is_valid_call(call),
                varicode::is_valid_callsign(call, None),
                "validity differs for {call:?}"
            );
            assert_eq!(
                is_compound_call(call),
                varicode::is_compound_callsign(call),
                "compound classification differs for {call:?}"
            );
        }
    }

    #[test]
    fn command_buckets_are_complete_and_ordered() {
        let buckets = [
            ACK_COMMANDS,
            AGAIN_COMMANDS,
            CMD_COMMANDS,
            CQ_COMMANDS,
            D_COMMANDS,
            F_COMMANDS,
            G_COMMANDS,
            HB_COMMANDS,
            HEARING_COMMANDS,
            HEARTBEAT_COMMANDS,
            HW_COMMANDS,
            I_COMMANDS,
            M_COMMANDS,
            NACK_COMMANDS,
            NO_COMMANDS,
            QUERY_COMMANDS,
            QSL_COMMANDS,
            R_COMMANDS,
            SK_COMMANDS,
            SNR_COMMANDS,
            STATUS_COMMANDS,
            Y_COMMANDS,
            SEVENTY_COMMANDS,
        ];
        let mut seen = [false; COMMANDS.len()];

        for bucket in buckets {
            let mut previous: Option<CommandDef> = None;
            for &index in bucket {
                let index = usize::from(index);
                assert!(!seen[index], "duplicate command index {index}");
                seen[index] = true;
                let current = COMMANDS[index];
                if let Some(previous) = previous {
                    assert!(
                        previous.text.len() > current.text.len()
                            || (previous.text.len() == current.text.len()
                                && previous.text < current.text),
                        "bucket precedence is wrong for {:?} and {:?}",
                        previous.text,
                        current.text
                    );
                }
                previous = Some(current);
            }
        }

        assert!(seen.into_iter().all(|value| value));
    }
}
