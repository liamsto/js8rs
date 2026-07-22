// SPDX-License-Identifier: GPL-3.0-only
//
// Copyright (C) 2018 Jordan Sherer <kn4crd@gmail.com>
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>
//
// Ported the detector to Rust and added no alloc typed input paths.

use std::sync::Mutex;

use crate::{internal::commons::JS8_RX_SAMPLE_SIZE, timing::unix_time_ms};

/// Input sample rate expected by the detector
pub const INPUT_SAMPLE_RATE_HZ: u32 = 48_000;
/// Output sample rate written into `d2`
pub const OUTPUT_SAMPLE_RATE_HZ: u32 = 12_000;
/// Decimation factor (48 kHz -> 12 kHz).
pub const NDOWN: usize = 4;

// FIR lowpass coefficients
// (ScopeFIR, Ntaps=49, fs=48k, fc=4500, fstop=6000, ripple=1dB, stop=40dB, fout=12k)
const NTAPS: usize = 49;
const LOWPASS: [f32; NTAPS] = [
    0.000_861_074_f32,
    0.010_051_92_f32,
    0.010_161_984_f32,
    0.011_363_155_f32,
    0.008_706_594_f32,
    0.002_613_872_8_f32,
    -0.005_202_883_f32,
    -0.011_720_749_f32,
    -0.013_752_163_f32,
    -0.009_431_602_f32,
    0.000_539_063_9_f32,
    0.012_636_767_f32,
    0.021_494_66_f32,
    0.021_951_236_f32,
    0.011_564_169_f32,
    -0.007_656_47_f32,
    -0.028_965_788_f32,
    -0.042_637_873_f32,
    -0.039_203_31_f32,
    -0.013_153_302_f32,
    0.034_320_768_f32,
    0.094_717_83_f32,
    0.154_224_6_f32,
    0.197_758_33_f32,
    0.213_715_14_f32,
    0.197_758_33_f32,
    0.154_224_6_f32,
    0.094_717_83_f32,
    0.034_320_768_f32,
    -0.013_153_302_f32,
    -0.039_203_31_f32,
    -0.042_637_873_f32,
    -0.028_965_788_f32,
    -0.007_656_47_f32,
    0.011_564_169_f32,
    0.021_951_236_f32,
    0.021_494_66_f32,
    0.012_636_767_f32,
    0.000_539_063_9_f32,
    -0.009_431_602_f32,
    -0.013_752_163_f32,
    -0.011_720_749_f32,
    -0.005_202_883_f32,
    0.002_613_872_8_f32,
    0.008_706_594_f32,
    0.011_363_155_f32,
    0.010_161_984_f32,
    0.010_051_92_f32,
    0.000_861_074_f32,
];

/// How incoming i16 samples are interpreted.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub enum InputFormat {
    /// Mono i16 at 48 kHz.
    Mono,
    /// Stereo interleaved i16 at 48 kHz; use left channel.
    StereoLeft,
    /// Stereo interleaved i16 at 48 kHz; average L+R (truncating).
    StereoAverage,
}

/// Summary of one detector input write.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct WriteStats {
    /// Input frames provided (frames, not samples).
    pub frames_in: usize,
    /// Input frames accepted (others are dropped).
    pub frames_accepted: usize,
    /// Input frames dropped because the 12 kHz ring buffer is full until next period.
    pub frames_dropped: usize,
    /// Number of 12 kHz blocks written (each block = `samples_per_fft` output samples).
    pub blocks_written: usize,
    /// `kin` after the write.
    pub kin_after: usize,
    /// Current second within the configured period.
    pub second_in_period: u64,
}

/// Live detector that:
/// - accepts 48 kHz PCM,
/// - lowpass+decimates to 12 kHz,
/// - fills `d2` (length `JS8_RX_SAMPLE_SIZE`) and advances `kin`,
/// - resets `kin` to 0 on period wrap (like original).
pub struct Detector {
    period_s: u64,
    inner: Mutex<Inner>,
}

