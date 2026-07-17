// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Extracted JS8 slot alignment and transmit gating into a Rust API.

//! Transmit-slot alignment and gating.

use crate::protocol::{Js8Protocol, Submode, SubmodeLookup};
use core::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Inputs that affect slot math and TX gating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TxTimingConfig {
    tx_delay: Duration,
    tr_period: Duration,
}

impl TxTimingConfig {
    /// Creates timing configuration from a TX delay and transceiver period.
    #[must_use]
    pub const fn new(tx_delay: Duration, tr_period: Duration) -> Self {
        Self {
            tx_delay,
            tr_period,
        }
    }

    /// Creates configuration whose transceiver period matches `submode`.
    #[must_use]
    pub fn for_submode(tx_delay: Duration, submode: Submode) -> Self {
        Self::new(
            tx_delay,
            Duration::from_secs(Js8Protocol::period_seconds(submode)),
        )
    }

    /// Returns the configured transmit delay.
    #[must_use]
    pub const fn tx_delay(self) -> Duration {
        self.tx_delay
    }

    /// Returns the configured transceiver period.
    #[must_use]
    pub const fn tr_period(self) -> Duration {
        self.tr_period
    }
}

/// Computed properties for the current slot.
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq)]
pub struct TransmitSlot {
    /// Submode whose slot properties were used.
    pub submode: Submode,
    /// Input Unix timestamp in milliseconds.
    pub now_unix_ms: u64,
    /// Slot period in seconds.
    pub period_seconds: u64,
    /// Slot period in milliseconds.
    pub period_ms: u64,
    /// Current UTC-aligned slot start in milliseconds.
    pub slot_start_ms: u64,
    /// Current UTC-aligned slot end in milliseconds.
    pub slot_end_ms: u64,
    /// Milliseconds elapsed in the current slot.
    pub ms_into_slot: u64,
    /// Seconds elapsed in the current slot.
    pub seconds_into_slot: f64,
    /// `seconds_into_slot` / `period_seconds`
    pub fraction_into_slot: f64,
    /// TX duration (seconds) at the beginning of the slot.
    pub tx_duration_seconds: f64,
    /// End of the transmit payload window in Unix milliseconds.
    pub tx_window_end_ms: u64,
    /// TX delay config (seconds).
    pub tx_delay_seconds: f64,
    /// Start of the configured TX-delay window in Unix milliseconds.
    pub tx_delay_window_start_ms: u64,
    /// Late-threshold fraction (0..1+) for starting TX during the slot.
    pub late_threshold_fraction: f64,
    /// Exclusive latest allowed start timestamp in Unix milliseconds.
    pub latest_start_ms_exclusive: u64,
}

impl fmt::Debug for TransmitSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimeSlot")
            .field("submode", &self.submode)
            .field("now_unix_ms", &self.now_unix_ms)
            .field("period_seconds", &self.period_seconds)
            .field("slot_start_ms", &self.slot_start_ms)
            .field("slot_end_ms", &self.slot_end_ms)
            .field("ms_into_slot", &self.ms_into_slot)
            .field("seconds_into_slot", &self.seconds_into_slot)
            .field("fraction_into_slot", &self.fraction_into_slot)
            .field("tx_duration_seconds", &self.tx_duration_seconds)
            .field("tx_window_end_ms", &self.tx_window_end_ms)
            .field("tx_delay_seconds", &self.tx_delay_seconds)
            .field("tx_delay_window_start_ms", &self.tx_delay_window_start_ms)
            .field("late_threshold_fraction", &self.late_threshold_fraction)
            .field("latest_start_ms_exclusive", &self.latest_start_ms_exclusive)
            .finish()
    }
}

/// Timing related params per mode.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimingParams {
    /// `JS8Call` `time_is_in_tx_delay`.
    pub in_tx_delay_window: bool,
    /// Equivalent to `(0 <= seconds_into_slot && seconds_into_slot < tx_duration)`.
    pub in_tx_payload_window: bool,
    /// Equivalent to `JS8Call` `m_timeToSend` excluding "tune":
    /// `in_tx_payload_window || in_tx_delay_window`.
    pub time_is_in_send_region: bool,
    /// Equivalent to `JS8Call` condition `(fraction < lateThreshold || in_tx_delay_window)`.
    pub allowed_to_start_now: bool,
    /// Convenience: start disallowed, past late threshold and not in tx-delay window.
    pub too_late_to_start: bool,
}

/// Compute a fully-populated `TimeSlot` from an absolute UTC-aligned timestamp.
///
/// `now_ms` should be based on UTC epoch milliseconds (or any absolute timebase aligned to UTC
/// second boundaries), since slot alignment is modulo `period_ms`.
#[must_use]
pub fn compute_slot(submode: Submode, now_unix_ms: u64, config: TxTimingConfig) -> TransmitSlot {
    compute_slot_with::<Js8Protocol>(submode, now_unix_ms, config)
}

