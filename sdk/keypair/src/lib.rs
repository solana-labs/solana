//! Concrete implementation of a Solana `Signer` from raw bytes
#![cfg_attr(docsrs, feature(doc_auto_cfg))]
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
use {
    ed25519_dalek::Signer as DalekSigner,
    rand0_7::{rngs::OsRng, CryptoRng, RngCore},
    solana_pubkey::Pubkey,
    solana_seed_phrase::generate_seed_from_seed_phrase_and_passphrase,
    solana_signature::Signature,
    solana_signer::{EncodableKey, EncodableKeypair, Signer, SignerError},
    std::{
        error,
        io::{Read, Write},
        path::Path,
    },
};

#[cfg(feature = "seed-derivable")]
pub mod seed_derivable;

/// A vanilla Ed25519 key pair
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Debug)]
pub struct Keypair(ed25519_dalek::Keypair);

impl Keypair {
    /// Can be used for generating a Keypair without a dependency on `rand` types
    pub const SECRET_KEY_LENGTH: usize = 32;

    /// Constructs a new, random `Keypair` using a caller-provided RNG
    pub fn generate<R>(csprng: &mut R) -> Self
    where
        R: CryptoRng + RngCore,
    {
        Self(ed25519_dalek::Keypair::generate(csprng))
    }

    /// Constructs a new, random `Keypair` using `OsRng`
    pub fn new() -> Self {
        let mut rng = OsRng;
        Self::generate(&mut rng)
    }

    /// Recovers a `Keypair` from a byte array
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ed25519_dalek::SignatureError> {
        if bytes.len() < ed25519_dalek::KEYPAIR_LENGTH {
            return Err(ed25519_dalek::SignatureError::from_source(String::from(
                "candidate keypair byte array is too short",
            )));
        }
        let secret =
            ed25519_dalek::SecretKey::from_bytes(&bytes[..ed25519_dalek::SECRET_KEY_LENGTH])?;
        let public =
            ed25519_dalek::PublicKey::from_bytes(&bytes[ed25519_dalek::SECRET_KEY_LENGTH..])?;
        let expected_public = ed25519_dalek::PublicKey::from(&secret);
        (public == expected_public)
            .then_some(Self(ed25519_dalek::Keypair { secret, public }))
            .ok_or(ed25519_dalek::SignatureError::from_source(String::from(
                "keypair bytes do not specify same pubkey as derived from their secret key",
            )))
    }

    /// Returns this `Keypair` as a byte array
    pub fn to_bytes(&self) -> [u8; 64] {
        self.0.to_bytes()
    }

    /// Recovers a `Keypair` from a base58-encoded string
    pub fn from_base58_string(s: &str) -> Self {
        let mut buf = [0u8; ed25519_dalek::KEYPAIR_LENGTH];
        bs58::decode(s).onto(&mut buf).unwrap();
        Self::from_bytes(&buf).unwrap()
    }

    /// Returns this `Keypair` as a base58-encoded string
    pub fn to_base58_string(&self) -> String {
        bs58::encode(&self.0.to_bytes()).into_string()
    }

    /// Gets this `Keypair`'s SecretKey
    pub fn secret(&self) -> &ed25519_dalek::SecretKey {
        &self.0.secret
    }

    /// Allows Keypair cloning
    ///
    /// Note that the `Clone` trait is intentionally unimplemented because making a
    /// second copy of sensitive secret keys in memory is usually a bad idea.
    ///
    /// Only use this in tests or when strictly required. Consider using [`std::sync::Arc<Keypair>`]
    /// instead.
    pub fn insecure_clone(&self) -> Self {
        Self(ed25519_dalek::Keypair {
            // This will never error since self is a valid keypair
            secret: ed25519_dalek::SecretKey::from_bytes(self.0.secret.as_bytes()).unwrap(),
            public: self.0.public,
        })
    }
}

#[cfg(target_arch = "wasm32")]
#[allow(non_snake_case)]
#[wasm_bindgen]
impl Keypair {
    /// Create a new `Keypair `
    #[wasm_bindgen(constructor)]
    pub fn constructor() -> Keypair {
        Keypair::new()
    }

    /// Convert a `Keypair` to a `Uint8Array`
    pub fn toBytes(&self) -> Box<[u8]> {
        self.to_bytes().into()
    }

    /// Recover a `Keypair` from a `Uint8Array`
    pub fn fromBytes(bytes: &[u8]) -> Result<Keypair, JsValue> {
        Keypair::from_bytes(bytes).map_err(|e| e.to_string().into())
    }

    /// Return the `Pubkey` for this `Keypair`
    #[wasm_bindgen(js_name = pubkey)]
    pub fn js_pubkey(&self) -> Pubkey {
        // `wasm_bindgen` does not support traits (`Signer) yet
        self.pubkey()
    }
}

impl From<ed25519_dalek::Keypair> for Keypair {
    fn from(value: ed25519_dalek::Keypair) -> Self {
        Self(value)
    }
}

