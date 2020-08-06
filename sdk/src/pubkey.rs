#[cfg(not(feature = "program"))]
use crate::hash::Hasher;
use crate::{decode_error::DecodeError, hash::hashv};
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
    /// Length of the seed is too long for address generation
    #[error("Length of the seed is too long for address generation")]
    MaxSeedLengthExceeded,
    #[error("Provided seeds do not result in a valid address")]
    InvalidSeeds,
}
impl<T> DecodeError<T> for PubkeyError {
    fn type_of() -> &'static str {
        "PubkeyError"
    }
}

#[repr(transparent)]
#[derive(
    Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash, AbiExample,
)]
pub struct Pubkey([u8; 32]);

impl crate::sanitize::Sanitize for Pubkey {}

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
        owner: &Pubkey,
    ) -> Result<Pubkey, PubkeyError> {
        if seed.len() > MAX_SEED_LEN {
            return Err(PubkeyError::MaxSeedLengthExceeded);
        }

        Ok(Pubkey::new(
            hashv(&[base.as_ref(), seed.as_ref(), owner.as_ref()]).as_ref(),
        ))
    }

    /// Create a program address, valid program address must not be on the
    /// ed25519 curve
    #[cfg(not(feature = "program"))]
    pub fn create_program_address(
        seeds: &[&[u8]],
        program_id: &Pubkey,
    ) -> Result<Pubkey, PubkeyError> {
        let mut hasher = Hasher::default();
        for seed in seeds.iter() {
            if seed.len() > MAX_SEED_LEN {
                return Err(PubkeyError::MaxSeedLengthExceeded);
            }
            hasher.hash(seed);
        }
        hasher.hashv(&[program_id.as_ref(), "ProgramDerivedAddress".as_ref()]);
        let hash = hasher.result();

        if curve25519_dalek::edwards::CompressedEdwardsY::from_slice(hash.as_ref())
            .decompress()
            .is_some()
        {
            return Err(PubkeyError::InvalidSeeds);
        }

        Ok(Pubkey::new(hash.as_ref()))
    }

    /// Find a valid program address and its corresponding nonce which must be passed
    /// as an additional seed when calling `create_program_address`
    #[cfg(not(feature = "program"))]
    pub fn find_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> (Pubkey, u8) {
        let mut nonce = [255];
        for _ in 0..std::u8::MAX {
            {
                let mut seeds_with_nonce = seeds.to_vec();
                seeds_with_nonce.push(&nonce);
                if let Ok(address) = Self::create_program_address(&seeds_with_nonce, program_id) {
                    return (address, nonce[0]);
                }
            }
            nonce[0] -= 1;
        }
        panic!("Unable to find a viable program address nonce");
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
    fn test_create_program_address() {
        let exceeded_seed = &[127; MAX_SEED_LEN + 1];
        let max_seed = &[0; MAX_SEED_LEN];
        let program_id = Pubkey::from_str("BPFLoader1111111111111111111111111111111111").unwrap();
        let public_key = Pubkey::from_str("SeedPubey1111111111111111111111111111111111").unwrap();

        assert_eq!(
            Pubkey::create_program_address(&[exceeded_seed], &program_id),
            Err(PubkeyError::MaxSeedLengthExceeded)
        );
        assert_eq!(
            Pubkey::create_program_address(&[b"short_seed", exceeded_seed], &program_id),
            Err(PubkeyError::MaxSeedLengthExceeded)
        );
        assert!(Pubkey::create_program_address(&[max_seed], &program_id).is_ok());
        assert_eq!(
            Pubkey::create_program_address(&[b"", &[1]], &program_id),
            Ok("3gF2KMe9KiC6FNVBmfg9i267aMPvK37FewCip4eGBFcT"
                .parse()
                .unwrap())
        );
        assert_eq!(
            Pubkey::create_program_address(&["☉".as_ref()], &program_id),
            Ok("7ytmC1nT1xY4RfxCV2ZgyA7UakC93do5ZdyhdF3EtPj7"
                .parse()
                .unwrap())
        );
        assert_eq!(
            Pubkey::create_program_address(&[b"Talking", b"Squirrels"], &program_id),
            Ok("HwRVBufQ4haG5XSgpspwKtNd3PC9GM9m1196uJW36vds"
                .parse()
                .unwrap())
        );
        assert_eq!(
            Pubkey::create_program_address(&[public_key.as_ref()], &program_id),
            Ok("GUs5qLUfsEHkcMB9T38vjr18ypEhRuNWiePW2LoK4E3K"
                .parse()
                .unwrap())
        );
        assert_ne!(
            Pubkey::create_program_address(&[b"Talking", b"Squirrels"], &program_id).unwrap(),
            Pubkey::create_program_address(&[b"Talking"], &program_id).unwrap(),
        );
    }

    #[test]
    fn test_pubkey_off_curve() {
        // try a bunch of random input, all successful generated program
        // addresses must land off the curve and be unique
        let mut addresses = vec![];
        for _ in 0..1_000 {
            let program_id = Pubkey::new_rand();
            let bytes1 = rand::random::<[u8; 10]>();
            let bytes2 = rand::random::<[u8; 32]>();
            if let Ok(program_address) =
                Pubkey::create_program_address(&[&bytes1, &bytes2], &program_id)
            {
                let is_on_curve = curve25519_dalek::edwards::CompressedEdwardsY::from_slice(
                    &program_address.to_bytes(),
                )
                .decompress()
                .is_some();
                assert!(!is_on_curve);
                assert!(!addresses.contains(&program_address));
                addresses.push(program_address);
            }
        }
    }

    #[test]
    fn test_find_program_address() {
        for _ in 0..1_000 {
            let program_id = Pubkey::new_rand();
            let (address, nonce) = Pubkey::find_program_address(&[b"Lil'", b"Bits"], &program_id);
            assert_eq!(
                address,
                Pubkey::create_program_address(&[b"Lil'", b"Bits", &[nonce]], &program_id).unwrap()
            );
        }
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
