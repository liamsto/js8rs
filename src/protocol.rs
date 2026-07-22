// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported JS8 protocol flags and submode logic to Rust.

//! JS8 wire types, flag sets, and submode properties.

use core::{
    fmt,
    ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign},
};

/// A JS8 transmission submode.
#[repr(i32)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Default)]
pub enum Submode {
    /// Standard transmission, 15-second slots.
    #[default]
    Normal = 0,
    /// Faster transmission, 10-second slots.
    Fast = 1,
    /// High-speed transmission, 6-second slots.
    Turbo = 2,
    /// Slower but more reliable transmission, 30-second slots.
    Slow = 4,
    /// Highest-speed transmission, 4-second slots.
    Ultra = 8,
}

/// Semantic type of a decoded JS8 frame.
#[repr(u8)]
#[non_exhaustive]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum FrameType {
    /// The frame could not be classified.
    FrameUnknown = 255,
    /// Heartbeat or CQ frame.
    FrameHeartbeat = 0,
    /// Compound callsign frame.
    FrameCompound = 1,
    /// Directed compound-callsign frame.
    FrameCompoundDirected = 2,
    /// Directed message frame.
    FrameDirected = 3,
    /// Data frame.
    FrameData = 4,
    /// Compressed data frame.
    FrameDataCompressed = 6,
}

impl TryFrom<u8> for FrameType {
    type Error = FrameTypeParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            255 => Ok(Self::FrameUnknown),
            0 => Ok(Self::FrameHeartbeat),
            1 => Ok(Self::FrameCompound),
            2 => Ok(Self::FrameCompoundDirected),
            3 => Ok(Self::FrameDirected),
            4 => Ok(Self::FrameData),
            6 => Ok(Self::FrameDataCompressed),
            _ => Err(FrameTypeParseError { value }),
        }
    }
}

impl fmt::Display for FrameType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameUnknown => f.write_str("Unknown"),
            Self::FrameHeartbeat => f.write_str("Heartbeat"),
            Self::FrameCompound => f.write_str("Compound"),
            Self::FrameCompoundDirected => f.write_str("CompoundDirected"),
            Self::FrameDirected => f.write_str("Directed"),
            Self::FrameData => f.write_str("Data"),
            Self::FrameDataCompressed => f.write_str("DataCompressed"),
        }
    }
}

/// Error returned when a raw frame type is unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameTypeParseError {
    /// Invalid raw frame type.
    pub value: u8,
}

impl fmt::Display for FrameTypeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JS8 frame type {}", self.value)
    }
}

impl std::error::Error for FrameTypeParseError {}

/// Three-bit transmission flags carried by a JS8 frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FrameFlags(u8);

impl FrameFlags {
    /// No flags, used for intermediate frames.
    pub const NONE: Self = Self(0);
    /// First frame of a message.
    pub const FIRST: Self = Self(1 << 0);
    /// Last frame of a message.
    pub const LAST: Self = Self(1 << 1);
    /// Data frame with no frame-type header.
    pub const DATA: Self = Self(1 << 2);
    /// All frame flags set.
    pub const ALL: Self = Self(Self::FIRST.0 | Self::LAST.0 | Self::DATA.0);

    /// Returns the raw three-bit value.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Creates flags if all raw bits are recognized.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Option<Self> {
        if bits & !Self::ALL.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    /// Creates flags after discarding unknown bits.
    #[must_use]
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }

    /// Returns whether every flag in `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    /// Returns whether any flag in `other` is present.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Returns whether no flags are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl From<FrameFlags> for u8 {
    fn from(value: FrameFlags) -> Self {
        value.bits()
    }
}

impl BitOr for FrameFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for FrameFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for FrameFlags {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for FrameFlags {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

/// Error returned when an integer is not a defined [`Submode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmodeParseError {
    /// Invalid raw submode value.
    pub value: i32,
}

impl fmt::Display for SubmodeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JS8 submode {}", self.value)
    }
}

impl std::error::Error for SubmodeParseError {}

impl TryFrom<i32> for Submode {
    type Error = SubmodeParseError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Normal),
            1 => Ok(Self::Fast),
            2 => Ok(Self::Turbo),
            4 => Ok(Self::Slow),
            8 => Ok(Self::Ultra),
            _ => Err(SubmodeParseError { value }),
        }
    }
}

/// Set of submodes selected for a decoder pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DecodeModes(u8);