#[cfg(test)]
static_assertions::const_assert_eq!(Keypair::SECRET_KEY_LENGTH, ed25519_dalek::SECRET_KEY_LENGTH);

impl Signer for Keypair {
    #[inline]
    fn pubkey(&self) -> Pubkey {
        Pubkey::from(self.0.public.to_bytes())
    }

    fn try_pubkey(&self) -> Result<Pubkey, SignerError> {
        Ok(self.pubkey())
    }

    fn sign_message(&self, message: &[u8]) -> Signature {
        Signature::from(self.0.sign(message).to_bytes())
    }

    fn try_sign_message(&self, message: &[u8]) -> Result<Signature, SignerError> {
        Ok(self.sign_message(message))
    }

    fn is_interactive(&self) -> bool {
        false
    }
}

impl<T> PartialEq<T> for Keypair
where
    T: Signer,
{
    fn eq(&self, other: &T) -> bool {
        self.pubkey() == other.pubkey()
    }
}

impl EncodableKey for Keypair {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Box<dyn error::Error>> {
        read_keypair(reader)
    }

    fn write<W: Write>(&self, writer: &mut W) -> Result<String, Box<dyn error::Error>> {
        write_keypair(self, writer)
    }
}

impl EncodableKeypair for Keypair {
    type Pubkey = Pubkey;

    /// Returns the associated pubkey. Use this function specifically for settings that involve
    /// reading or writing pubkeys. For other settings, use `Signer::pubkey()` instead.
    fn encodable_pubkey(&self) -> Self::Pubkey {
        self.pubkey()
    }
}

/// Reads a JSON-encoded `Keypair` from a `Reader` implementor
pub fn read_keypair<R: Read>(reader: &mut R) -> Result<Keypair, Box<dyn error::Error>> {
    let mut buffer = String::new();
    reader.read_to_string(&mut buffer)?;
    let trimmed = buffer.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Input must be a JSON array",
        )
        .into());
    }
    // we already checked that the string has at least two chars,
    // so 1..trimmed.len() - 1 won't be out of bounds
    #[allow(clippy::arithmetic_side_effects)]
    let contents = &trimmed[1..trimmed.len() - 1];
    let elements_vec: Vec<&str> = contents.split(',').map(|s| s.trim()).collect();
    let len = elements_vec.len();
    let elements: [&str; ed25519_dalek::KEYPAIR_LENGTH] =
        elements_vec.try_into().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Expected {} elements, found {}",
                    ed25519_dalek::KEYPAIR_LENGTH,
                    len
                ),
            )
        })?;
    let mut out = [0u8; ed25519_dalek::KEYPAIR_LENGTH];
    for (idx, element) in elements.into_iter().enumerate() {
        let parsed: u8 = element.parse()?;
        out[idx] = parsed;
    }
    Keypair::from_bytes(&out)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()).into())
}

/// Reads a `Keypair` from a file
pub fn read_keypair_file<F: AsRef<Path>>(path: F) -> Result<Keypair, Box<dyn error::Error>> {
    Keypair::read_from_file(path)
}

/// Writes a `Keypair` to a `Write` implementor with JSON-encoding
pub fn write_keypair<W: Write>(
    keypair: &Keypair,
    writer: &mut W,
) -> Result<String, Box<dyn error::Error>> {
    let keypair_bytes = keypair.0.to_bytes();
    let mut result = Vec::with_capacity(64 * 4 + 2); // Estimate capacity: 64 numbers * (up to 3 digits + 1 comma) + 2 brackets

    result.push(b'['); // Opening bracket

    for (i, &num) in keypair_bytes.iter().enumerate() {
        if i > 0 {
            result.push(b','); // Comma separator for all elements except the first
        }

        // Convert number to string and then to bytes
        let num_str = num.to_string();
        result.extend_from_slice(num_str.as_bytes());
    }

    result.push(b']'); // Closing bracket
    writer.write_all(&result)?;
    let as_string = String::from_utf8(result)?;
    Ok(as_string)
}

/// Writes a `Keypair` to a file with JSON-encoding
pub fn write_keypair_file<F: AsRef<Path>>(
    keypair: &Keypair,
    outfile: F,
) -> Result<String, Box<dyn error::Error>> {
    keypair.write_to_file(outfile)
}

/// Constructs a `Keypair` from caller-provided seed entropy
pub fn keypair_from_seed(seed: &[u8]) -> Result<Keypair, Box<dyn error::Error>> {
    if seed.len() < ed25519_dalek::SECRET_KEY_LENGTH {
        return Err("Seed is too short".into());
    }
    let secret = ed25519_dalek::SecretKey::from_bytes(&seed[..ed25519_dalek::SECRET_KEY_LENGTH])
        .map_err(|e| e.to_string())?;
    let public = ed25519_dalek::PublicKey::from(&secret);
    let dalek_keypair = ed25519_dalek::Keypair { secret, public };
    Ok(Keypair(dalek_keypair))
}

