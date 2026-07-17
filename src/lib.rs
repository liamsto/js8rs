// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

//! Lightweight, synchronous JS8 framing and DSP primitives.
//!
//!
//! To transmit, build application text, then encode each frame
//! with the same submode:
//!
//! ```
//! use js8rs::codec::{BuildFramesOptions, build_frames};
//! use js8rs::protocol::{FrameFlags, Submode};
//!
//! let built = build_frames(&BuildFramesOptions::new("HELLO WORLD", Submode::Fast));
//! assert!(built.frames[0].flags.contains(FrameFlags::FIRST));
//! let encoded = built.encode()?;
//! assert_eq!(encoded[0].tones.len(), js8rs::TONES_PER_FRAME);
//! # Ok::<(), js8rs::codec::EncodeError>(())
//! ```

/// Frame construction, parsing, and tone encoding.
pub mod codec;
/// Typed parsing of application-level JS8 commands.
pub mod command;
mod decoder;
mod detector;
mod encoder;
pub(crate) mod internal;
mod modulator;
/// Wire-level protocol types and submode constants.
pub mod protocol;
/// Synchronous detection, decoding, and receive helpers.
pub mod rx;
mod submode;
/// JS8 transmit-slot calculations.
pub mod timing;
/// Audio modulation types.
pub mod tx;
mod varicode;

/// Number of tone symbols in an encoded JS8 frame.
pub use encoder::TONES_PER_FRAME;
/// Common protocol types.
pub use protocol::{DecodeModes, FrameFlags, FrameType, Js8Protocol, Submode};