struct Inner {
    samples_per_fft: usize,
    d2: Vec<i16>, // 12 kHz ring buffer (actually linear within a period, reset to 0 on wrap)
    kin: usize,   // number of 12 kHz samples written so far in current period
    buffer_pos: usize, // position in the 48 kHz staging buffer
    ns: u64,      // last secondInPeriod observed
    buffer: Vec<i16>, // 48 kHz mono staging buffer, length = samples_per_fft * NDOWN
    filter: FirDecimator49x4,
}

impl Detector {
    /// `period_s` should match JS8 “minute” period.
    /// `samples_per_fft` is the detector block size (the C++ `setBlockSize`).
    #[must_use]
    pub fn new(period_s: u64, samples_per_fft: usize) -> Self {
        let mut inner = Inner {
            samples_per_fft,
            d2: vec![0i16; JS8_RX_SAMPLE_SIZE],
            kin: 0,
            buffer_pos: 0,
            ns: second_in_period(period_s),
            buffer: vec![0i16; samples_per_fft * NDOWN],
            filter: FirDecimator49x4::new(LOWPASS),
        };

        // Match C++ ctor behavior: clear() calls resetBufferPosition + resetBufferContent.
        reset_buffer_position_locked(period_s, &mut inner);
        reset_buffer_content_locked(&mut inner);

        Self {
            period_s,
            inner: Mutex::new(inner),
        }
    }

    /// Changes the internal 48 kHz staging block size.
    pub fn set_block_size(&self, n: usize) {
        let mut g = self.inner.lock().unwrap();
        g.samples_per_fft = n;
        g.buffer_pos = 0;
        g.buffer.resize(n * NDOWN, 0);
    }

    /// Equivalent to C++ `clear()` when ring-buffer path is enabled:
    /// - align `kin` to wall clock within the period,
    /// - rotate existing content to preserve time alignment,
    /// - then zero-fill.
    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap();
        reset_buffer_position_locked(self.period_s, &mut g);
        reset_buffer_content_locked(&mut g);
    }

    /// Align `kin` to “now” inside the period and rotate `d2` to preserve alignment.
    pub fn reset_buffer_position(&self) {
        let mut g = self.inner.lock().unwrap();
        reset_buffer_position_locked(self.period_s, &mut g);
    }

    /// Zero-fill `d2`.
    pub fn reset_buffer_content(&self) {
        let mut g = self.inner.lock().unwrap();
        reset_buffer_content_locked(&mut g);
    }

    /// Current `kin` (12 kHz samples written in this period).
    pub fn kin(&self) -> usize {
        self.inner.lock().unwrap().kin
    }

    /// Borrow the decoder samples without copying.
    ///
    /// The detector lock is held for the duration of `f`, so this is intended
    /// for synchronous decoding when input writes can be paused.
    pub fn with_samples<R>(&self, f: impl FnOnce(&[i16], usize) -> R) -> R {
        let g = self.inner.lock().unwrap();
        f(&g.d2, g.kin)
    }

    /// Copy the decoder samples into caller-owned storage.
    ///
    /// `out` must contain exactly [`crate::rx::SAMPLE_BUFFER_SIZE`] samples.
    /// The returned value is the current `kin`.
    pub fn copy_samples(&self, out: &mut [i16]) -> usize {
        let g = self.inner.lock().unwrap();
        out.copy_from_slice(&g.d2);
        g.kin
    }

    /// Feed PCM into detector.
    ///
    /// `data` is i16 samples:
    /// - Mono: length is number of frames
    /// - Stereo*: length is 2 * number of frames (interleaved L,R)
    pub fn write_i16(&self, data: &[i16], fmt: InputFormat) -> WriteStats {
        let mut g = self.inner.lock().unwrap();

        let ns = second_in_period(self.period_s);
        if ns < g.ns {
            g.kin = 0;
            g.buffer_pos = 0;
        }
        g.ns = ns;

        let (frames_in, chan_stride) = match fmt {
            InputFormat::Mono => (data.len(), 1usize),
            InputFormat::StereoLeft | InputFormat::StereoAverage => (data.len() / 2, 2usize),
        };

        let remaining_12k = JS8_RX_SAMPLE_SIZE.saturating_sub(g.kin);
        let frames_acceptable = remaining_12k * NDOWN;

        let frames_accepted = frames_in.min(frames_acceptable);
        let frames_dropped = frames_in - frames_accepted;

        let mut blocks_written = 0usize;

        let mut remaining = frames_accepted;
        while remaining != 0 {
            let cap = g.samples_per_fft * NDOWN - g.buffer_pos;
            let num = cap.min(remaining);

            let start_frame = frames_accepted - remaining;

            let buffer_pos = g.buffer_pos;
            match fmt {
                InputFormat::Mono => {
                    g.buffer[buffer_pos..buffer_pos + num]
                        .copy_from_slice(&data[start_frame..start_frame + num]);
                }
                InputFormat::StereoLeft => {
                    for j in 0..num {
                        g.buffer[buffer_pos + j] = data[(start_frame + j) * chan_stride];
                    }
                }
                InputFormat::StereoAverage => {
                    for j in 0..num {
                        let l = i32::from(data[(start_frame + j) * chan_stride]);
                        let r = i32::from(data[(start_frame + j) * chan_stride + 1]);
                        g.buffer[buffer_pos + j] = i32::midpoint(l, r) as i16;
                    }
                }
            }

            g.buffer_pos += num;

            if g.buffer_pos == g.samples_per_fft * NDOWN {
                if g.kin < JS8_RX_SAMPLE_SIZE.saturating_sub(g.samples_per_fft) {
                    for i in 0..g.samples_per_fft {
                        let base = i * NDOWN;
                        let group = [
                            g.buffer[base],
                            g.buffer[base + 1],
                            g.buffer[base + 2],
                            g.buffer[base + 3],
                        ];
                        let out = g.filter.down_sample_i16(group);
                        let k = g.kin;
                        g.d2[k] = out;
                        g.kin += 1;
                    }
                }

                blocks_written += 1;
                g.buffer_pos = 0;
            }

            remaining -= num;
        }

        WriteStats {
            frames_in,
            frames_accepted,
            frames_dropped,
            blocks_written,
            kin_after: g.kin,
            second_in_period: ns,
        }
    }
}

