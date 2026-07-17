//! Frame construction, semantic parsing, and channel-symbol encoding.

use crate::{
    protocol::{FrameFlags, FrameType, Submode},
    varicode,
};
use core::fmt;

/// A packed 12-character JS8 frame and its transmission flags.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Packed frame text.
    pub encoded: String,
    /// Message position and data flags.
    pub flags: FrameFlags,
}

impl Frame {
    /// Creates a frame from packed text and flags.
    #[must_use]
    pub fn new(encoded: impl Into<String>, flags: FrameFlags) -> Self {
        Self {
            encoded: encoded.into(),
            flags,
        }
    }
}

/// Extra addressing information found while building frames.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MessageInfo {
    /// Directed-message destination.
    pub directed_to: String,
    /// Directed command.
    pub directed_command: String,
    /// Optional directed numeric argument.
    pub directed_number: String,
}

/// Options used to split application text into JS8 frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFramesOptions {
    mycall: String,
    mygrid: String,
    selected_call: String,
    text: String,
    force_identify: bool,
    force_data: bool,
    submode: Submode,
}

impl BuildFramesOptions {
    /// Creates options for `text` in `submode`.
    #[must_use]
    pub fn new(text: impl Into<String>, submode: Submode) -> Self {
        Self {
            mycall: String::new(),
            mygrid: String::new(),
            selected_call: String::new(),
            text: text.into(),
            force_identify: false,
            force_data: false,
            submode,
        }
    }

    /// Sets the local callsign and grid used for identification frames.
    #[must_use]
    pub fn with_station(mut self, callsign: impl Into<String>, grid: impl Into<String>) -> Self {
        self.mycall = callsign.into();
        self.mygrid = grid.into();
        self
    }

    /// Sets the currently selected remote callsign.
    #[must_use]
    pub fn with_selected_call(mut self, callsign: impl Into<String>) -> Self {
        self.selected_call = callsign.into();
        self
    }

    /// Controls whether an identification frame is always generated.
    #[must_use]
    pub const fn with_identify(mut self, force: bool) -> Self {
        self.force_identify = force;
        self
    }

    /// Controls whether text is always packed as data.
    #[must_use]
    pub const fn with_data(mut self, force: bool) -> Self {
        self.force_data = force;
        self
    }

    /// Returns the selected submode.
    #[must_use]
    pub const fn submode(&self) -> Submode {
        self.submode
    }
}

/// Frames and display text produced by [`build_frames`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFramesResult {
    /// Submode used to build and encode every frame.
    pub submode: Submode,
    /// Packed frames in transmission order.
    pub frames: Vec<Frame>,
    /// Text representing the transmitted frames.
    pub transmit_text: String,
    /// Plaintext content reconstructed while building.
    pub plaintext: String,
    /// Directed-message details, when present.
    pub info: MessageInfo,
}

/// A frame encoded into its complete 79-tone waveform description.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedFrame {
    /// Packed source frame.
    pub frame: Frame,
    /// Submode used for Costas and tone selection.
    pub submode: Submode,
    /// Tone index for each transmitted symbol.
    pub tones: [u8; crate::encoder::TONES_PER_FRAME],
}

impl BuildFramesResult {
    /// Encodes every packed frame using the submode selected at build time.
    pub fn encode(self) -> Result<Vec<EncodedFrame>, EncodeError> {
        let submode = self.submode;
        self.frames
            .into_iter()
            .map(|frame| {
                let bytes = frame.encoded.as_bytes();
                if bytes.len() > 12 {
                    return Err(EncodeError::InvalidMessageLength(bytes.len()));
                }

                let mut padded = [b' '; 12];
                padded[..bytes.len()].copy_from_slice(bytes);
                let tones = encode_tones(frame.flags, submode, &padded)?;
                Ok(EncodedFrame {
                    frame,
                    submode,
                    tones,
                })
            })
            .collect()
    }
}

impl EncodedFrame {
    /// Parses the packed source frame.
    #[must_use]
    pub fn decode(&self) -> DecodedFrame {
        parse_frame(&self.frame.encoded, self.frame.flags, self.submode)
    }
}

/// Semantic contents of one decoded JS8 frame.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Classified frame type.
    pub frame_type: FrameType,
    /// Original packed frame text.
    pub encoded: String,
    /// Alternate heartbeat/CQ form.
    pub is_alt: bool,
    /// Whether this is a heartbeat or CQ frame.
    pub is_heartbeat: bool,
    /// Compound callsign, when present.
    pub compound: String,
    /// Directed-message components, when present.
    pub directed: Vec<String>,
    /// Additional unpacked frame content.
    pub extra: String,
    /// Human-readable message content.
    pub message: String,
    /// Message position and data flags.
    pub flags: FrameFlags,
    /// Submode in which the frame was encoded or received.
    pub submode: Submode,
}

