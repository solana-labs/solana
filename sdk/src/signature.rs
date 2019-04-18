//! The `signature` module provides functionality for public, and private keys.

use crate::pubkey::Pubkey;
use bs58;
use generic_array::typenum::U64;
use generic_array::GenericArray;
use ring::signature::Ed25519KeyPair;
use ring::{rand, signature};
use serde_json;
use std::error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use untrusted::Input;

pub type Keypair = Ed25519KeyPair;

#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Signature(GenericArray<u8, U64>);

impl Signature {
    pub fn new(signature_slice: &[u8]) -> Self {
        Self(GenericArray::clone_from_slice(&signature_slice))
    }

    pub fn verify(&self, pubkey_bytes: &[u8], message_bytes: &[u8]) -> bool {
        let pubkey = Input::from(pubkey_bytes);
        let message = Input::from(message_bytes);
        let signature = Input::from(self.0.as_slice());
        signature::verify(&signature::ED25519, pubkey, message, signature).is_ok()
    }
}

pub trait Signable {
    fn sign(&mut self, keypair: &Keypair) {
        let data = self.signable_data();
        self.set_signature(keypair.sign_message(&data));
    }
    fn verify(&self) -> bool {
        self.get_signature()
            .verify(&self.pubkey().as_ref(), &self.signable_data())
    }

    fn pubkey(&self) -> Pubkey;
    fn signable_data(&self) -> Vec<u8>;
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

pub trait KeypairUtil {
    fn new() -> Self;
    fn pubkey(&self) -> Pubkey;
    fn sign_message(&self, message: &[u8]) -> Signature;
}

impl KeypairUtil for Ed25519KeyPair {
    /// Return a new ED25519 keypair
    fn new() -> Self {
        let rng = rand::SystemRandom::new();
        let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rng).expect("generate_pkcs8");
        Ed25519KeyPair::from_pkcs8(Input::from(&pkcs8_bytes)).expect("from_pcks8")
    }

    /// Return the public key for the given keypair
    fn pubkey(&self) -> Pubkey {
        Pubkey::new(self.public_key_bytes())
    }

    fn sign_message(&self, message: &[u8]) -> Signature {
        Signature::new(self.sign(message).as_ref())
    }
}

pub fn read_pkcs8(path: &str) -> Result<Vec<u8>, Box<error::Error>> {
    let file = File::open(path.to_string())?;
    let pkcs8: Vec<u8> = serde_json::from_reader(file)?;
    Ok(pkcs8)
}

pub fn read_keypair(path: &str) -> Result<Keypair, Box<error::Error>> {
    let pkcs8 = read_pkcs8(path)?;
    let keypair = Ed25519KeyPair::from_pkcs8(Input::from(&pkcs8))?;
    Ok(keypair)
}

pub fn gen_pkcs8() -> Result<Vec<u8>, Box<error::Error>> {
    let rnd = rand::SystemRandom::new();
    let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rnd)?;
    Ok(pkcs8_bytes.to_vec())
}

//pub fn gen_keypair_file(outfile: String) -> Result<String, Box<dyn error::Error>> {
pub fn gen_keypair_file(outfile: String) -> Result<String, Box<error::Error>> {
    let serialized = serde_json::to_string(&gen_pkcs8()?)?;

    if outfile != "-" {
        if let Some(outdir) = Path::new(&outfile).parent() {
            fs::create_dir_all(outdir)?;
        }
        let mut f = File::create(outfile)?;
        f.write_all(&serialized.clone().into_bytes())?;
    }
    Ok(serialized)
}