pub fn keypair_from_seed_phrase_and_passphrase(
    seed_phrase: &str,
    passphrase: &str,
) -> Result<Keypair, Box<dyn std::error::Error>> {
    keypair_from_seed(&generate_seed_from_seed_phrase_and_passphrase(
        seed_phrase,
        passphrase,
    ))
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bip39::{Language, Mnemonic, MnemonicType, Seed},
        solana_signer::unique_signers,
        std::{
            fs::{self, File},
            mem,
        },
    };

    fn tmp_file_path(name: &str) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();

        format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey())
    }

    #[test]
    fn test_write_keypair_file() {
        let outfile = tmp_file_path("test_write_keypair_file.json");
        let serialized_keypair = write_keypair_file(&Keypair::new(), &outfile).unwrap();
        let keypair_vec: Vec<u8> = serde_json::from_str(&serialized_keypair).unwrap();
        assert!(Path::new(&outfile).exists());
        assert_eq!(
            keypair_vec,
            read_keypair_file(&outfile).unwrap().0.to_bytes().to_vec()
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                File::open(&outfile)
                    .expect("open")
                    .metadata()
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }

        assert_eq!(
            read_keypair_file(&outfile).unwrap().pubkey().as_ref().len(),
            mem::size_of::<Pubkey>()
        );
        fs::remove_file(&outfile).unwrap();
        assert!(!Path::new(&outfile).exists());
    }

    #[test]
    fn test_write_keypair_file_overwrite_ok() {
        let outfile = tmp_file_path("test_write_keypair_file_overwrite_ok.json");

        write_keypair_file(&Keypair::new(), &outfile).unwrap();
        write_keypair_file(&Keypair::new(), &outfile).unwrap();
    }

    #[test]
    fn test_write_keypair_file_truncate() {
        let outfile = tmp_file_path("test_write_keypair_file_truncate.json");

        write_keypair_file(&Keypair::new(), &outfile).unwrap();
        read_keypair_file(&outfile).unwrap();

        // Ensure outfile is truncated
        {
            let mut f = File::create(&outfile).unwrap();
            f.write_all(String::from_utf8([b'a'; 2048].to_vec()).unwrap().as_bytes())
                .unwrap();
        }
        write_keypair_file(&Keypair::new(), &outfile).unwrap();
        read_keypair_file(&outfile).unwrap();
    }

    #[test]
    fn test_keypair_from_seed() {
        let good_seed = vec![0; 32];
        assert!(keypair_from_seed(&good_seed).is_ok());

        let too_short_seed = vec![0; 31];
        assert!(keypair_from_seed(&too_short_seed).is_err());
    }

    #[test]
    fn test_keypair() {
        let keypair = keypair_from_seed(&[0u8; 32]).unwrap();
        let pubkey = keypair.pubkey();
        let data = [1u8];
        let sig = keypair.sign_message(&data);

        // Signer
        assert_eq!(keypair.try_pubkey().unwrap(), pubkey);
        assert_eq!(keypair.pubkey(), pubkey);
        assert_eq!(keypair.try_sign_message(&data).unwrap(), sig);
        assert_eq!(keypair.sign_message(&data), sig);

        // PartialEq
        let keypair2 = keypair_from_seed(&[0u8; 32]).unwrap();
        assert_eq!(keypair, keypair2);
    }

    fn pubkeys(signers: &[&dyn Signer]) -> Vec<Pubkey> {
        signers.iter().map(|x| x.pubkey()).collect()
    }

    #[test]
    fn test_unique_signers() {
        let alice = Keypair::new();
        let bob = Keypair::new();
        assert_eq!(
            pubkeys(&unique_signers(vec![&alice, &bob, &alice])),
            pubkeys(&[&alice, &bob])
        );
    }

    #[test]
    fn test_containers() {
        use std::{rc::Rc, sync::Arc};

        struct Foo<S: Signer> {
            #[allow(unused)]
            signer: S,
        }

        fn foo(_s: impl Signer) {}

        let _arc_signer = Foo {
            signer: Arc::new(Keypair::new()),
        };
        foo(Arc::new(Keypair::new()));

        let _rc_signer = Foo {
            signer: Rc::new(Keypair::new()),
        };
        foo(Rc::new(Keypair::new()));

        let _ref_signer = Foo {
            signer: &Keypair::new(),
        };
        foo(Keypair::new());

        let _box_signer = Foo {
            signer: Box::new(Keypair::new()),
        };
        foo(Box::new(Keypair::new()));

        let _signer = Foo {
            signer: Keypair::new(),
        };
        foo(Keypair::new());
    }

    #[test]
    fn test_keypair_from_seed_phrase_and_passphrase() {
        let mnemonic = Mnemonic::new(MnemonicType::Words12, Language::English);
        let passphrase = "42";
        let seed = Seed::new(&mnemonic, passphrase);
        let expected_keypair = keypair_from_seed(seed.as_bytes()).unwrap();
        let keypair =
            keypair_from_seed_phrase_and_passphrase(mnemonic.phrase(), passphrase).unwrap();
        assert_eq!(keypair.pubkey(), expected_keypair.pubkey());
    }
}