fn reset_buffer_position_locked(period_s: u64, g: &mut Inner) {
    let now_ms = unix_time_ms();
    let ms_in_day = now_ms % 86_400_000u64;
    let ms_in_period = ms_in_day % (period_s * 1000u64);

    let prev_kin = g.kin as isize;
    let mut kin = ((ms_in_period * u64::from(OUTPUT_SAMPLE_RATE_HZ)) / 1000u64) as usize;
    if kin > g.d2.len() {
        kin = g.d2.len();
    }

    g.kin = kin;
    g.buffer_pos = 0;
    g.ns = second_in_period(period_s);

    let delta = g.kin as isize - prev_kin;
    if delta < 0 {
        g.d2.rotate_left((-delta) as usize);
    } else if delta > 0 {
        g.d2.rotate_right(delta as usize);
    }
}

fn reset_buffer_content_locked(g: &mut Inner) {
    g.d2.fill(0);
}

fn second_in_period(period_s: u64) -> u64 {
    let now_ms = unix_time_ms();
    let sec_in_day = (now_ms % 86_400_000) / 1000;
    sec_in_day % period_s
}

/// 49-tap FIR decimator by 4, stateful, f32 coefficients, i16 in/out.
///
/// - caller provides 4 new 48 kHz samples per output sample,
/// - internal FIR state advances by 4 samples,
/// - one 12 kHz output is produced.
struct FirDecimator49x4 {
    h: [f32; NTAPS],
    z: [f32; NTAPS * 2],
    w: usize,
}

impl FirDecimator49x4 {
    const fn new(h: [f32; NTAPS]) -> Self {
        Self {
            h,
            z: [0.0; NTAPS * 2],
            w: 0,
        }
    }

    #[inline]
    const fn push(&mut self, x: f32) {
        if self.w == 0 {
            self.w = NTAPS - 1;
        } else {
            self.w -= 1;
        }
        self.z[self.w] = x;
        self.z[self.w + NTAPS] = x;
    }

