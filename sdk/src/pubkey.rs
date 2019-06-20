use generic_array::typenum::U32;
use generic_array::GenericArray;
use std::error;
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::mem;
use std::path::Path;
use std::str::FromStr;

#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Pubkey(GenericArray<u8, U32>);

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
    pub fn new(pubkey_vec: &[u8]) -> Self {
        Pubkey(GenericArray::clone_from_slice(&pubkey_vec))
    }

    pub fn new_rand() -> Self {
        Self::new(&rand::random::<[u8; 32]>())
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

pub fn write_pubkey(outfile: &str, pubkey: Pubkey) -> Result<(), Box<error::Error>> {
    let printable = format!("{}", pubkey);
    let serialized = serde_json::to_string(&printable)?;

    if let Some(outdir) = Path::new(&outfile).parent() {
        fs::create_dir_all(outdir)?;
    }
    let mut f = File::create(outfile)?;
    f.write_all(&serialized.clone().into_bytes())?;

    Ok(())
}

pub fn read_pubkey(infile: &str) -> Result<Pubkey, Box<error::Error>> {
    let f = File::open(infile.to_string())?;
    let printable: String = serde_json::from_reader(f)?;
    Ok(Pubkey::from_str(&printable)?)
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
            // un-comment me to see what the id should look like, given a name
            //  if id().to_string() != $name {
            //      panic!("id for `{}` should be `{:?}`", $name, bs58::decode($name).into_vec().unwrap());
            //  }
            assert_eq!(id().to_string(), $name)
        }
    )
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::remove_file;

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
    fn test_read_write_pubkey() -> Result<(), Box<error::Error>> {
        let filename = "test_pubkey.json";
        let pubkey = Pubkey::new_rand();
        write_pubkey(filename, pubkey)?;
        let read = read_pubkey(filename)?;
        assert_eq!(read, pubkey);
        remove_file(filename)?;
        Ok(())
    }
}
