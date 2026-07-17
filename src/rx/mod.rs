//! Synchronous receive-side detection and decoding.

use crate::codec::DecodedFrame;
use crate::protocol::{DecodeModes, Submode, decode_nmax_frames};

/// Buffered directed-message reassembly.
pub mod reassembly;
/// Decode-window scheduling helpers.
pub mod scheduling;

pub use crate::detector::{
    Detector, INPUT_SAMPLE_RATE_HZ, InputFormat, NDOWN as DETECTOR_DECIMATION,
    OUTPUT_SAMPLE_RATE_HZ, WriteStats,
};
pub use reassembly::{
    BufferKey, BufferedChecksum, BufferedCommandResult, MessageBufferAssembler, ReassemblyEvent,
};
pub use scheduling::{DecodeCursor, DecodeScheduler, DecodeWindow, next_decode_window};

/// Fixed decoder ring-buffer size (`d2` samples at 12 kHz).
pub const SAMPLE_BUFFER_SIZE: usize = crate::internal::commons::JS8_RX_SAMPLE_SIZE;

/// Event emitted during one decoder pass.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A decode pass has started.
    DecodeStarted(DecodeStarted),
    /// A submode search window is starting.
    SyncStart(SyncStart),
    /// Synchronization candidate or successful sync information.
    SyncState(SyncState),
    /// A frame was decoded.
    Decoded(Decoded),
    /// The decode pass has finished.
    DecodeFinished(DecodeFinished),
}

/// Metadata from the beginning of a decode pass.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeStarted {
    /// Modes searched during the pass.
    pub modes: DecodeModes,
}

/// Sample window to be searched for synchronization.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SyncStart {
    /// First sample in the search window.
    pub sample_position: usize,
    /// Number of samples in the search window.
    pub sample_count: usize,
}

/// Synchronization details for a candidate or decoded frame.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SyncState {
    /// Kind of synchronization update.
    pub kind: SyncStateType,
    /// Candidate submode.
    pub submode: Submode,
    /// Candidate audio frequency in hertz.
    pub frequency_hz: f32,
    /// Candidate timing offset in seconds.
    pub time_offset_seconds: f32,
    /// Synchronization metric.
    pub metric: SyncMetric,
}

/// Stage represented by a [`SyncState`] event.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SyncStateType {
    /// A possible frame was found.
    Candidate,
    /// A frame decoded successfully.
    Decoded,
}

/// Metric attached to a synchronization event.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SyncMetric {
    /// Integer sync score for a candidate.
    Candidate(u32),
    /// Decoder quality for a completed frame.
    Decoded(f32),
}

/// A parsed frame and its receiver-specific signal metadata.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    /// Parsed protocol frame.
    pub frame: DecodedFrame,
    /// Packed UTC time in `HHMMSS` form.
    pub utc: u32,
    /// Signal-to-noise ratio in decibels.
    pub snr: i32,
    /// Timing offset in seconds.
    pub time_offset_seconds: f32,
    /// Audio frequency in hertz.
    pub frequency_hz: f32,
    /// Decoder quality score.
    pub quality: f32,
}

impl Decoded {
    /// Creates received-frame metadata around an already parsed frame.
    #[must_use]
    pub const fn new(
        frame: DecodedFrame,
        utc: u32,
        snr: i32,
        time_offset_seconds: f32,
        frequency_hz: f32,
        quality: f32,
    ) -> Self {
        Self {
            frame,
            utc,
            snr,
            time_offset_seconds,
            frequency_hz,
            quality,
        }
    }

    pub(crate) fn from_raw(
        encoded: String,
        flags: crate::protocol::FrameFlags,
        submode: Submode,
        utc: u32,
        snr: i32,
        time_offset_seconds: f32,
        frequency_hz: f32,
        quality: f32,
    ) -> Self {
        Self::new(
            crate::codec::parse_frame(&encoded, flags, submode),
            utc,
            snr,
            time_offset_seconds,
            frequency_hz,
            quality,
        )
    }

    /// Returns whether the decoder quality is below the normal confidence threshold.
    #[must_use]
    pub fn is_low_confidence(&self) -> bool {
        self.quality < 0.17
    }

    /// Formats this decode in the JS8Call `ALL.TXT` format.
    #[must_use]
    pub fn log_line(&self) -> String {
        self.frame.log_line(
            self.utc,
            self.snr,
            self.time_offset_seconds,
            self.frequency_hz.max(0.0) as u32,
        )
    }
}

/// Number of frames decoded in a completed pass.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct DecodeFinished {
    /// Successfully decoded frame count.
    pub decoded: usize,
}

impl std::fmt::Display for SyncStateType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Candidate => f.write_str("CANDIDATE"),
            Self::Decoded => f.write_str("DECODED"),
        }
    }
}

#[inline]
#[must_use]
/// Computes a linear decode window ending at `kin_end` and capped at `nmax`.
pub const fn window_from_kin(kin_end: usize, nmax: usize) -> (usize, usize) {
    let span = if kin_end < nmax { kin_end } else { nmax };
    let start = kin_end.saturating_sub(span);
    (start, span)
}

