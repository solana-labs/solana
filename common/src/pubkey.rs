use bs58;
use generic_array::typenum::U32;
use generic_array::GenericArray;
use std::fmt;

#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey(GenericArray<u8, U32>);

impl Pubkey {
    pub fn new(pubkey_vec: &[u8]) -> Self {
        Pubkey(GenericArray::clone_from_slice(&pubkey_vec))
    }
}

impl AsRef<[u8]> for Pubkey {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

impl fmt::Display for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}
