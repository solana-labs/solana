//! The `signature` module provides functionality for public, and private keys.

use bs58;
use ed25519_dalek;
use generic_array::typenum::U64;
use generic_array::GenericArray;
use rand::{ChaChaRng, OsRng, Rng, SeedableRng};
use rayon::prelude::*;
use serde_json;
use sha2::Sha512;
use solana_sdk::pubkey::Pubkey;
use std::error;
use std::fmt;
use std::fs::File;

pub type Keypair = ed25519_dalek::Keypair;

#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Signature(GenericArray<u8, U64>);

impl Signature {
    pub fn new(signature_slice: &[u8]) -> Self {
        Signature(GenericArray::clone_from_slice(&signature_slice))
    }
    pub fn verify(&self, pubkey_bytes: &[u8], message_bytes: &[u8]) -> bool {
        let pubkey = ed25519_dalek::PublicKey::from_bytes(pubkey_bytes).unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(self.0.as_slice()).unwrap();
        pubkey.verify::<Sha512>(message_bytes, &signature).is_ok()
    }
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

pub trait KeypairUtil {
    fn new() -> Self;
    fn pubkey(&self) -> Pubkey;
}

impl KeypairUtil for Keypair {
    /// Return a new ED25519 keypair
    fn new() -> Self {
        let mut rng: OsRng = OsRng::new().unwrap();
        Keypair::generate::<Sha512, _>(&mut rng)
    }

    /// Return the public key for the given keypair
    fn pubkey(&self) -> Pubkey {
        Pubkey::new(&self.public.to_bytes())
    }
}

pub struct GenKeys {
    generator: ChaChaRng,
}

impl GenKeys {
    pub fn new(seed: [u8; 32]) -> GenKeys {
        let generator = ChaChaRng::from_seed(seed);
        GenKeys { generator }
    }

    fn gen_seed(&mut self) -> [u8; 64] {
        let mut seed = [0u8; 64];
        self.generator.fill(&mut seed);
        seed
    }

    fn gen_n_seeds(&mut self, n: u64) -> Vec<[u8; 64]> {
        (0..n).map(|_| self.gen_seed()).collect()
    }

    pub fn gen_n_keypairs(&mut self, n: u64) -> Vec<Keypair> {
        self.gen_n_seeds(n)
            .into_par_iter()
            .map(|seed| Keypair::from_bytes(&seed).unwrap())
            .collect()
    }
}

pub fn read_pkcs8(path: &str) -> Result<Vec<u8>, Box<error::Error>> {
    let file = File::open(path.to_string())?;
    let pkcs8: Vec<u8> = serde_json::from_reader(file)?;
    Ok(pkcs8)
}

pub fn read_keypair(path: &str) -> Result<Keypair, Box<error::Error>> {
    let pkcs8 = read_pkcs8(path)?;
    let keypair = Keypair::from_bytes(&pkcs8).unwrap();
    Ok(keypair)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_new_key_is_deterministic() {
        let seed = [0u8; 32];
        let mut gen0 = GenKeys::new(seed);
        let mut gen1 = GenKeys::new(seed);

        for _ in 0..100 {
            assert_eq!(gen0.gen_seed().to_vec(), gen1.gen_seed().to_vec());
        }
    }

    fn gen_n_pubkeys(seed: [u8; 32], n: u64) -> HashSet<Pubkey> {
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
