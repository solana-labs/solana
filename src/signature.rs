//! The `signature` module provides functionality for public, and private keys.

use generic_array::GenericArray;
use generic_array::typenum::{U32, U64};
use rand::{ChaChaRng, Rng, SeedableRng};
use rayon::prelude::*;
use ring::error::Unspecified;
use ring::rand::SecureRandom;
use ring::signature::Ed25519KeyPair;
use ring::{rand, signature};
use std::mem;
use std::sync::RwLock;
use untrusted;

pub type KeyPair = Ed25519KeyPair;
pub type PublicKey = GenericArray<u8, U32>;
pub type Signature = GenericArray<u8, U64>;

pub trait KeyPairUtil {
    fn new() -> Self;
    fn pubkey(&self) -> PublicKey;
}

impl KeyPairUtil for Ed25519KeyPair {
    /// Return a new ED25519 keypair
    fn new() -> Self {
        let rng = rand::SystemRandom::new();
        let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        signature::Ed25519KeyPair::from_pkcs8(untrusted::Input::from(&pkcs8_bytes)).unwrap()
    }

    /// Return the public key for the given keypair
    fn pubkey(&self) -> PublicKey {
        GenericArray::clone_from_slice(self.public_key_bytes())
    }
}

pub trait SignatureUtil {
    fn verify(&self, peer_public_key_bytes: &[u8], msg_bytes: &[u8]) -> bool;
}

impl SignatureUtil for GenericArray<u8, U64> {
    fn verify(&self, peer_public_key_bytes: &[u8], msg_bytes: &[u8]) -> bool {
        let peer_public_key = untrusted::Input::from(peer_public_key_bytes);
        let msg = untrusted::Input::from(msg_bytes);
        let sig = untrusted::Input::from(self);
        signature::verify(&signature::ED25519, peer_public_key, msg, sig).is_ok()
    }
}

pub struct GenKeys {
    generator: RwLock<ChaChaRng>,
}

impl GenKeys {
    pub fn new(seed_values: &[u8]) -> GenKeys {
        let seed: &[u8] = &seed_values[..];
        let rng: ChaChaRng = SeedableRng::from_seed(unsafe { mem::transmute(seed) });
        GenKeys {
            generator: RwLock::new(rng),
        }
    }

    pub fn new_key(&self) -> Vec<u8> {
        KeyPair::generate_pkcs8(self).unwrap().to_vec()
    }

    pub fn gen_n_keys(&self, n_keys: i64, tokens_per_user: i64) -> Vec<(Vec<u8>, i64)> {
        let users: Vec<_> = (0..n_keys)
            .into_par_iter()
            .map(|_| {
                let pkcs8 = self.new_key();
                (pkcs8, tokens_per_user)
            })
            .collect();
        users
    }
}

impl SecureRandom for GenKeys {
    fn fill(&self, dest: &mut [u8]) -> Result<(), Unspecified> {
        let mut rng = self.generator.write().unwrap();
        rng.fill_bytes(dest);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::iter::FromIterator;

    #[test]
    fn test_new_key_is_redundant() {
        let seed: &[_] = &[1, 2, 3, 4];
        let rnd = GenKeys::new(seed);
        let rnd2 = GenKeys::new(seed);

        for _ in 0..100 {
            assert_eq!(rnd.new_key(), rnd2.new_key());
        }
    }

    #[test]
    fn test_gen_n_keys() {
        let seed: &[_] = &[1, 2, 3, 4];
        let rnd = GenKeys::new(seed);
        let rnd2 = GenKeys::new(seed);

        let users1 = rnd.gen_n_keys(50, 1);
        let users2 = rnd2.gen_n_keys(50, 1);

        let users1_set: HashSet<(Vec<u8>, i64)> = HashSet::from_iter(users1.iter().cloned());
        let users2_set: HashSet<(Vec<u8>, i64)> = HashSet::from_iter(users2.iter().cloned());
        assert_eq!(users1_set, users2_set);
    }
}
