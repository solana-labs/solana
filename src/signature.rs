//! The `signature` module provides functionality for public, and private keys.

use generic_array::typenum::{U32, U64};
use generic_array::GenericArray;
use rand::{ChaChaRng, Rng, SeedableRng};
use rayon::prelude::*;
use ring::error::Unspecified;
use ring::rand::SecureRandom;
use ring::signature::Ed25519KeyPair;
use ring::{rand, signature};
use std::cell::RefCell;
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
        let pkcs8_bytes = signature::Ed25519KeyPair::generate_pkcs8(&rng)
            .expect("generate_pkcs8 in signature pb fn new");
        signature::Ed25519KeyPair::from_pkcs8(untrusted::Input::from(&pkcs8_bytes))
            .expect("from_pcks8 in signature pb fn new")
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
    // This is necessary because the rng needs to mutate its state to remain
    // deterministic, and the fill trait requires an immuatble reference to self
    generator: RefCell<ChaChaRng>,
}

impl GenKeys {
    pub fn new(seed: [u8; 32]) -> GenKeys {
        let rng = ChaChaRng::from_seed(seed);
        GenKeys {
            generator: RefCell::new(rng),
        }
    }

    pub fn new_key(&self) -> Vec<u8> {
        KeyPair::generate_pkcs8(self).unwrap().to_vec()
    }

    pub fn gen_n_seeds(&self, n: i64) -> Vec<[u8; 32]> {
        let mut rng = self.generator.borrow_mut();
        (0..n).map(|_| rng.gen()).collect()
    }

    pub fn gen_n_keypairs(&self, n: i64) -> Vec<KeyPair> {
        self.gen_n_seeds(n)
            .into_par_iter()
            .map(|seed| {
                let pkcs8 = GenKeys::new(seed).new_key();
                KeyPair::from_pkcs8(untrusted::Input::from(&pkcs8)).unwrap()
            })
            .collect()
    }
}

impl SecureRandom for GenKeys {
    fn fill(&self, dest: &mut [u8]) -> Result<(), Unspecified> {
        let mut rng = self.generator.borrow_mut();
        rng.fill(dest);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_new_key_is_deterministic() {
        let seed = [0u8; 32];
        let rng0 = GenKeys::new(seed);
        let rng1 = GenKeys::new(seed);

        for _ in 0..100 {
            assert_eq!(rng0.new_key(), rng1.new_key());
        }
    }

    fn gen_n_pubkeys(seed: [u8; 32], n: i64) -> HashSet<PublicKey> {
        GenKeys::new(seed)
            .gen_n_keypairs(n)
            .into_iter()
            .map(|x| x.pubkey())
            .collect()
    }

    #[test]
    fn test_gen_n_pubkeys_deterministic() {
        let seed = [0u8; 32];
        assert_eq!(gen_n_pubkeys(seed, 50), gen_n_pubkeys(seed, 50));
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;

    use self::test::Bencher;
    use super::*;

    #[bench]
    fn bench_gen_keys(b: &mut Bencher) {
        let rnd = GenKeys::new([0u8; 32]);
        b.iter(|| rnd.gen_n_keypairs(1000));
    }
}
