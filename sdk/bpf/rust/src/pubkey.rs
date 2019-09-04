//! @brief Solana public key

use crate::log::sol_log_64;

use std::convert::TryFrom;
use std::error;
use std::fmt;
use std::mem;
use std::str::FromStr;

pub use bs58;

/// Public key
#[repr(transparent)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey([u8; 32]);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsePubkeyError {
    WrongSize,
    Invalid,
}

impl fmt::Display for ParsePubkeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParsePubkeyError: {:?}", self)
    }
}

impl error::Error for ParsePubkeyError {}

impl FromStr for Pubkey {
    type Err = ParsePubkeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pubkey_vec = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParsePubkeyError::Invalid)?;
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            Err(ParsePubkeyError::WrongSize)
        } else {
            Ok(Pubkey::new(&pubkey_vec))
        }
    }
}

impl Pubkey {
    pub fn new(slice: &[u8]) -> Self {
        Self(
            <[u8; 32]>::try_from(<&[u8]>::clone(&slice))
                .expect("Slice must be the same length as a Pubkey"),
        )
    }

    pub fn print(&self) {
        for (i, k) in self.0.iter().enumerate() {
            sol_log_64(0, 0, 0, i as u64, u64::from(*k));
        }
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

#[macro_export]
macro_rules! solana_id(
    ($id:ident) => (

        pub fn check_id(id: &$crate::pubkey::Pubkey) -> bool {
            id.as_ref() == $id
        }

        pub fn id() -> $crate::pubkey::Pubkey {
            $crate::pubkey::Pubkey::new(&$id)
        }

        #[cfg(test)]
        #[test]
        fn test_id() {
            assert!(check_id(&id()));
        }

    )
);

#[macro_export]
macro_rules! solana_name_id(
    ($id:ident, $name:expr) => (

        $crate::solana_id!($id);

        #[cfg(test)]
        #[test]
        fn test_name_id() {
            if id().to_string() != $name {
                panic!("id for `{}` should be `{:?}`", $name, $crate::pubkey::bs58::decode($name).into_vec().unwrap());
            }
        }
    )
);