    /// Feed 4 new samples, compute one output.
    ///
    /// Output conversion uses truncation toward zero (using `i32::from()`), then saturates to i16.
    #[inline]
    fn down_sample_i16(&mut self, x4: [i16; NDOWN]) -> i16 {
        self.push(f32::from(x4[0]));
        self.push(f32::from(x4[1]));
        self.push(f32::from(x4[2]));
        self.push(f32::from(x4[3]));

        let samples = &self.z[self.w..self.w + NTAPS];
        let mut acc = 0.0f32;
        for k in 0..NTAPS / 2 {
            let pair = samples[k] + samples[NTAPS - 1 - k];
            acc = self.h[k].mul_add(pair, acc);
        }
        acc = self.h[NTAPS / 2].mul_add(samples[NTAPS / 2], acc);
        let yi = acc as i32;
        if yi > i32::from(i16::MAX) {
            i16::MAX
        } else if yi < i32::from(i16::MIN) {
            i16::MIN
        } else {
            yi as i16
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowpass_is_exactly_symmetric() {
        for k in 0..NTAPS / 2 {
            assert_eq!(LOWPASS[k].to_bits(), LOWPASS[NTAPS - 1 - k].to_bits());
        }
    }

    #[test]
    fn mono_write_produces_decimated_block_and_samples() {
        let detector = Detector::new(1, 256);
        {
            let mut g = detector.inner.lock().unwrap();
            g.kin = 0;
            g.buffer_pos = 0;
        }
        let input = vec![100i16; 256 * NDOWN];
        let stats = detector.write_i16(&input, InputFormat::Mono);

        assert_eq!(stats.frames_in, 256 * NDOWN);
        assert_eq!(stats.frames_accepted, 256 * NDOWN);
        assert_eq!(stats.frames_dropped, 0);
        assert_eq!(stats.blocks_written, 1);
        assert_eq!(stats.kin_after, 256);

        detector.with_samples(|samples, kin| {
            assert_eq!(samples.len(), JS8_RX_SAMPLE_SIZE);
            assert_eq!(kin, 256);
        });

        let mut samples = vec![0; JS8_RX_SAMPLE_SIZE];
        let kin = detector.copy_samples(&mut samples);
        assert_eq!(samples.len(), JS8_RX_SAMPLE_SIZE);
        assert_eq!(kin, 256);
    }

    #[test]
    fn stereo_formats_accept_expected_frame_counts() {
        let detector_left = Detector::new(1, 128);
        let detector_avg = Detector::new(1, 128);

        let mut stereo = vec![0i16; 200 * 2];
        for i in 0..200 {
            stereo[2 * i] = i as i16;
            stereo[2 * i + 1] = (200 - i) as i16;
        }

        let left = detector_left.write_i16(&stereo, InputFormat::StereoLeft);
        let avg = detector_avg.write_i16(&stereo, InputFormat::StereoAverage);

        assert_eq!(left.frames_in, 200);
        assert_eq!(avg.frames_in, 200);
        assert_eq!(left.frames_accepted, 200);
        assert_eq!(avg.frames_accepted, 200);
    }

    #[test]
    fn drops_frames_when_near_capacity() {
        let detector = Detector::new(1, 32);
        {
            let mut g = detector.inner.lock().unwrap();
            g.kin = JS8_RX_SAMPLE_SIZE - 10;
            g.buffer_pos = 0;
        }

        let input = vec![1i16; 200];
        let stats = detector.write_i16(&input, InputFormat::Mono);

        assert_eq!(stats.frames_in, 200);
        assert_eq!(stats.frames_accepted, 40);
        assert_eq!(stats.frames_dropped, 160);
    }

    #[test]
    fn wrap_resets_kin_when_period_rolls_over() {
        let detector = Detector::new(1, 64);
        {
            let mut g = detector.inner.lock().unwrap();
            g.kin = 500;
            g.ns = u64::MAX;
        }

        let input = vec![0i16; 64 * NDOWN];
        let stats = detector.write_i16(&input, InputFormat::Mono);
        assert!(stats.kin_after <= 64);
    }
}
