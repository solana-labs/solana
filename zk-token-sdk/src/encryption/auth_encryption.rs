//! Authenticated encryption implementation.
//!
//! This module is a simple wrapper of the `Aes128GcmSiv` implementation.
#[cfg(not(target_os = "solana"))]
use {
    aes_gcm_siv::{
        aead::{Aead, NewAead},
        Aes128GcmSiv,
    },
    rand::{rngs::OsRng, CryptoRng, Rng, RngCore},
};
use {
    arrayref::{array_ref, array_refs},
    sha3::{Digest, Sha3_512},
    solana_sdk::{
        derivation_path::DerivationPath,
        signature::Signature,
        signer::{
            keypair::generate_seed_from_seed_phrase_and_passphrase, EncodableKey, Signer,
            SignerError,
        },
    },
    std::{
        convert::TryInto,
        error, fmt,
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        path::Path,
    },
    subtle::ConstantTimeEq,
    thiserror::Error,
    zeroize::Zeroize,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum AuthenticatedEncryptionError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,

    #[error("pubkey does not exist")]
    PubkeyDoesNotExist,
}

struct AuthenticatedEncryption;
impl AuthenticatedEncryption {
    #[cfg(not(target_os = "solana"))]
    #[allow(clippy::new_ret_no_self)]
    fn keygen<T: RngCore + CryptoRng>(rng: &mut T) -> AeKey {
        AeKey(rng.gen::<[u8; 16]>())
    }

    #[cfg(not(target_os = "solana"))]
    fn encrypt(key: &AeKey, balance: u64) -> AeCiphertext {
        let mut plaintext = balance.to_le_bytes();
        let nonce: Nonce = OsRng.gen::<[u8; 12]>();

        // The balance and the nonce have fixed length and therefore, encryption should not fail.
        let ciphertext = Aes128GcmSiv::new(&key.0.into())
            .encrypt(&nonce.into(), plaintext.as_ref())
            .expect("authenticated encryption");

        plaintext.zeroize();

        AeCiphertext {
            nonce,
            ciphertext: ciphertext.try_into().unwrap(),
        }
    }

    #[cfg(not(target_os = "solana"))]
    fn decrypt(key: &AeKey, ct: &AeCiphertext) -> Option<u64> {
        let plaintext =
            Aes128GcmSiv::new(&key.0.into()).decrypt(&ct.nonce.into(), ct.ciphertext.as_ref());

        if let Ok(plaintext) = plaintext {
            let amount_bytes: [u8; 8] = plaintext.try_into().unwrap();
            Some(u64::from_le_bytes(amount_bytes))
        } else {
            None
        }
    }
}

#[derive(Debug, Zeroize)]
pub struct AeKey([u8; 16]);
impl AeKey {
    pub fn new_from_signer(signer: &dyn Signer, tag: &[u8]) -> Result<Self, Box<dyn error::Error>> {
        let seed = Self::seed_from_signer(signer, tag)?;
        Self::from_seed(&seed)
    }

    pub fn seed_from_signer(signer: &dyn Signer, tag: &[u8]) -> Result<Vec<u8>, SignerError> {
        let message = [b"AeKey", tag].concat();
        let signature = signer.try_sign_message(&message)?;

        if bool::from(signature.as_ref().ct_eq(Signature::default().as_ref())) {
            return Err(SignerError::Custom("Rejecting default signatures".into()));
        }

        let mut hasher = Sha3_512::new();
        hasher.update(signature.as_ref());
        let result = hasher.finalize();

        Ok(result.to_vec())
    }

    pub fn new_rand() -> Self {
        AuthenticatedEncryption::keygen(&mut OsRng)
    }

    pub fn encrypt(&self, amount: u64) -> AeCiphertext {
        AuthenticatedEncryption::encrypt(self, amount)
    }

    pub fn decrypt(&self, ct: &AeCiphertext) -> Option<u64> {
        AuthenticatedEncryption::decrypt(self, ct)
    }

