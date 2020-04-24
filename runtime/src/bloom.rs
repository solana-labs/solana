//! Simple Bloom Filter
use bv::BitVec;
use fnv::FnvHasher;
use rand::{self, Rng};
use serde::{Deserialize, Serialize};
use std::{cmp, hash::Hasher, marker::PhantomData};

/// Generate a stable hash of `self` for each `hash_index`
/// Best effort can be made for uniqueness of each hash.
pub trait BloomHashIndex {
    fn hash_at_index(&self, hash_index: u64) -> u64;
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Bloom<T: BloomHashIndex> {
    pub keys: Vec<u64>,
    pub bits: BitVec<u64>,
    num_bits_set: u64,
    _phantom: PhantomData<T>,
}

impl<T: BloomHashIndex> solana_sdk::sanitize::Sanitize for Bloom<T> {}

impl<T: BloomHashIndex> Bloom<T> {
    pub fn new(num_bits: usize, keys: Vec<u64>) -> Self {
        let bits = BitVec::new_fill(false, num_bits as u64);
        Bloom {
            keys,
            bits,
            num_bits_set: 0,
            _phantom: PhantomData::default(),
        }
    }
    /// create filter optimal for num size given the `FALSE_RATE`
    /// the keys are randomized for picking data out of a collision resistant hash of size
    /// `keysize` bytes
    /// https://hur.st/bloomfilter/
    pub fn random(num_items: usize, false_rate: f64, max_bits: usize) -> Self {
        let m = Self::num_bits(num_items as f64, false_rate);
        let num_bits = cmp::max(1, cmp::min(m as usize, max_bits));
        let num_keys = Self::num_keys(num_bits as f64, num_items as f64) as usize;
        let keys: Vec<u64> = (0..num_keys).map(|_| rand::thread_rng().gen()).collect();
        Self::new(num_bits, keys)
    }
    pub fn num_bits(num_items: f64, false_rate: f64) -> f64 {
        let n = num_items;
        let p = false_rate;
        ((n * p.ln()) / (1f64 / 2f64.powf(2f64.ln())).ln()).ceil()
    }
    pub fn num_keys(num_bits: f64, num_items: f64) -> f64 {
        let n = num_items;
        let m = num_bits;
        1f64.max(((m / n) * 2f64.ln()).round())
    }
    fn pos(&self, key: &T, k: u64) -> u64 {
        key.hash_at_index(k) % self.bits.len()
    }
    pub fn clear(&mut self) {
        self.bits = BitVec::new_fill(false, self.bits.len());
        self.num_bits_set = 0;
    }
    pub fn add(&mut self, key: &T) {
        for k in &self.keys {
            let pos = self.pos(key, *k);
            if !self.bits.get(pos) {
                self.num_bits_set += 1;
                self.bits.set(pos, true);
            }
        }
    }
    pub fn contains(&self, key: &T) -> bool {
        for k in &self.keys {
            let pos = self.pos(key, *k);
            if !self.bits.get(pos) {
                return false;
            }
        }
        true
    }
}

fn slice_hash(slice: &[u8], hash_index: u64) -> u64 {
    let mut hasher = FnvHasher::with_key(hash_index);
    hasher.write(slice);
    hasher.finish()
}

impl<T: AsRef<[u8]>> BloomHashIndex for T {
    fn hash_at_index(&self, hash_index: u64) -> u64 {
        slice_hash(self.as_ref(), hash_index)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_sdk::hash::{hash, Hash};

    #[test]
    fn test_bloom_filter() {
        //empty
        let bloom: Bloom<Hash> = Bloom::random(0, 0.1, 100);
        assert_eq!(bloom.keys.len(), 0);
        assert_eq!(bloom.bits.len(), 1);

        //normal
        let bloom: Bloom<Hash> = Bloom::random(10, 0.1, 100);
        assert_eq!(bloom.keys.len(), 3);
        assert_eq!(bloom.bits.len(), 48);

        //saturated
        let bloom: Bloom<Hash> = Bloom::random(100, 0.1, 100);
        assert_eq!(bloom.keys.len(), 1);
        assert_eq!(bloom.bits.len(), 100);
    }
    #[test]
    fn test_add_contains() {
        let mut bloom: Bloom<Hash> = Bloom::random(100, 0.1, 100);
        //known keys to avoid false positives in the test
        bloom.keys = vec![0, 1, 2, 3];

        let key = hash(b"hello");
        assert!(!bloom.contains(&key));
        bloom.add(&key);
        assert!(bloom.contains(&key));

        let key = hash(b"world");
        assert!(!bloom.contains(&key));
        bloom.add(&key);
        assert!(bloom.contains(&key));
    }
    #[test]
    fn test_random() {
        let mut b1: Bloom<Hash> = Bloom::random(10, 0.1, 100);
        let mut b2: Bloom<Hash> = Bloom::random(10, 0.1, 100);
        b1.keys.sort();
        b2.keys.sort();
        assert_ne!(b1.keys, b2.keys);
    }
    // Bloom filter math in python
    // n number of items
    // p false rate
    // m number of bits
    // k number of keys
    //
    // n = ceil(m / (-k / log(1 - exp(log(p) / k))))
    // p = pow(1 - exp(-k / (m / n)), k)
    // m = ceil((n * log(p)) / log(1 / pow(2, log(2))));
    // k = round((m / n) * log(2));
    #[test]
    fn test_filter_math() {
        assert_eq!(Bloom::<Hash>::num_bits(100f64, 0.1f64) as u64, 480u64);
        assert_eq!(Bloom::<Hash>::num_bits(100f64, 0.01f64) as u64, 959u64);
        assert_eq!(Bloom::<Hash>::num_keys(1000f64, 50f64) as u64, 14u64);
        assert_eq!(Bloom::<Hash>::num_keys(2000f64, 50f64) as u64, 28u64);
        assert_eq!(Bloom::<Hash>::num_keys(2000f64, 25f64) as u64, 55u64);
        //ensure min keys is 1
        assert_eq!(Bloom::<Hash>::num_keys(20f64, 1000f64) as u64, 1u64);
    }
}
