use std::{ops::Range, sync::OnceLock};

const INDEX_BITS: u32 = 18;
const LOOKUP_HEADER_SIZE: usize = 12;
const ROUTE_COUNT: usize = 256;
const ROUTE_SIZE: usize = 4;
const SPAN_SIZE: usize = 12;

pub const DIRECT_ROUTE: u32 = 1 << 31;
pub const NO_ROUTE: u32 = u32::MAX;
pub const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;

pub static JSC_MAP_BIN: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/jsc_map.bin"));

pub static JSC_LIST_BIN: &[u8] =
    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/jsc_list.bin"));

pub static JSC_PREFIX_BIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/jsc_prefix.bin"
));

#[derive(Debug, Copy, Clone)]
pub struct BinTupleRef<'a> {
    pub text: &'a [u8],
    pub size: u32,
}

#[derive(Debug, Copy, Clone)]
pub struct RankRef {
    pub size: u32,
    pub index: u32,
}

#[derive(Debug, Copy, Clone)]
pub struct LookupSpan {
    pub start: u32,
    pub count: u32,
    pub sizes: u32,
}

#[derive(Debug)]
pub struct BinTable {
    bytes: &'static [u8],
    count: u32,
    max_len: u32,
    rec_size: usize,
    data_off: usize,
}

impl BinTable {
    pub fn new(bytes: &'static [u8]) -> Self {
        assert!(bytes.len() >= 8, "bin too small");

        let count = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let max_len = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        let rec_size = 8usize + max_len as usize;
        let data_off = 8usize;
        let need = data_off + count as usize * rec_size;
        assert!(
            bytes.len() >= need,
            "bin truncated: need {need}, have {}",
            bytes.len()
        );

        Self {
            bytes,
            count,
            max_len,
            rec_size,
            data_off,
        }
    }

    #[inline]
    pub const fn count(&self) -> u32 {
        self.count
    }

    #[inline]
    const fn record_range(&self, i: u32) -> Range<usize> {
        let start = self.data_off + i as usize * self.rec_size;
        start..start + self.rec_size
    }

    #[inline]
    pub fn get(&self, i: u32) -> Option<BinTupleRef<'_>> {
        if i >= self.count {
            return None;
        }

        let rec = &self.bytes[self.record_range(i)];
        let size = i32::from_le_bytes(rec[0..4].try_into().unwrap()).max(0) as u32;
        let text = &rec[8..8 + self.max_len as usize];

        Some(BinTupleRef { text, size })
    }

    #[inline]
    pub fn text_trimmed(t: BinTupleRef<'_>) -> &[u8] {
        let mut n = t.text.len();
        while n > 0 && t.text[n - 1] == 0 {
            n -= 1;
        }
        &t.text[..n]
    }
}

#[derive(Debug)]
pub struct RankTable {
    bytes: &'static [u8],
    count: u32,
}

impl RankTable {
    fn new(bytes: &'static [u8]) -> Self {
        assert!(
            bytes.len().is_multiple_of(4),
            "packed rank table is truncated"
        );
        Self {
            bytes,
            count: (bytes.len() / 4) as u32,
        }
    }

    #[inline]
    #[cfg(test)]
    pub const fn count(&self) -> u32 {
        self.count
    }

    #[inline]
    pub fn get(&self, rank: u32) -> Option<RankRef> {
        if rank >= self.count {
            return None;
        }
        let offset = rank as usize * 4;
        let packed = u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap());
        Some(RankRef {
            size: packed >> INDEX_BITS,
            index: packed & INDEX_MASK,
        })
    }
}

#[derive(Debug)]
pub struct LookupTable {
    bytes: &'static [u8],
    span_count: u32,
}

impl LookupTable {
    fn new(bytes: &'static [u8]) -> Self {
        assert!(
            bytes.len() >= LOOKUP_HEADER_SIZE,
            "lookup index is truncated"
        );
        assert_eq!(&bytes[..4], b"JSCI", "invalid lookup index magic");
        let routes = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let span_count = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        assert_eq!(routes, ROUTE_COUNT, "invalid lookup route count");
        let need = LOOKUP_HEADER_SIZE + routes * ROUTE_SIZE + span_count as usize * SPAN_SIZE;
        assert_eq!(bytes.len(), need, "invalid lookup index length");
        Self { bytes, span_count }
    }

    #[inline]
    pub fn route(&self, byte: u8) -> u32 {
        let offset = LOOKUP_HEADER_SIZE + usize::from(byte) * ROUTE_SIZE;
        u32::from_le_bytes(self.bytes[offset..offset + ROUTE_SIZE].try_into().unwrap())
    }

    #[inline]
    pub fn span(&self, index: u32) -> Option<LookupSpan> {
        if index >= self.span_count {
            return None;
        }
        let offset = LOOKUP_HEADER_SIZE + ROUTE_COUNT * ROUTE_SIZE + index as usize * SPAN_SIZE;
        let record = &self.bytes[offset..offset + SPAN_SIZE];
        Some(LookupSpan {
            start: u32::from_le_bytes(record[..4].try_into().unwrap()),
            count: u32::from_le_bytes(record[4..8].try_into().unwrap()),
            sizes: u32::from_le_bytes(record[8..12].try_into().unwrap()),
        })
    }

    #[inline]
    #[cfg(test)]
    pub const fn span_count(&self) -> u32 {
        self.span_count
    }
}

static MAP_TABLE: OnceLock<BinTable> = OnceLock::new();
static RANK_TABLE: OnceLock<RankTable> = OnceLock::new();
static LOOKUP_TABLE: OnceLock<LookupTable> = OnceLock::new();

#[inline]
pub fn map_table() -> &'static BinTable {
    MAP_TABLE.get_or_init(|| BinTable::new(JSC_MAP_BIN))
}

#[inline]
pub fn rank_table() -> &'static RankTable {
    RANK_TABLE.get_or_init(|| RankTable::new(JSC_LIST_BIN))
}

#[inline]
pub fn lookup_table() -> &'static LookupTable {
    LOOKUP_TABLE.get_or_init(|| LookupTable::new(JSC_PREFIX_BIN))
}