    /// Reads a JSON-encoded key from a `Reader` implementor
    pub fn read_json<R: Read>(reader: &mut R) -> Result<Self, Box<dyn error::Error>> {
        let bytes: [u8; 16] = serde_json::from_reader(reader)?;
        Ok(Self(bytes))
    }

    /// Reads key from a file
    pub fn read_json_file<F: AsRef<Path>>(path: F) -> Result<Self, Box<dyn error::Error>> {
        let mut file = File::open(path.as_ref())?;
        Self::read_json(&mut file)
    }

    /// Writes to a `Write` implementer with JSON-encoding
    pub fn write_json<W: Write>(&self, writer: &mut W) -> Result<String, Box<dyn error::Error>> {
        let bytes = self.0;
        let json = serde_json::to_string(&bytes.to_vec())?;
        writer.write_all(&json.clone().into_bytes())?;
        Ok(json)
    }

    /// Write key to a file with JSON-encoding
    pub fn write_json_file<F: AsRef<Path>>(
        &self,
        outfile: F,
    ) -> Result<String, Box<dyn error::Error>> {
        let outfile = outfile.as_ref();

        if let Some(outdir) = outfile.parent() {
            fs::create_dir_all(outdir)?;
        }

        let mut f = {
            #[cfg(not(unix))]
            {
                OpenOptions::new()
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                OpenOptions::new().mode(0o600)
            }
        }
        .write(true)
        .truncate(true)
        .create(true)
        .open(outfile)?;

        self.write_json(&mut f)
    }

    /// Derive a key from an entropy seed.
    ///
    /// The seed is hashed using SHA3-512 and the first 16 bytes of the digest is taken as key. The
    /// hash SHA3-512 is used to be consistent with `ElGamalKeypair::from_seed`.
    pub fn from_seed(seed: &[u8]) -> Result<Self, Box<dyn error::Error>> {
        const MINIMUM_SEED_LEN: usize = 16;

        if seed.len() < MINIMUM_SEED_LEN {
            return Err("Seed is too short".into());
        }

        let mut hasher = Sha3_512::new();
        hasher.update(seed);
        let result = hasher.finalize();

        Ok(Self(result[..16].try_into()?))
    }

    /// Derive a key from a seed phrase and passphrase.
    pub fn from_seed_phrase_and_passphrase(
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<Self, Box<dyn error::Error>> {
        Self::from_seed(&generate_seed_from_seed_phrase_and_passphrase(
            seed_phrase,
            passphrase,
        ))
    }
}

impl EncodableKey for AeKey {
    fn pubkey_string(&self) -> Result<String, Box<dyn error::Error>> {
        Err(AuthenticatedEncryptionError::PubkeyDoesNotExist.into())
    }

    fn read_key<R: Read>(reader: &mut R) -> Result<Self, Box<dyn error::Error>> {
        Self::read_json(reader)
    }

    fn read_key_file<F: AsRef<Path>>(path: F) -> Result<Self, Box<dyn error::Error>> {
        Self::read_json_file(path)
    }

    fn write_key<W: Write>(&self, writer: &mut W) -> Result<String, Box<dyn error::Error>> {
        self.write_json(writer)
    }

    fn write_key_file<F: AsRef<Path>>(&self, outfile: F) -> Result<String, Box<dyn error::Error>> {
        self.write_json_file(outfile)
    }

    fn key_from_seed(seed: &[u8]) -> Result<Self, Box<dyn error::Error>> {
        Self::from_seed(seed)
    }

    fn key_from_seed_and_derivation_path(
        _seed: &[u8],
        _derivation_path: Option<DerivationPath>,
    ) -> Result<Self, Box<dyn error::Error>> {
        Err(AuthenticatedEncryptionError::DerivationMethodNotSupported.into())
    }