/// Computes a transmit slot using a custom protocol lookup.
///
/// This is primarily an advanced integration and testing seam. Normal JS8
/// callers should use [`compute_slot`].
#[must_use]
pub fn compute_slot_with<L: SubmodeLookup>(
    submode: Submode,
    now_unix_ms: u64,
    config: TxTimingConfig,
) -> TransmitSlot {
    let period_seconds = L::period_seconds(submode);
    let period_ms = period_seconds.saturating_mul(1000);

    // Avoid division-by-zero behavior if caller supplies invalid lookup data.
    // (In practice, period_seconds should never be 0.)
    let safe_period_ms = if period_ms == 0 { 1 } else { period_ms };

    let ms_into_slot = now_unix_ms % safe_period_ms;
    let slot_start_ms = now_unix_ms - ms_into_slot;
    let slot_end_ms = slot_start_ms + safe_period_ms;

    let seconds_into_slot = (ms_into_slot as f64) / 1000.0;
    let fraction_into_slot = if period_seconds == 0 {
        0.0
    } else {
        seconds_into_slot / (period_seconds as f64)
    };

    let tx_duration_seconds = L::tx_duration(submode);
    let tx_window_end_ms = slot_start_ms.saturating_add((tx_duration_seconds * 1000.0) as u64);

    // tx-delay window starts at end-of-slot minus tx_delay.
    // Clamp negative/oversized delays to a sensible range.
    let tx_delay_seconds = config.tx_delay.as_secs_f64();
    let tx_delay_ms = (tx_delay_seconds * 1000.0) as u64;
    let tx_delay_window_start_ms = slot_end_ms.saturating_sub(tx_delay_ms);

    // JS8Call late threshold:
    // ratio - (txDelay / TRperiod), then scaled by submode.
    let configured_period_seconds = config.tr_period.as_secs_f64();
    let tr_period_seconds = if configured_period_seconds > 0.0 {
        configured_period_seconds
    } else {
        period_seconds as f64
    };

    let ratio = L::compute_ratio(submode);
    let mut late_threshold = ratio - (tx_delay_seconds / tr_period_seconds);
    late_threshold *= L::late_threshold_multiplier(submode);

    // Convert fraction threshold into an absolute ms boundary.
    // JS8Call uses strict `< lateThreshold`; so this is "exclusive".
    let latest_start_ms_exclusive = if safe_period_ms == 0 {
        slot_start_ms
    } else {
        let boundary = (late_threshold * (safe_period_ms as f64)).floor();
        slot_start_ms.saturating_add(boundary.max(0.0) as u64)
    };

    TransmitSlot {
        submode,
        now_unix_ms,
        period_seconds,
        period_ms: safe_period_ms,
        slot_start_ms,
        slot_end_ms,
        ms_into_slot,
        seconds_into_slot,
        fraction_into_slot,
        tx_duration_seconds,
        tx_window_end_ms,
        tx_delay_seconds,
        tx_delay_window_start_ms,
        late_threshold_fraction: late_threshold,
        latest_start_ms_exclusive,
    }
}

impl TransmitSlot {
    #[must_use]
    /// Derives boolean start and payload-window conditions.
    pub const fn params(&self) -> TimingParams {
        let in_tx_delay_window = self.now_unix_ms >= self.tx_delay_window_start_ms;

        let in_tx_payload_window =
            self.seconds_into_slot >= 0.0 && self.seconds_into_slot < self.tx_duration_seconds;

        let time_is_in_send_region = in_tx_payload_window || in_tx_delay_window;

        // Equivalent to: fraction < lateThreshold OR in_tx_delay_window
        let allowed_to_start_now =
            (self.fraction_into_slot < self.late_threshold_fraction) || in_tx_delay_window;

        let too_late_to_start = !allowed_to_start_now;

        TimingParams {
            in_tx_delay_window,
            in_tx_payload_window,
            time_is_in_send_region,
            allowed_to_start_now,
            too_late_to_start,
        }
    }

    /// Returns the current slot index (floor(now / `period_ms`)).
    #[must_use]
    pub const fn slot_index(&self) -> u64 {
        self.now_unix_ms / self.period_ms
    }

    /// Start timestamp of the next slot boundary (strictly greater than now unless `period_ms==0`).
    #[must_use]
    pub const fn next_slot_start_ms(&self) -> u64 {
        self.slot_end_ms
    }

    /// Timestamp for the start of the current slot (UTC-aligned).
    #[must_use]
    pub const fn current_slot_start_ms(&self) -> u64 {
        self.slot_start_ms
    }

    /// Timestamp when the TX payload window ends for this slot.
    #[must_use]
    pub const fn tx_payload_window_end_ms(&self) -> u64 {
        self.tx_window_end_ms
    }

    /// Timestamp when the tx-delay window begins for this slot.
    #[must_use]
    pub const fn tx_delay_window_start_ms(&self) -> u64 {
        self.tx_delay_window_start_ms
    }

