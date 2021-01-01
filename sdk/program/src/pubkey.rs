use crate::{decode_error::DecodeError, hash::hashv};
use num_derive::{FromPrimitive, ToPrimitive};
use std::{convert::TryFrom, fmt, mem, str::FromStr};
use thiserror::Error;

/// Number of bytes in a pubkey
pub const PUBKEY_BYTES: usize = 32;
/// maximum length of derived `Pubkey` seed
pub const MAX_SEED_LEN: usize = 32;
/// Maximum number of seeds
pub const MAX_SEEDS: usize = 16;

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
impl From<u64> for PubkeyError {
    fn from(error: u64) -> Self {
        match error {
            0 => PubkeyError::MaxSeedLengthExceeded,
            1 => PubkeyError::InvalidSeeds,
            _ => panic!("Unsupported PubkeyError"),
        }
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

    #[deprecated(since = "1.3.9", note = "Please use 'Pubkey::new_unique' instead")]
    #[cfg(not(target_arch = "bpf"))]
    pub fn new_rand() -> Self {
        // Consider removing Pubkey::new_rand() entirely in the v1.5 or v1.6 timeframe
        Pubkey::new(&rand::random::<[u8; 32]>())
    }

    /// unique Pubkey for tests and benchmarks.
    pub fn new_unique() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static I: AtomicU64 = AtomicU64::new(1);

        let mut b = [0u8; 32];
        let i = I.fetch_add(1, Ordering::Relaxed);
        b[0..8].copy_from_slice(&i.to_le_bytes());
        Self::new(&b)
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

    /// Create a program address
    ///
    /// Program addresses are account keys that only the program has the
    /// authority to sign.  The address is of the same form as a Solana
    /// `Pubkey`, except they are ensured to not be on the ed25519 curve and
    /// thus have no associated private key.  When performing cross-program
    /// invocations the program can "sign" for the key by calling
    /// `invoke_signed` and passing the same seeds used to generate the address.
    /// The runtime will check that indeed the program associated with this
    /// address is the caller and thus authorized to be the signer.
    ///
    /// Because the program address cannot lie on the ed25519 curve there may be
    /// seed and program id combinations that are invalid.  In these cases an
    /// extra seed (bump seed) can be calculated that results in a point off the
    /// curve.  Use `find_program_address` to calculate that bump seed.
    ///
    /// Warning: Because of the way the seeds are hashed there is a potential
    /// for program address collisions for the same program id.  The seeds are
    /// hashed sequentially which means that seeds {"abcdef"}, {"abc", "def"},
    /// and {"ab", "cd", "ef"} will all result in the same program address given
    /// the same program id.  Since the change of collision is local to a given
    /// program id the developer of that program must take care to choose seeds
    /// that do not collide with themselves.
    pub fn create_program_address(
        seeds: &[&[u8]],
        program_id: &Pubkey,
    ) -> Result<Pubkey, PubkeyError> {
        if seeds.len() > MAX_SEEDS {
            return Err(PubkeyError::MaxSeedLengthExceeded);
        }
        for seed in seeds.iter() {
            if seed.len() > MAX_SEED_LEN {
                return Err(PubkeyError::MaxSeedLengthExceeded);
            }
        }

        // Perform the calculation inline, calling this from within a program is
        // not supported
        #[cfg(not(target_arch = "bpf"))]
        {
            let mut hasher = crate::hash::Hasher::default();
            for seed in seeds.iter() {
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
        // Call via a system call to perform the calculation
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_create_program_address(
                    seeds_addr: *const u8,
                    seeds_len: u64,
                    program_id_addr: *const u8,
                    address_bytes_addr: *const u8,
                ) -> u64;
            };
            let mut bytes = [0; 32];
            let result = unsafe {
                sol_create_program_address(
                    seeds as *const _ as *const u8,
                    seeds.len() as u64,
                    program_id as *const _ as *const u8,
                    &mut bytes as *mut _ as *mut u8,
                )
            };
            match result {
                crate::entrypoint::SUCCESS => Ok(Pubkey::new(&bytes)),
                _ => Err(result.into()),
            }
        }
    }

    /// Find a valid program address and its corresponding bump seed which must
    /// be passed as an additional seed when calling `invoke_signed`.
    ///
    /// Panics in the very unlikely event that the additional seed could not be
    /// found.
    pub fn find_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> (Pubkey, u8) {
        Self::try_find_program_address(seeds, program_id)
            .unwrap_or_else(|| panic!("Unable to find a viable program address bump seed"))
    }

    /// Find a valid program address and its corresponding bump seed which must
    /// be passed as an additional seed when calling `invoke_signed`
    #[allow(clippy::same_item_push)]
    pub fn try_find_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> Option<(Pubkey, u8)> {
        // Perform the calculation inline, calling this from within a program is
        // not supported
        #[cfg(not(target_arch = "bpf"))]
        {
            let mut bump_seed = [std::u8::MAX];
            for _ in 0..std::u8::MAX {
                {
                    let mut seeds_with_bump = seeds.to_vec();
                    seeds_with_bump.push(&bump_seed);
                    if let Ok(address) = Self::create_program_address(&seeds_with_bump, program_id)
                    {
                        return Some((address, bump_seed[0]));
                    }
                }
                bump_seed[0] -= 1;
            }
            None
        }
        // Call via a system call to perform the calculation
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_try_find_program_address(
                    seeds_addr: *const u8,
                    seeds_len: u64,
                    program_id_addr: *const u8,
                    address_bytes_addr: *const u8,
                    bump_seed_addr: *const u8,
                ) -> u64;
            };
            let mut bytes = [0; 32];
            let mut bump_seed = std::u8::MAX;
            let result = unsafe {
                sol_try_find_program_address(
                    seeds as *const _ as *const u8,
                    seeds.len() as u64,
                    program_id as *const _ as *const u8,
                    &mut bytes as *mut _ as *mut u8,
                    &mut bump_seed as *mut _ as *mut u8,
                )
            };
            match result {
                crate::entrypoint::SUCCESS => Some((Pubkey::new(&bytes), bump_seed)),
                _ => None,
            }
        }
    }

    pub fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Log a `Pubkey` from a program
    pub fn log(&self) {
        #[cfg(target_arch = "bpf")]
        {
            extern "C" {
                fn sol_log_pubkey(pubkey_addr: *const u8);
            };
            unsafe { sol_log_pubkey(self.as_ref() as *const _ as *const u8) };
        }

        #[cfg(not(target_arch = "bpf"))]
        crate::program_stubs::sol_log(&self.to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::from_utf8;

    #[test]
    fn test_new_unique() {
        assert!(Pubkey::new_unique() != Pubkey::new_unique());
    }

    #[test]
    fn pubkey_fromstr() {
        let pubkey = Pubkey::new_unique();
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
        assert!(
            Pubkey::create_with_seed(&Pubkey::new_unique(), "☉", &Pubkey::new_unique()).is_ok()
        );
        assert_eq!(
            Pubkey::create_with_seed(
                &Pubkey::new_unique(),
                from_utf8(&[127; MAX_SEED_LEN + 1]).unwrap(),
                &Pubkey::new_unique()
            ),
            Err(PubkeyError::MaxSeedLengthExceeded)
        );
        assert!(Pubkey::create_with_seed(
            &Pubkey::new_unique(),
            "\
             \u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\
             ",
            &Pubkey::new_unique()
        )
        .is_ok());
        // utf-8 abuse ;)
        assert_eq!(
            Pubkey::create_with_seed(
                &Pubkey::new_unique(),
                "\
                 x\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\u{10FFFF}\
                 ",
                &Pubkey::new_unique()
            ),
            Err(PubkeyError::MaxSeedLengthExceeded)
        );

        assert!(Pubkey::create_with_seed(
            &Pubkey::new_unique(),
            std::str::from_utf8(&[0; MAX_SEED_LEN]).unwrap(),
            &Pubkey::new_unique(),
        )
        .is_ok());

        assert!(
            Pubkey::create_with_seed(&Pubkey::new_unique(), "", &Pubkey::new_unique(),).is_ok()
        );

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
            let program_id = Pubkey::new_unique();
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
            let program_id = Pubkey::new_unique();
            let (address, bump_seed) =
                Pubkey::find_program_address(&[b"Lil'", b"Bits"], &program_id);
            assert_eq!(
                address,
                Pubkey::create_program_address(&[b"Lil'", b"Bits", &[bump_seed]], &program_id)
                    .unwrap()
            );
        }
    }
}