    fn key_from_seed_phrase_and_passphrase(
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<Self, Box<dyn error::Error>> {
        Self::from_seed_phrase_and_passphrase(seed_phrase, passphrase)
    }
}

/// For the purpose of encrypting balances for the spl token accounts, the nonce and ciphertext
/// sizes should always be fixed.
pub type Nonce = [u8; 12];
pub type Ciphertext = [u8; 24];

/// Authenticated encryption nonce and ciphertext
#[derive(Debug, Default, Clone)]
pub struct AeCiphertext {
    pub nonce: Nonce,
    pub ciphertext: Ciphertext,
}
impl AeCiphertext {
    pub fn decrypt(&self, key: &AeKey) -> Option<u64> {
        AuthenticatedEncryption::decrypt(key, self)
    }

    pub fn to_bytes(&self) -> [u8; 36] {
        let mut buf = [0_u8; 36];
        buf[..12].copy_from_slice(&self.nonce);
        buf[12..].copy_from_slice(&self.ciphertext);
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<AeCiphertext> {
        if bytes.len() != 36 {
            return None;
        }

        let bytes = array_ref![bytes, 0, 36];
        let (nonce, ciphertext) = array_refs![bytes, 12, 24];

        Some(AeCiphertext {
            nonce: *nonce,
            ciphertext: *ciphertext,
        })
    }
}

impl fmt::Display for AeCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", base64::encode(self.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        bip39::{Language, Mnemonic, MnemonicType, Seed},
        solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::null_signer::NullSigner},
    };

    #[test]
    fn test_aes_encrypt_decrypt_correctness() {
        let key = AeKey::new_rand();
        let amount = 55;

        let ct = key.encrypt(amount);
        let decrypted_amount = ct.decrypt(&key).unwrap();

        assert_eq!(amount, decrypted_amount);
    }

    #[test]
    fn test_aes_new_from_signer() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        assert_ne!(
            AeKey::new_from_signer(&keypair1, &Pubkey::default().as_ref())
                .unwrap()
                .0,
            AeKey::new_from_signer(&keypair2, &Pubkey::default().as_ref())
                .unwrap()
                .0,
        );

        let null_signer = NullSigner::new(&Pubkey::default());
        assert!(AeKey::new_from_signer(&null_signer, &Pubkey::default().as_ref()).is_err());
    }

    fn tmp_file_path(name: &str) -> String {
        use std::env;
        let out_dir = env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        format!("{}/tmp/{}", out_dir, name)
    }

    #[test]
    fn test_write_key_file() {
        let outfile = tmp_file_path("test_write_ae_key_file.json");
        let serialized_key = AeKey::new_rand().write_json_file(&outfile).unwrap();
        let key_vec: Vec<u8> = serde_json::from_str(&serialized_key).unwrap();
        assert!(Path::new(&outfile).exists());
        assert_eq!(key_vec, AeKey::read_json_file(&outfile).unwrap().0.to_vec());

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
        fs::remove_file(&outfile).unwrap();
    }

    #[test]
    fn test_write_keypair_file_truncate() {
        let outfile = tmp_file_path("test_write_keypair_file_truncate.json");

        AeKey::new_rand().write_json_file(&outfile).unwrap();
        AeKey::read_json_file(&outfile).unwrap();

        // Ensure outfile is truncated
        {
            let mut f = File::create(&outfile).unwrap();
            f.write_all(String::from_utf8([b'a'; 2048].to_vec()).unwrap().as_bytes())
                .unwrap();
        }
        AeKey::new_rand().write_json_file(&outfile).unwrap();
        AeKey::read_json_file(&outfile).unwrap();
    }

    #[test]
    fn test_key_from_seed() {
        let good_seed = vec![0; 16];
        assert!(AeKey::from_seed(&good_seed).is_ok());

        let too_short_seed = vec![0; 8];
        assert!(AeKey::from_seed(&too_short_seed).is_err());
    }

    #[test]
    fn test_key_from_seed_phrase_and_passphrase() {
        let mnemonic = Mnemonic::new(MnemonicType::Words12, Language::English);
        let passphrase = "42";
        let seed = Seed::new(&mnemonic, passphrase);
        let expected_key = AeKey::from_seed(seed.as_bytes()).unwrap();
        let key = AeKey::from_seed_phrase_and_passphrase(mnemonic.phrase(), passphrase).unwrap();
        assert_eq!(key.0, expected_key.0);
    }
}