impl DecodeModes {
    /// No submodes.
    pub const NONE: Self = Self(0);
    /// Normal submode.
    pub const NORMAL: Self = Self(1 << 0);
    /// Fast submode.
    pub const FAST: Self = Self(1 << 1);
    /// Turbo submode.
    pub const TURBO: Self = Self(1 << 2);
    /// Slow submode.
    pub const SLOW: Self = Self(1 << 3);
    /// Ultra submode.
    pub const ULTRA: Self = Self(1 << 4);
    /// Every supported submode.
    pub const ALL: Self =
        Self(Self::NORMAL.0 | Self::FAST.0 | Self::TURBO.0 | Self::SLOW.0 | Self::ULTRA.0);

    #[inline]
    #[must_use]
    /// Returns the raw mode-set bits.
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[inline]
    #[must_use]
    /// Returns whether every mode in `other` is selected.
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    #[must_use]
    /// Builds a mode set after discarding unknown bits.
    pub const fn from_bits_truncate(bits: u8) -> Self {
        Self(bits & Self::ALL.0)
    }
}

impl Default for DecodeModes {
    fn default() -> Self {
        Self::ALL
    }
}

impl BitOr for DecodeModes {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for DecodeModes {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl From<Submode> for DecodeModes {
    fn from(value: Submode) -> Self {
        match value {
            Submode::Normal => Self::NORMAL,
            Submode::Fast => Self::FAST,
            Submode::Turbo => Self::TURBO,
            Submode::Slow => Self::SLOW,
            Submode::Ultra => Self::ULTRA,
        }
    }
}

/// Decoder sample rate in hertz.
pub const RX_SAMPLE_RATE_HZ: u64 = crate::internal::commons::JS8_RX_SAMPLE_RATE;
/// Fixed decoder length in number of samples.
pub const RX_SAMPLE_SIZE: usize = crate::internal::commons::JS8_RX_SAMPLE_SIZE;
/// Normal mode duration in seconds.
pub const NORMAL_TX_SECONDS: u64 = crate::internal::commons::JS8A_TX_SECONDS;
/// Fast mode duration in seconds.
pub const FAST_TX_SECONDS: u64 = crate::internal::commons::JS8B_TX_SECONDS;
/// Turbo mode duration in seconds.
pub const TURBO_TX_SECONDS: u64 = crate::internal::commons::JS8C_TX_SECONDS;
/// Slow mode duration in seconds.
pub const SLOW_TX_SECONDS: u64 = crate::internal::commons::JS8E_TX_SECONDS;
/// Ultra mode duration in seconds.
pub const ULTRA_TX_SECONDS: u64 = crate::internal::commons::JS8I_TX_SECONDS;

#[inline]
#[must_use]
/// Packs hour, minute, and second into the decoder's `HHMMSS` integer form.
pub const fn code_time(hour: u8, minute: u8, second: u8) -> u32 {
    (hour as u32) * 10000 + (minute as u32) * 100 + (second as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_modes_bit_ops_and_contains_work() {
        let mut modes = DecodeModes::NONE;
        modes |= DecodeModes::FAST;
        modes |= DecodeModes::SLOW;

        assert!(modes.contains(DecodeModes::FAST));
        assert!(modes.contains(DecodeModes::SLOW));
        assert!(!modes.contains(DecodeModes::NORMAL));

        let masked = DecodeModes::from_bits_truncate(0b1111_1111);
        assert_eq!(masked.bits(), DecodeModes::ALL.bits());
    }

    #[test]
    fn submode_conversion_and_decode_lengths_are_valid() {
        assert_eq!(Submode::try_from(0).unwrap(), Submode::Normal);
        assert_eq!(Submode::try_from(1).unwrap(), Submode::Fast);
        assert_eq!(Submode::try_from(3), Err(SubmodeParseError { value: 3 }));

        assert_eq!(Submode::Normal.samples_per_period(), 180_000);
        assert_eq!(Submode::Fast.samples_per_period(), 120_000);
        assert_eq!(Submode::Turbo.samples_per_period(), 72_000);
        assert_eq!(Submode::Slow.samples_per_period(), 360_000);
        assert_eq!(Submode::Ultra.samples_per_period(), 48_000);
    }

    #[test]
    fn code_time_packs_hms_as_expected() {
        assert_eq!(code_time(12, 34, 56), 123_456);
        assert_eq!(code_time(0, 0, 7), 7);
    }
}
