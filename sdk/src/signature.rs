//! The `signature` module provides functionality for public, and private keys.

use crate::pubkey::Pubkey;
use bs58;
use generic_array::typenum::U64;
use generic_array::GenericArray;
use rand::rngs::OsRng;
use serde_json;
use solana_ed25519_dalek as ed25519_dalek;
use std::borrow::Borrow;
use std::borrow::Cow;
use std::error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::mem;
use std::path::Path;
use std::str::FromStr;

pub type Keypair = ed25519_dalek::Keypair;

#[repr(transparent)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Signature(GenericArray<u8, U64>);

impl Signature {
    pub fn new(signature_slice: &[u8]) -> Self {
        Self(GenericArray::clone_from_slice(&signature_slice))
    }

    pub fn verify(&self, pubkey_bytes: &[u8], message_bytes: &[u8]) -> bool {
        let pubkey = ed25519_dalek::PublicKey::from_bytes(pubkey_bytes);
        let signature = ed25519_dalek::Signature::from_bytes(self.0.as_slice());
        if pubkey.is_err() || signature.is_err() {
            return false;
        }
        pubkey
            .unwrap()
            .verify(message_bytes, &signature.unwrap())
            .is_ok()
    }
}

pub trait Signable {
    fn sign(&mut self, keypair: &Keypair) {
        let signature = keypair.sign_message(self.signable_data().borrow());
        self.set_signature(signature);
    }
    fn verify(&self) -> bool {
        self.get_signature()
            .verify(&self.pubkey().as_ref(), self.signable_data().borrow())
    }

    fn pubkey(&self) -> Pubkey;
    fn signable_data(&self) -> Cow<[u8]>;
    fn get_signature(&self) -> Signature;
    fn set_signature(&mut self, signature: Signature);
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

impl Into<[u8; 64]> for Signature {
    fn into(self) -> [u8; 64] {
        <GenericArray<u8, U64> as Into<[u8; 64]>>::into(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseSignatureError {
    WrongSize,
    Invalid,
}

impl FromStr for Signature {
    type Err = ParseSignatureError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParseSignatureError::Invalid)?;
        if bytes.len() != mem::size_of::<Signature>() {
            Err(ParseSignatureError::WrongSize)
        } else {
            Ok(Signature::new(&bytes))
        }
    }
}

pub trait KeypairUtil {
    fn new() -> Self;
    fn pubkey(&self) -> Pubkey;
    fn sign_message(&self, message: &[u8]) -> Signature;
}

impl KeypairUtil for Keypair {
    /// Return a new ED25519 keypair
    fn new() -> Self {
        let mut rng = OsRng::new().unwrap();
        Keypair::generate(&mut rng)
    }

    /// Return the public key for the given keypair
    fn pubkey(&self) -> Pubkey {
        Pubkey::new(&self.public.as_ref())
    }

    fn sign_message(&self, message: &[u8]) -> Signature {
        Signature::new(&self.sign(message).to_bytes())
    }
}

pub fn read_keypair(path: &str) -> Result<Keypair, Box<dyn error::Error>> {
    let file = File::open(path.to_string())?;
    let bytes: Vec<u8> = serde_json::from_reader(file)?;
    let keypair = Keypair::from_bytes(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    Ok(keypair)
}

pub fn write_keypair(keypair: &Keypair, outfile: &str) -> Result<String, Box<dyn error::Error>> {
    let keypair_bytes = keypair.to_bytes();
    let serialized = serde_json::to_string(&keypair_bytes.to_vec())?;

    if outfile != "-" {
        if let Some(outdir) = Path::new(outfile).parent() {
            fs::create_dir_all(outdir)?;
        }
        let mut f = File::create(outfile)?;
        f.write_all(&serialized.clone().into_bytes())?;
    }
    Ok(serialized)
}

pub fn keypair_from_seed(seed: &[u8]) -> Result<Keypair, Box<dyn error::Error>> {
    if seed.len() < ed25519_dalek::SECRET_KEY_LENGTH {
        return Err("Seed is too short".into());
    }
    let secret = ed25519_dalek::SecretKey::from_bytes(&seed[..ed25519_dalek::SECRET_KEY_LENGTH])
        .map_err(|e| e.to_string())?;
    let public = ed25519_dalek::PublicKey::from(&secret);
    let keypair = Keypair { secret, public };
    Ok(keypair)
}

pub fn gen_keypair_file(outfile: &str) -> Result<String, Box<dyn error::Error>> {
    write_keypair(&Keypair::new(), outfile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    fn tmp_file_path(name: &str) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();

        format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey()).to_string()
    }

    #[test]
    fn test_gen_keypair_file() {
        let outfile = tmp_file_path("test_gen_keypair_file.json");
        let serialized_keypair = gen_keypair_file(&outfile).unwrap();
        let keypair_vec: Vec<u8> = serde_json::from_str(&serialized_keypair).unwrap();
        assert!(Path::new(&outfile).exists());
        assert_eq!(
            keypair_vec,
            read_keypair(&outfile).unwrap().to_bytes().to_vec()
        );
        assert_eq!(
            read_keypair(&outfile).unwrap().pubkey().as_ref().len(),
            mem::size_of::<Pubkey>()
        );
        fs::remove_file(&outfile).unwrap();
        assert!(!Path::new(&outfile).exists());
    }

    #[test]
    fn test_keypair_from_seed() {
        let good_seed = vec![0; 32];
        assert!(keypair_from_seed(&good_seed).is_ok());

        let too_short_seed = vec![0; 31];
        assert!(keypair_from_seed(&too_short_seed).is_err());
    }

    #[test]
    fn test_signature_fromstr() {
        let signature = Keypair::new().sign_message(&[0u8]);

        let mut signature_base58_str = bs58::encode(signature).into_string();

        assert_eq!(signature_base58_str.parse::<Signature>(), Ok(signature));

        signature_base58_str.push_str(&bs58::encode(signature.0).into_string());
        assert_eq!(
            signature_base58_str.parse::<Signature>(),
            Err(ParseSignatureError::WrongSize)
        );

        signature_base58_str.truncate(signature_base58_str.len() / 2);
        assert_eq!(signature_base58_str.parse::<Signature>(), Ok(signature));

        signature_base58_str.truncate(signature_base58_str.len() / 2);
        assert_eq!(
            signature_base58_str.parse::<Signature>(),
            Err(ParseSignatureError::WrongSize)
        );

        let mut signature_base58_str = bs58::encode(signature.0).into_string();
        assert_eq!(signature_base58_str.parse::<Signature>(), Ok(signature));

        // throw some non-base58 stuff in there
        signature_base58_str.replace_range(..1, "I");
        assert_eq!(
            signature_base58_str.parse::<Signature>(),
            Err(ParseSignatureError::Invalid)
        );
    }
}
