//! Simple Bloom Filter
use crate::accounts::{deserialize_object, serialize_object};
use bitintr::Popcnt;
use bv::BitVec;
use bv::{Bits, BitsMut};
use byteorder::LittleEndian;
use byteorder::{ReadBytesExt, WriteBytesExt};
use fnv::FnvHasher;
use rand::{self, Rng};
use serde::{Deserialize, Serialize};
use std::cmp;
use std::hash::Hasher;
use std::io::{Read, Write};
use std::marker::PhantomData;

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

    pub fn serialize_into<W: Write>(&self, writer: &mut W) -> Result<(), ()> {
        serialize_object(&self.keys, writer).unwrap();
        writer
            .write_u64::<LittleEndian>(self.bits.len() as u64)
            .unwrap();
        let mut set: Vec<(u64, u64)> = vec![];

        let mut total = 0;
        for i in 0..self.bits.block_len() {
            let block = self.bits.get_block(i);
            if block != 0 {
                set.push((i as u64, block));
                total += block.popcnt();
            }
            if total >= self.num_bits_set {
                break;
            }
        }

        serialize_object(&set, writer).unwrap();
        Ok(())
    }

    pub fn deserialize_from<R: Read>(reader: &mut R) -> Result<Self, ()> {
        let keys = deserialize_object(reader).unwrap();
        let len = reader.read_u64::<LittleEndian>().unwrap();
        let set: Vec<(u64, u64)> = deserialize_object(reader).unwrap();
        let mut bits = BitVec::new_fill(false, len);
        let mut num_bits_set = 0;
        for (i, block) in &set {
            bits.set_block(*i as usize, *block);
            num_bits_set += block.popcnt();
        }
        Ok(Bloom {
            keys,
            bits,
            num_bits_set,
            _phantom: PhantomData::default(),
        })
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
