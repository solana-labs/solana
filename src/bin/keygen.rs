extern crate ring;
extern crate serde_json;

use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use std::error;

fn main() -> Result<(), Box<error::Error>> {
    let rnd = SystemRandom::new();
    let pkcs8_bytes = Ed25519KeyPair::generate_pkcs8(&rnd)?;
    let serialized = serde_json::to_string(&pkcs8_bytes.to_vec())?;
    println!("{}", serialized);
    Ok(())
}
