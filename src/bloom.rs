//! Simple Bloom Filter
use crate::bloom_hash_index::BloomHashIndex;
use bv::BitVec;
use rand::{self, Rng};
use std::cmp;
use std::marker::PhantomData;

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Bloom<T: BloomHashIndex> {
    pub keys: Vec<u64>,
    pub bits: BitVec<u8>,
    _phantom: PhantomData<T>,
}

impl<T: BloomHashIndex> Bloom<T> {
    /// create filter optimal for num size given the `false_rate`
    /// the keys are randomized for picking data out of a collision resistant hash of size
    /// `keysize` bytes
    /// https://hur.st/bloomfilter/
    pub fn random(num: usize, false_rate: f64, max_bits: usize) -> Self {
        let min_num_bits = ((num as f64 * false_rate.log(2f64))
            / (1f64 / 2f64.powf(2f64.log(2f64))).log(2f64))
        .ceil() as usize;
        let num_bits = cmp::max(1, cmp::min(min_num_bits, max_bits));
        let num_keys = ((num_bits as f64 / num as f64) * 2f64.log(2f64)).round() as usize;
        let keys: Vec<u64> = (0..num_keys).map(|_| rand::thread_rng().gen()).collect();
        let bits = BitVec::new_fill(false, num_bits as u64);
        Bloom {
            keys,
            bits,
            _phantom: Default::default(),
        }
    }
    fn pos(&self, key: &T, k: u64) -> u64 {
        key.hash(k) % self.bits.len()
    }
    pub fn add(&mut self, key: &T) {
        for k in &self.keys {
            let pos = self.pos(key, *k);
            self.bits.set(pos, true);
        }
    }
    pub fn contains(&mut self, key: &T) -> bool {
        for k in &self.keys {
            let pos = self.pos(key, *k);
            if !self.bits.get(pos) {
                return false;
            }
        }
        true
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
        assert_eq!(bloom.bits.len(), 34);

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
}
