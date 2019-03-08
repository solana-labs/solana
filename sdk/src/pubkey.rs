use bs58;
use generic_array::typenum::U32;
use generic_array::GenericArray;
use std::fmt;
use std::mem;
use std::str::FromStr;

#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey(GenericArray<u8, U32>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsePubkeyError {
    TooShort,
    Invalid,
}

impl FromStr for Pubkey {
    type Err = ParsePubkeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pubkey_vec = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParsePubkeyError::Invalid)?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            Err(ParsePubkeyError::TooShort)
        } else {
            Ok(Pubkey::new(&pubkey_vec))
        }
    }
}

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
