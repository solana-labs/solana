use {
    sha2::{Digest, Sha256},
    std::fmt,
};

const HASH_BYTES: usize = 32;
#[derive(AbiExample)]
pub struct Hash(pub [u8; HASH_BYTES]);

#[derive(Default)]
pub struct Hasher {
    hasher: Sha256,
}

impl Hasher {
    pub fn hash(&mut self, val: &[u8]) {
        self.hasher.update(val);
    }
    pub fn result(self) -> Hash {
        Hash(self.hasher.finalize().into())
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}
