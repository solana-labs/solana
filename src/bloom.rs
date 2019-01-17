//! Simple Bloom Filter
use bv::BitVec;
use fnv::FnvHasher;
use rand::{self, Rng};
use std::cmp;
use std::hash::Hasher;
use std::marker::PhantomData;

/// Generate a stable hash of `self` for each `hash_index`
/// Best effort can be made for uniqueness of each hash.
pub trait BloomHashIndex {
    fn hash_at_index(&self, hash_index: u64) -> u64;
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Bloom<T: BloomHashIndex> {
    pub keys: Vec<u64>,
    pub bits: BitVec<u8>,
    _phantom: PhantomData<T>,
}

impl<T: BloomHashIndex> Bloom<T> {
    pub fn new(num_bits: usize, keys: Vec<u64>) -> Self {
        let bits = BitVec::new_fill(false, num_bits as u64);
        Bloom {
            keys,
            bits,
            _phantom: Default::default(),
        }
    }
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
        Self::new(num_bits, keys)
    }
    fn pos(&self, key: &T, k: u64) -> u64 {
        key.hash_at_index(k) % self.bits.len()
    }
    pub fn clear(&mut self) {
        self.bits.clear();
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

//fn to_slice(v: u64) -> [u8; 8] {
//    [
//        v as u8,
//        (v >> 8) as u8,
//        (v >> 16) as u8,
//        (v >> 24) as u8,
//        (v >> 32) as u8,
//        (v >> 40) as u8,
//        (v >> 48) as u8,
//        (v >> 56) as u8,
//    ]
//}

//fn from_slice(v: &[u8]) -> u64 {
//    u64::from(v[0])
//        | u64::from(v[1]) << 8
//        | u64::from(v[2]) << 16
//        | u64::from(v[3]) << 24
//        | u64::from(v[4]) << 32
//        | u64::from(v[5]) << 40
//        | u64::from(v[6]) << 48
//        | u64::from(v[7]) << 56
//}
//
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
    //    #[test]
    //    fn test_slice() {
    //        assert_eq!(from_slice(&to_slice(10)), 10);
    //        assert_eq!(from_slice(&to_slice(0x7fff7fff)), 0x7fff7fff);
    //        assert_eq!(
    //            from_slice(&to_slice(0x7fff7fff7fff7fff)),
    //            0x7fff7fff7fff7fff
    //        );
    //    }

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
