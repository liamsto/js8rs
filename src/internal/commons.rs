pub const JS8_NTMAX: usize = 60;
pub const JS8_RX_SAMPLE_RATE: u64 = 12_000;
pub const JS8_RX_SAMPLE_SIZE: usize = JS8_NTMAX * JS8_RX_SAMPLE_RATE as usize;
pub const JS8_NUM_SYMBOLS: u64 = 79;

pub const JS8A_SYMBOL_SAMPLES: u64 = 1920;
pub const JS8A_TX_SECONDS: u64 = 15;
pub const JS8A_START_DELAY_MS: u64 = 500;

pub const JS8B_SYMBOL_SAMPLES: u64 = 1200;
pub const JS8B_TX_SECONDS: u64 = 10;
pub const JS8B_START_DELAY_MS: u64 = 200;

pub const JS8C_SYMBOL_SAMPLES: u64 = 600;
pub const JS8C_TX_SECONDS: u64 = 6;
pub const JS8C_START_DELAY_MS: u64 = 100;

pub const JS8E_SYMBOL_SAMPLES: u64 = 3840;
pub const JS8E_TX_SECONDS: u64 = 30;
pub const JS8E_START_DELAY_MS: u64 = 500;

pub const JS8I_SYMBOL_SAMPLES: u64 = 384;
pub const JS8I_TX_SECONDS: u64 = 4;
pub const JS8I_START_DELAY_MS: u64 = 100;

#[derive(Clone, Copy, Default, Debug)]
pub struct DecodeParams {
    /// UTC, as integer, ecnoded by `code_time` (HHMMSS).
    pub nutc: u32,
    /// User-selected QSO freq in kHz.
    pub nfqso: u32,
    /// Low decode limit, Hz (filter min).
    pub nfa: u32,
    /// Highdecode limit, Hz (filter max).
    pub nfb: u32,
    /// Whether to compute sync candidates.
    pub sync_stats: bool,
    /// Starting position of decode, submode A.
    pub kpos_a: usize,
    /// Starting position of decode, submode B.
    pub kpos_b: usize,
    /// Starting position of decode, submode C.
    pub kpos_c: usize,
    /// Starting position of decode, submode E.
    pub kpos_e: usize,
    /// Starting position of decode, submode I.
    pub kpos_i: usize,
    /// Number of frames for decode, submode A.
    pub ksz_a: usize,
    /// Number of frames for decode, submode B.
    pub ksz_b: usize,
    /// Number of frames for decode, submode C.
    pub ksz_c: usize,
    /// Number of frames for decode, submode E.
    pub ksz_e: usize,
    /// Number of frames for decode, submode I.
    pub ksz_i: usize,
    /// Submodes to decode.
    pub nsubmodes: u8,
}

pub struct DecData<'a> {
    pub d2: &'a [i16],
    pub params: DecodeParams,
}

/// Hour / minute / second triple.
#[derive(Copy, Clone)]
pub struct HourMinuteSecond {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// Decode a packed HHMMSS timestamp.
#[inline]
pub const fn decode_time(nutc: u32) -> HourMinuteSecond {
    HourMinuteSecond {
        hour: (nutc / 10_000) as u8,
        minute: ((nutc % 10_000) / 100) as u8,
        second: (nutc % 100) as u8,
    }
}