/// Parsed directed-message header.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDirected {
    /// Classified frame type.
    pub frame_type: FrameType,
    /// Source callsign.
    pub from: String,
    /// Destination callsign.
    pub to: String,
    /// Directed command.
    pub command: String,
    /// Optional numeric command argument.
    pub number: Option<String>,
}

/// Parsed compound-callsign frame.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCompound {
    /// Classified frame type.
    pub frame_type: FrameType,
    /// Compound callsign.
    pub callsign: String,
    /// Optional trailing content.
    pub extra: Option<String>,
}

/// Error returned when a packed frame cannot be encoded.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// A packed character is outside the JS8 64-character alphabet.
    InvalidCharacter(u8),
    /// The packed frame does not contain exactly 12 bytes.
    InvalidMessageLength(usize),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCharacter(byte) => write!(f, "invalid character byte: 0x{byte:02X}"),
            Self::InvalidMessageLength(len) => {
                write!(f, "invalid frame length {len}; expected exactly 12 bytes")
            }
        }
    }
}

impl std::error::Error for EncodeError {}

#[must_use]
/// Splits application text into packed frames using the supplied options.
pub fn build_frames(options: &BuildFramesOptions) -> BuildFramesResult {
    let mut info = varicode::MessageInfo::default();
    let raw_frames = varicode::build_message_frames(
        &options.mycall,
        &options.mygrid,
        &options.selected_call,
        &options.text,
        options.force_identify,
        options.force_data,
        options.submode,
        Some(&mut info),
    );

    let mut frames = Vec::with_capacity(raw_frames.len());
    let mut transmit_text = String::new();
    let mut plaintext = String::new();

    for (encoded, bits) in raw_frames {
        let parsed = DecodedFrame::new(
            encoded,
            FrameFlags::from_bits_truncate(bits),
            options.submode,
        );
        transmit_text.push_str(&parsed.log_line(0, 0, 0.0, 0));
        plaintext.push_str(&parsed.message);

        frames.push(Frame {
            encoded: parsed.encoded,
            flags: FrameFlags::from_bits_truncate(bits),
        });
    }

    BuildFramesResult {
        submode: options.submode,
        frames,
        transmit_text,
        plaintext,
        info: MessageInfo {
            directed_to: info.dir_to,
            directed_command: info.dir_cmd,
            directed_number: info.dir_num,
        },
    }
}

/// Parses one packed frame into semantic protocol fields.
#[must_use]
pub fn parse_frame(encoded: &str, flags: FrameFlags, submode: Submode) -> DecodedFrame {
    DecodedFrame::new(encoded.to_owned(), flags, submode)
}

#[must_use]
/// Parses a directed-message header from packed frame text.
pub fn parse_directed(encoded: &str) -> Option<ParsedDirected> {
    let mut ty = varicode::FRAME_DIRECTED;
    let parts = varicode::unpack_directed_message(encoded, Some(&mut ty));
    if parts.len() < 3 {
        return None;
    }

    let frame_type = FrameType::try_from(ty).ok()?;
    let mut parts = parts.into_iter();
    Some(ParsedDirected {
        frame_type,
        from: parts.next()?,
        to: parts.next()?,
        command: parts.next()?,
        number: parts.next(),
    })
}

#[must_use]
/// Parses a compound callsign from packed frame text.
pub fn parse_compound(encoded: &str) -> Option<ParsedCompound> {
    let mut ty = varicode::FRAME_COMPOUND;
    let parts = varicode::unpack_compound_message(encoded, Some(&mut ty), None);
    if parts.len() < 2 {
        return None;
    }

    let mut parts = parts.into_iter();
    let callsign = parts.next()?;
    parts.next()?;

    Some(ParsedCompound {
        frame_type: FrameType::try_from(ty).ok()?,
        callsign,
        extra: parts.next(),
    })
}

/// Encodes an exact 12-byte packed frame into 79 tone indices.
pub fn encode_tones(
    flags: FrameFlags,
    submode: Submode,
    frame: &[u8; 12],
) -> Result<[u8; crate::encoder::TONES_PER_FRAME], EncodeError> {
    crate::encoder::encode(flags.bits(), submode, frame)
}
