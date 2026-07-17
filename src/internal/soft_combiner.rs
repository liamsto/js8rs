use crate::protocol::Submode;
use core::hash::{Hash, Hasher};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SoftCombinerKey {
    pub mode: Submode,
    pub freq_bin: i32,
    pub dt_bin: i32,
    pub signature: u32,
}

#[derive(Clone, Debug)]
pub struct SoftCombined<const N: usize> {
    pub key: SoftCombinerKey,
    pub llr0: [f32; N],
    pub llr1: [f32; N],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CoarseKey {
    mode: Submode,
    freq_bin: i32,
    dt_bin: i32,
}

impl Hash for CoarseKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Equivalent mixing to C++ (h1 ^ (h2<<1) ^ (h3<<2)), but w/ the Rust hasher.
        self.mode.hash(state);
        self.freq_bin.hash(state);
        self.dt_bin.hash(state);
    }
}

#[derive(Clone)]
struct Entry<const N: usize> {
    signature: u32,
    llr0: [f32; N],
    llr1: [f32; N],
    repeats: u32,
    last_seen: Instant,
}

type Bucket<const N: usize> = Vec<Entry<N>>;

pub struct SoftCombiner<const N: usize> {
    entries: HashMap<CoarseKey, Bucket<N>>,
}

impl<const N: usize> SoftCombiner<N> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    pub fn make_key(
        &self,
        mode: Submode,
        f1: f32,
        dt: f32,
        llr0: &[f32; N],
        llr1: &[f32; N],
    ) -> SoftCombinerKey {
        SoftCombinerKey {
            mode,
            freq_bin: f1.round() as i32,        // lround
            dt_bin: (dt * 10.0).round() as i32, // 100 ms bins
            signature: Self::signature(llr0, llr1),
        }
    }

    pub fn combine(
        &mut self,
        key: SoftCombinerKey,
        llr0: &[f32; N],
        llr1: &[f32; N],
        ttl: Duration,
    ) -> SoftCombined<N> {
        self.flush(ttl);

        let lookup = Self::key_for_lookup(key);
        let bucket = self.entries.entry(lookup).or_default();

        if let Some(entry) = Self::find_entry(bucket, key.signature) {
            for i in 0..N {
                entry.llr0[i] += llr0[i];
                entry.llr1[i] += llr1[i];
            }
            entry.repeats += 1;
            entry.last_seen = Instant::now();

            SoftCombined {
                key,
                llr0: entry.llr0,
                llr1: entry.llr1,
            }
        } else {
            bucket.push(Self::make_entry(key.signature, llr0, llr1));
            SoftCombined {
                key,
                llr0: *llr0,
                llr1: *llr1,
            }
        }
    }

    pub fn mark_decoded(&mut self, key: SoftCombinerKey) {
        let lookup = Self::key_for_lookup(key);
        let Some(bucket) = self.entries.get_mut(&lookup) else {
            return;
        };

        bucket.retain(|e| e.signature != key.signature);

        if bucket.is_empty() {
            self.entries.remove(&lookup);
        }
    }

    pub fn flush(&mut self, ttl: Duration) {
        let now = Instant::now();

        self.entries.retain(|_, bucket| {
            bucket.retain(|e| now.duration_since(e.last_seen) <= ttl);
            !bucket.is_empty()
        });
    }

    #[inline]
    const fn key_for_lookup(key: SoftCombinerKey) -> CoarseKey {
        CoarseKey {
            mode: key.mode,
            freq_bin: key.freq_bin,
            dt_bin: key.dt_bin,
        }
    }

    const fn signature_indices() -> [usize; 32] {
        let mut indices = [0usize; 32];
        let mut value: i32 = 0;

        let mut i = 0usize;
        while i < 32 {
            value = (value + 37) % (N as i32);
            indices[i] = value as usize;
            i += 1;
        }
        indices
    }

    fn find_entry(bucket: &mut Bucket<N>, signature: u32) -> Option<&mut Entry<N>> {
        const MAX_HAMMING: i32 = 4;

        bucket
            .iter_mut()
            .find(|e| Self::hamming(signature, e.signature) <= MAX_HAMMING)
    }

    fn signature(llr0: &[f32; N], llr1: &[f32; N]) -> u32 {
        let indices = Self::signature_indices();

        let mut sig: u32 = 0;
        for (i, idx) in indices.iter().copied().enumerate() {
            let v = 0.5 * (llr0[idx] + llr1[idx]);
            if v >= 0.0 {
                sig |= 1u32 << i;
            }
        }
        sig
    }

    #[inline]
    const fn hamming(a: u32, b: u32) -> i32 {
        let mut v = a ^ b;
        let mut c = 0i32;
        while v != 0 {
            v &= v - 1;
            c += 1;
        }
        c
    }

    fn make_entry(signature: u32, llr0: &[f32; N], llr1: &[f32; N]) -> Entry<N> {
        Entry {
            signature,
            llr0: *llr0,
            llr1: *llr1,
            repeats: 1,
            last_seen: Instant::now(),
        }
    }
}
