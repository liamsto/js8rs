//! Helpers for selecting decoder sample windows.

use crate::protocol::{
    Submode, compute_cycle_for_decode, submode_samples_needed, submode_samples_per_period,
};

/// Per-submode cursor used to avoid duplicate decode windows.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecodeCursor {
    /// Start of the current candidate cycle.
    pub current_start: Option<usize>,
    /// Start of the following cycle.
    pub next_start: Option<usize>,
}

impl DecodeCursor {
    #[must_use]
    /// Creates an uninitialized cursor.
    pub const fn new() -> Self {
        Self {
            current_start: None,
            next_start: None,
        }
    }
}

/// A sample range ready for one decoder pass.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeWindow {
    /// Receive cycle number.
    pub cycle: usize,
    /// First sample in the decoder ring.
    pub start: usize,
    /// Number of samples to decode.
    pub size: usize,
}

/// Returns the next ready window and advances `cursor`.
pub fn next_decode_window(
    submode: Submode,
    k: usize,
    k0: usize,
    cursor: &mut DecodeCursor,
) -> Option<DecodeWindow> {
    let cycle_frames = submode_samples_per_period(submode) as usize;
    let frames_needed = submode_samples_needed(submode) as usize;
    let current_cycle = compute_cycle_for_decode(submode, k);
    let delta = k.abs_diff(k0);

    if cycle_frames == 0 {
        return None;
    }

    let current_start = cursor.current_start.unwrap_or(0);
    let dead_air = k < current_start
        && k < current_start
            .saturating_sub(cycle_frames)
            .saturating_add(frames_needed);

    if dead_air
        || k < k0
        || delta > cycle_frames
        || cursor.current_start.is_none()
        || cursor.next_start.is_none()
    {
        let start = current_cycle * cycle_frames;
        cursor.current_start = Some(start);
        cursor.next_start = Some(start + cycle_frames);
    }

    let current_start = cursor.current_start.unwrap_or(current_start);
    let next_start = cursor.next_start.unwrap_or(current_start + cycle_frames);
    let ready = current_start + frames_needed <= k;
    if !ready {
        return None;
    }

    let window = DecodeWindow {
        cycle: current_cycle,
        start: current_start,
        size: frames_needed.max(k - current_start),
    };

    cursor.current_start = Some(next_start);
    cursor.next_start = Some(next_start + cycle_frames);

    Some(window)
}

/// Independent decode cursors for every supported submode.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DecodeScheduler {
    /// Normal-mode cursor.
    pub normal: DecodeCursor,
    /// Fast-mode cursor.
    pub fast: DecodeCursor,
    /// Turbo-mode cursor.
    pub turbo: DecodeCursor,
    /// Slow-mode cursor.
    pub slow: DecodeCursor,
    /// Ultra-mode cursor.
    pub ultra: DecodeCursor,
}

impl DecodeScheduler {
    #[must_use]
    /// Creates a scheduler with empty cursors.
    pub const fn new() -> Self {
        Self {
            normal: DecodeCursor::new(),
            fast: DecodeCursor::new(),
            turbo: DecodeCursor::new(),
            slow: DecodeCursor::new(),
            ultra: DecodeCursor::new(),
        }
    }

    /// Returns the cursor for `submode`.
    pub const fn cursor_mut(&mut self, submode: Submode) -> &mut DecodeCursor {
        match submode {
            Submode::Normal => &mut self.normal,
            Submode::Fast => &mut self.fast,
            Submode::Turbo => &mut self.turbo,
            Submode::Slow => &mut self.slow,
            Submode::Ultra => &mut self.ultra,
        }
    }

    /// Returns and records the next ready window for `submode`.
    pub fn next_window(&mut self, submode: Submode, k: usize, k0: usize) -> Option<DecodeWindow> {
        next_decode_window(submode, k, k0, self.cursor_mut(submode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{
        compute_cycle_for_decode, submode_samples_needed, submode_samples_per_period,
    };

    #[test]
    fn initializes_cursor_and_waits_until_window_is_ready() {
        let mut cursor = DecodeCursor::new();
        let k = 10_000;
        let got = next_decode_window(Submode::Normal, k, 0, &mut cursor);
        assert!(got.is_none());
        assert!(cursor.current_start.is_some());
        assert!(cursor.next_start > cursor.current_start);
    }

    #[test]
    fn emits_window_with_expected_cycle_start_and_size() {
        let mut cursor = DecodeCursor::new();
        let period = submode_samples_per_period(Submode::Fast) as usize;
        let need = submode_samples_needed(Submode::Fast) as usize;
        let k = period + need + 500;
        let window = next_decode_window(Submode::Fast, k, 0, &mut cursor).expect("window ready");

        assert_eq!(window.cycle, compute_cycle_for_decode(Submode::Fast, k));
        assert_eq!(window.start, period);
        assert_eq!(window.size, need.max(k - period));
    }

    #[test]
    fn rewinds_when_k_is_behind_k0() {
        let mut cursor = DecodeCursor {
            current_start: Some(120_000),
            next_start: Some(180_000),
        };
        let _ = next_decode_window(Submode::Normal, 50_000, 120_000, &mut cursor);
        assert!(cursor.current_start.unwrap_or(usize::MAX) <= 50_000);
    }

    #[test]
    fn rewinds_when_delta_exceeds_cycle_frames() {
        let mut cursor = DecodeCursor {
            current_start: Some(0),
            next_start: Some(1),
        };
        let period = submode_samples_per_period(Submode::Turbo) as usize;
        let _ = next_decode_window(Submode::Turbo, period * 3, 0, &mut cursor);
        assert_eq!(
            cursor.current_start,
            Some(
                compute_cycle_for_decode(Submode::Turbo, period * 3)
                    * submode_samples_per_period(Submode::Turbo) as usize
            )
        );
    }

    #[test]
    fn scheduler_maintains_independent_cursors_per_submode() {
        let mut scheduler = DecodeScheduler::new();

        let n = scheduler.next_window(Submode::Normal, 710_000, 0);
        let f = scheduler.next_window(Submode::Fast, 710_000, 0);

        assert!(n.is_some());
        assert!(f.is_some());
        assert_ne!(n.unwrap().start, f.unwrap().start);
    }
}
