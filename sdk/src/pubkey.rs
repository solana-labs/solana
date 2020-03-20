use crate::{hash::hashv, program_utils::DecodeError};
use num_derive::{FromPrimitive, ToPrimitive};
#[cfg(not(feature = "program"))]
use std::error;
use std::{convert::TryFrom, fmt, mem, str::FromStr};
use thiserror::Error;

pub use bs58;

/// maximum length of derived pubkey seed
pub const MAX_SEED_LEN: usize = 32;

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum PubkeyError {
    #[error("length of requested seed is too long")]
    MaxSeedLengthExceeded,
}
impl<T> DecodeError<T> for PubkeyError {
    fn type_of() -> &'static str {
        "PubkeyError"
    }
}

#[repr(transparent)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey([u8; 32]);

#[derive(Error, Debug, Serialize, Clone, PartialEq, FromPrimitive, ToPrimitive)]
pub enum ParsePubkeyError {
    #[error("String is the wrong size")]
    WrongSize,
    #[error("Invalid Base58 string")]
    Invalid,
}
impl<T> DecodeError<T> for ParsePubkeyError {
    fn type_of() -> &'static str {
        "ParsePubkeyError"
    }
}

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
    pub fn new(pubkey_vec: &[u8]) -> Self {
        Self(
            <[u8; 32]>::try_from(<&[u8]>::clone(&pubkey_vec))
                .expect("Slice must be the same length as a Pubkey"),
        )
    }

    pub const fn new_from_array(pubkey_array: [u8; 32]) -> Self {
        Self(pubkey_array)
    }

    pub fn create_with_seed(
        base: &Pubkey,
        seed: &str,
        program_id: &Pubkey,
    ) -> Result<Pubkey, PubkeyError> {
        if seed.len() > MAX_SEED_LEN {
            return Err(PubkeyError::MaxSeedLengthExceeded);
        }

        Ok(Pubkey::new(
            hashv(&[base.as_ref(), seed.as_ref(), program_id.as_ref()]).as_ref(),
        ))
    }

    #[cfg(not(feature = "program"))]
    pub fn new_rand() -> Self {
        Self::new(&rand::random::<[u8; 32]>())
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0
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

#[cfg(not(feature = "program"))]
pub fn write_pubkey_file(outfile: &str, pubkey: Pubkey) -> Result<(), Box<dyn error::Error>> {
    use std::io::Write;

    let printable = format!("{}", pubkey);
    let serialized = serde_json::to_string(&printable)?;

    if let Some(outdir) = std::path::Path::new(&outfile).parent() {
        std::fs::create_dir_all(outdir)?;
    }
    let mut f = std::fs::File::create(outfile)?;
    f.write_all(&serialized.into_bytes())?;

    Ok(())
}

#[cfg(not(feature = "program"))]
pub fn read_pubkey_file(infile: &str) -> Result<Pubkey, Box<dyn error::Error>> {
    let f = std::fs::File::open(infile.to_string())?;
    let printable: String = serde_json::from_reader(f)?;
    Ok(Pubkey::from_str(&printable)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs::remove_file, str::from_utf8};

    #[test]
    fn pubkey_fromstr() {
        let pubkey = Pubkey::new_rand();
        let mut pubkey_base58_str = bs58::encode(pubkey.0).into_string();

        assert_eq!(pubkey_base58_str.parse::<Pubkey>(), Ok(pubkey));

        pubkey_base58_str.push_str(&bs58::encode(pubkey.0).into_string());
        assert_eq!(
            pubkey_base58_str.parse::<Pubkey>(),
            Err(ParsePubkeyError::WrongSize)
        );

        pubkey_base58_str.truncate(pubkey_base58_str.len() / 2);
        assert_eq!(pubkey_base58_str.parse::<Pubkey>(), Ok(pubkey));

        pubkey_base58_str.truncate(pubkey_base58_str.len() / 2);
        assert_eq!(
            pubkey_base58_str.parse::<Pubkey>(),
            Err(ParsePubkeyError::WrongSize)
        );

        let mut pubkey_base58_str = bs58::encode(pubkey.0).into_string();
        assert_eq!(pubkey_base58_str.parse::<Pubkey>(), Ok(pubkey));

        // throw some non-base58 stuff in there
        pubkey_base58_str.replace_range(..1, "I");
        assert_eq!(
            pubkey_base58_str.parse::<Pubkey>(),
            Err(ParsePubkeyError::Invalid)
        );
    }

    #[test]
    fn test_create_with_seed() {
        assert!(Pubkey::create_with_seed(&Pubkey::new_rand(), "☉", &Pubkey::new_rand()).is_ok());
        assert_eq!(
            Pubkey::create_with_seed(
                &Pubkey::new_rand(),
                from_utf8(&[127; MAX_SEED_LEN + 1]).unwrap(),
                &Pubkey::new_rand()
            ),
            Err(PubkeyError::MaxSeedLengthExceeded)
        );
        assert!(Pubkey::create_with_seed(
            &Pubkey::new_rand(),
            "\
             \u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\
             ",
            &Pubkey::new_rand()
        )
        .is_ok());
        // utf-8 abuse ;)
        assert_eq!(
            Pubkey::create_with_seed(
                &Pubkey::new_rand(),
                "\
                 x\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\
                 ",
                &Pubkey::new_rand()
            ),
            Err(PubkeyError::MaxSeedLengthExceeded)
        );

        assert!(Pubkey::create_with_seed(
            &Pubkey::new_rand(),
            std::str::from_utf8(&[0; MAX_SEED_LEN]).unwrap(),
            &Pubkey::new_rand(),
        )
        .is_ok());

        assert!(Pubkey::create_with_seed(&Pubkey::new_rand(), "", &Pubkey::new_rand(),).is_ok());

        assert_eq!(
            Pubkey::create_with_seed(
                &Pubkey::default(),
                "limber chicken: 4/45",
                &Pubkey::default(),
            ),
            Ok("9h1HyLCW5dZnBVap8C5egQ9Z6pHyjsh5MNy83iPqqRuq"
                .parse()
                .unwrap())
        );
    }

    #[test]
    fn test_read_write_pubkey() -> Result<(), Box<dyn error::Error>> {
        let filename = "test_pubkey.json";
        let pubkey = Pubkey::new_rand();
        write_pubkey_file(filename, pubkey)?;
        let read = read_pubkey_file(filename)?;
        assert_eq!(read, pubkey);
        remove_file(filename)?;
        Ok(())
    }
}