    /// If it is currently a valid moment to *start* a normal (non-tune) TX in this slot,
    /// return `now_ms`, otherwise return the next slot start.
    #[must_use]
    pub const fn next_viable_start_ms(&self) -> u64 {
        if self.params().allowed_to_start_now && self.params().time_is_in_send_region {
            self.now_unix_ms
        } else {
            self.next_slot_start_ms()
        }
    }

    /// Equivalent to the `JS8Call`:
    ///
    /// `if (ptt_inactive && ((m_timeToSend && (fraction < lateThreshold || in_tx_delay) && msgLen>0) || tune)) { ... }`
    ///
    /// Here `tune` is handled by the caller, use this for normal messages.
    #[must_use]
    pub const fn should_start_tx_now(&self, ptt_active: bool, msg_len: usize) -> bool {
        if ptt_active {
            return false;
        }
        if msg_len == 0 {
            return false;
        }

        let a = self.params();
        a.time_is_in_send_region && a.allowed_to_start_now
    }
}

/// Returns the current Unix timestamp in milliseconds.
#[must_use]
pub fn unix_time_ms() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl SubmodeLookup for Fake {
        fn samples_for_one_symbol(_submode_id: Submode) -> f64 {
            0.0
        }
        fn tone_spacing(_submode_id: Submode) -> f64 {
            0.0
        }
        fn period_seconds(_submode_id: Submode) -> u64 {
            15
        }
        fn start_delay_ms(_submode_id: Submode) -> u64 {
            0
        }
        fn tx_duration(_submode_id: Submode) -> f64 {
            12.6
        }
        fn compute_ratio(_submode_id: Submode) -> f64 {
            1.0
        }

        fn late_threshold_multiplier(_submode_id: Submode) -> f64 {
            1.0
        }
    }

    #[test]
    fn computes_slot_boundaries() {
        let cfg = TxTimingConfig::new(Duration::from_secs(1), Duration::from_secs(15));
        let slot = compute_slot_with::<Fake>(Submode::Normal, 1_000, cfg);
        assert_eq!(slot.period_ms, 15_000);
        assert_eq!(slot.slot_start_ms, 0);
        assert_eq!(slot.slot_end_ms, 15_000);
        assert_eq!(slot.ms_into_slot, 1_000);
    }

    #[test]
    fn tx_delay_window_and_late_gate_behave_as_expected() {
        // period=15s, tx_duration=12.6s, tx_delay=2s, late threshold=(1 - 2/15) = 0.866...
        let cfg = TxTimingConfig::new(Duration::from_secs(2), Duration::from_secs(15));

        // 14.1s into slot: in tx-delay window, but outside payload window.
        let slot = compute_slot_with::<Fake>(Submode::Normal, 14_100, cfg);
        let p = slot.params();
        assert!(p.in_tx_delay_window);
        assert!(!p.in_tx_payload_window);
        assert!(p.time_is_in_send_region);
        assert!(p.allowed_to_start_now);
    }

    #[test]
    fn should_start_tx_now_respects_ptt_and_message_len() {
        let cfg = TxTimingConfig::new(Duration::from_secs(1), Duration::from_secs(15));
        let slot = compute_slot_with::<Fake>(Submode::Normal, 1_000, cfg);

        assert!(slot.should_start_tx_now(false, 3));
        assert!(!slot.should_start_tx_now(true, 3));
        assert!(!slot.should_start_tx_now(false, 0));
    }

    #[test]
    fn next_viable_start_moves_to_next_slot_when_too_late() {
        let cfg = TxTimingConfig::new(Duration::ZERO, Duration::from_secs(15));
        // ratio=1.0 in Fake, so late threshold is 1.0 and this should still be viable.
        let slot_ok = compute_slot_with::<Fake>(Submode::Normal, 14_000, cfg);
        assert_eq!(slot_ok.next_viable_start_ms(), slot_ok.slot_end_ms);

        struct Tight;
        impl SubmodeLookup for Tight {
            fn samples_for_one_symbol(_submode_id: Submode) -> f64 {
                0.0
            }
            fn tone_spacing(_submode_id: Submode) -> f64 {
                0.0
            }
            fn period_seconds(_submode_id: Submode) -> u64 {
                10
            }
            fn start_delay_ms(_submode_id: Submode) -> u64 {
                0
            }
            fn tx_duration(_submode_id: Submode) -> f64 {
                4.0
            }
            fn compute_ratio(_submode_id: Submode) -> f64 {
                0.4
            }
            fn late_threshold_multiplier(_submode_id: Submode) -> f64 {
                1.0
            }
        }

        let late_cfg = TxTimingConfig::new(Duration::ZERO, Duration::from_secs(10));
        let late_slot = compute_slot_with::<Tight>(Submode::Fast, 6_000, late_cfg); // 60% into slot
        assert!(late_slot.params().too_late_to_start);
        assert_eq!(late_slot.next_viable_start_ms(), late_slot.slot_end_ms);
    }
}