/// User-controlled settings for a decoder pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeConfig {
    utc: u32,
    nominal_frequency_hz: u32,
    min_frequency_hz: u32,
    max_frequency_hz: u32,
    emit_sync: bool,
    modes: DecodeModes,
}

impl Default for DecodeConfig {
    fn default() -> Self {
        Self {
            utc: 0,
            nominal_frequency_hz: 0,
            min_frequency_hz: 0,
            max_frequency_hz: 5_000,
            emit_sync: false,
            modes: DecodeModes::ALL,
        }
    }
}

impl DecodeConfig {
    /// Restricts decoding to the selected modes.
    #[must_use]
    pub const fn with_modes(mut self, modes: DecodeModes) -> Self {
        self.modes = modes;
        self
    }

    /// Sets the packed UTC time included in decoded events.
    #[must_use]
    pub const fn with_utc(mut self, utc: u32) -> Self {
        self.utc = utc;
        self
    }

    /// Sets the nominal receive frequency in hertz.
    #[must_use]
    pub const fn with_nominal_frequency(mut self, frequency_hz: u32) -> Self {
        self.nominal_frequency_hz = frequency_hz;
        self
    }

    /// Sets the inclusive decoder frequency search range in hertz.
    #[must_use]
    pub const fn with_frequency_range(mut self, min_hz: u32, max_hz: u32) -> Self {
        self.min_frequency_hz = min_hz;
        self.max_frequency_hz = max_hz;
        self
    }

    /// Controls emission of synchronization events.
    #[must_use]
    pub const fn with_sync_events(mut self, emit: bool) -> Self {
        self.emit_sync = emit;
        self
    }

    fn legacy(self, valid_samples: usize) -> crate::internal::commons::DecodeParams {
        let mut params = crate::internal::commons::DecodeParams {
            nutc: self.utc,
            nfqso: self.nominal_frequency_hz,
            nfa: self.min_frequency_hz,
            nfb: self.max_frequency_hz,
            sync_stats: self.emit_sync,
            nsubmodes: self.modes.bits(),
            ..crate::internal::commons::DecodeParams::default()
        };

        let set_window = |submode, position: &mut usize, size: &mut usize| {
            (*position, *size) = window_from_kin(valid_samples, decode_nmax_frames(submode));
        };
        if self.modes.contains(DecodeModes::NORMAL) {
            set_window(Submode::Normal, &mut params.kpos_a, &mut params.ksz_a);
        }
        if self.modes.contains(DecodeModes::FAST) {
            set_window(Submode::Fast, &mut params.kpos_b, &mut params.ksz_b);
        }
        if self.modes.contains(DecodeModes::TURBO) {
            set_window(Submode::Turbo, &mut params.kpos_c, &mut params.ksz_c);
        }
        if self.modes.contains(DecodeModes::SLOW) {
            set_window(Submode::Slow, &mut params.kpos_e, &mut params.ksz_e);
        }
        if self.modes.contains(DecodeModes::ULTRA) {
            set_window(Submode::Ultra, &mut params.kpos_i, &mut params.ksz_i);
        }
        params
    }
}

/// JS8 decoder.
pub struct Decoder {
    core: crate::decoder::DecoderCore,
}

impl Decoder {
    #[must_use]
    /// Creates a decoder prepared for every supported submode.
    pub fn new() -> Self {
        Self::with_modes(DecodeModes::ALL)
    }

    /// Prepares only the requested modes, reducing memory and startup work.
    /// A mode requested later is initialized on its first decode pass.
    #[must_use]
    pub fn with_modes(modes: DecodeModes) -> Self {
        Self {
            core: crate::decoder::DecoderCore::with_modes(modes),
        }
    }

    /// Decodes one pass over `valid_samples` from the provided sample buffer.
    ///
    /// The decoder performs work on the calling thread. It can be moved to a
    /// caller-managed worker thread when background decoding is required.
    pub fn decode<E>(
        &mut self,
        samples: &[i16],
        valid_samples: usize,
        config: &DecodeConfig,
        mut emit: E,
    ) -> usize
    where
        E: FnMut(Event),
    {
        let valid_samples = valid_samples.min(samples.len());
        let mut data = crate::internal::commons::DecData {
            d2: samples,
            params: config.legacy(valid_samples),
        };

        self.core.decode_pass(&mut data, &mut emit)
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_from_kin_clamps_span_to_nmax() {
        assert_eq!(window_from_kin(1_000, 2_000), (0, 1_000));
        assert_eq!(window_from_kin(3_000, 2_000), (1_000, 2_000));
    }

    #[test]
    fn config_builds_private_windows_for_selected_modes() {
        let params = DecodeConfig::default()
            .with_modes(DecodeModes::TURBO)
            .legacy(100_000);
        assert_eq!(params.kpos_c, 28_000);
        assert_eq!(params.ksz_c, 72_000);
        assert_eq!(params.kpos_a, 0);
        assert_eq!(params.kpos_b, 0);
        assert_eq!(params.kpos_e, 0);
        assert_eq!(params.kpos_i, 0);
    }
}
