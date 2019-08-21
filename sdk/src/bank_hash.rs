use crate::hash::Hash;
use bincode::deserialize_from;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;
use std::io::Cursor;

// Type for representing a bank accounts state.
// Taken by xor of a sha256 of accounts state for lower 32-bytes, and
// then generating the rest of the bytes with a chacha rng init'ed with that state.
// 440 bytes solves the birthday problem when xor'ing of preventing an attacker of
// finding a value or set of values that could be xor'ed to match the bitpattern
// of an existing state value.
const BANK_HASH_BYTES: usize = 448;
#[derive(Clone, Copy)]
pub struct BankHash([u8; BANK_HASH_BYTES]);

impl fmt::Debug for BankHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BankHash {}", hex::encode(&self.0[..32]))
    }
}

impl fmt::Display for BankHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl PartialEq for BankHash {
    fn eq(&self, other: &Self) -> bool {
        self.0[..] == other.0[..]
    }
}
impl Eq for BankHash {}

impl Default for BankHash {
    fn default() -> Self {
        BankHash([0u8; BANK_HASH_BYTES])
    }
}

impl BankHash {
    pub fn from_hash(hash: &Hash) -> Self {
        let mut new = BankHash::default();

        // default hash should result in all 0s thus nop for xor
        if *hash == Hash::default() {
            return new;
        }

        new.0[..32].copy_from_slice(hash.as_ref());
        let mut seed = [0u8; 32];
        seed.copy_from_slice(hash.as_ref());
        let mut generator = ChaChaRng::from_seed(seed);
        generator.fill(&mut new.0[32..]);
        new
    }

    pub fn is_default(&self) -> bool {
        self.0[0..32] == Hash::default().as_ref()[0..32]
    }

    pub fn xor(self: &mut BankHash, hash: BankHash) {
        for (i, b) in hash.as_ref().iter().enumerate() {
            self.0.as_mut()[i] ^= b;
        }
    }
}

impl Serialize for BankHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.0[..])
    }
}

struct BankHashVisitor;

impl<'a> serde::de::Visitor<'a> for BankHashVisitor {
    type Value = BankHash;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting BankHash")
    }

    #[allow(clippy::mutex_atomic)]
    fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        use serde::de::Error;
        let mut new = BankHash::default();
        let mut rd = Cursor::new(&data[..]);
        for i in 0..BANK_HASH_BYTES {
            new.0[i] = deserialize_from(&mut rd).map_err(Error::custom)?;
        }

        Ok(new)
    }
}

impl<'de> Deserialize<'de> for BankHash {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(BankHashVisitor)
    }
}

impl AsRef<[u8]> for BankHash {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl AsMut<[u8]> for BankHash {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.0[..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::hash;
    use bincode::{deserialize, serialize};
    use log::*;

    #[test]
    fn test_bankhash() {
        let hash = hash(&[1, 2, 3, 4]);
        let bank_hash = BankHash::from_hash(&hash);
        assert!(!bank_hash.is_default());

        let default = BankHash::default();
        assert!(default.is_default());
        assert!(bank_hash != default);
        assert!(bank_hash == bank_hash);

        for i in 0..BANK_HASH_BYTES / 32 {
            let start = i * 32;
            let end = start + 32;
            assert!(bank_hash.0[start..end] != [0u8; 32]);
        }
    }

    #[test]
    fn test_serialize() {
        solana_logger::setup();
        let hash = hash(&[1, 2, 3, 4]);
        let bank_hash = BankHash::from_hash(&hash);
        info!("{}", bank_hash);
        let bytes = serialize(&bank_hash).unwrap();
        let new: BankHash = deserialize(&bytes).unwrap();
        info!("{}", new);
        assert_eq!(new, bank_hash);
    }
}
