//! Authenticated encryption implementation.
//!
//! This module is a simple wrapper of the `Aes128GcmSiv` implementation specialized for SPL
//! token-2022 where the plaintext is always `u64`.
#[cfg(not(target_os = "solana"))]
use {
    aes_gcm_siv::{
        aead::{Aead, NewAead},
        Aes128GcmSiv,
    },
    rand::{rngs::OsRng, Rng},
    thiserror::Error,
};
use {
    arrayref::{array_ref, array_refs},
    base64::{prelude::BASE64_STANDARD, Engine},
    sha3::{Digest, Sha3_512},
    solana_sdk::{
        derivation_path::DerivationPath,
        signature::Signature,
        signer::{
            keypair::generate_seed_from_seed_phrase_and_passphrase, EncodableKey, SeedDerivable,
            Signer, SignerError,
        },
    },
    std::{
        convert::TryInto,
        error, fmt,
        io::{Read, Write},
    },
    subtle::ConstantTimeEq,
    zeroize::Zeroize,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum AuthenticatedEncryptionError {
    #[error("key derivation method not supported")]
    DerivationMethodNotSupported,
    #[error("seed length too short for derivation")]
    SeedLengthTooShort,
}

struct AuthenticatedEncryption;
impl AuthenticatedEncryption {
    /// Generates an authenticated encryption key.
    ///
    /// This function is randomized. It internally samples a 128-bit key using `OsRng`.
    #[cfg(not(target_os = "solana"))]
    fn keygen() -> AeKey {
        AeKey(OsRng.gen::<[u8; 16]>())
    }

    /// On input of an authenticated encryption key and an amount, the function returns a
    /// corresponding authenticated encryption ciphertext.
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

    /// On input of an authenticated encryption key and a ciphertext, the function returns the
    /// originally encrypted amount.
    #[cfg(not(target_os = "solana"))]
    fn decrypt(key: &AeKey, ciphertext: &AeCiphertext) -> Option<u64> {
        let plaintext = Aes128GcmSiv::new(&key.0.into())
            .decrypt(&ciphertext.nonce.into(), ciphertext.ciphertext.as_ref());

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
    /// Deterministically derives an authenticated encryption key from a Solana signer and a public
    /// seed.
    ///
    /// This function exists for applications where a user may not wish to maintain a Solana signer
    /// and an authenticated encryption key separately. Instead, a user can derive the ElGamal
    /// keypair on-the-fly whenever encrytion/decryption is needed.
    pub fn new_from_signer(
        signer: &dyn Signer,
        public_seed: &[u8],
    ) -> Result<Self, Box<dyn error::Error>> {
        let seed = Self::seed_from_signer(signer, public_seed)?;
        Self::from_seed(&seed)
    }

    /// Derive a seed from a Solana signer used to generate an authenticated encryption key.
    ///
    /// The seed is derived as the hash of the signature of a public seed.
    pub fn seed_from_signer(
        signer: &dyn Signer,
        public_seed: &[u8],
    ) -> Result<Vec<u8>, SignerError> {
        let message = [b"AeKey", public_seed].concat();
        let signature = signer.try_sign_message(&message)?;

        // Some `Signer` implementations return the default signature, which is not suitable for
        // use as key material
        if bool::from(signature.as_ref().ct_eq(Signature::default().as_ref())) {
            return Err(SignerError::Custom("Rejecting default signature".into()));
        }

        let mut hasher = Sha3_512::new();
        hasher.update(signature.as_ref());
        let result = hasher.finalize();

        Ok(result.to_vec())
    }

    /// Generates a random authenticated encryption key.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    pub fn new_rand() -> Self {
        AuthenticatedEncryption::keygen()
    }

    /// Encrypts an amount under the authenticated encryption key.
    pub fn encrypt(&self, amount: u64) -> AeCiphertext {
        AuthenticatedEncryption::encrypt(self, amount)
    }

    pub fn decrypt(&self, ciphertext: &AeCiphertext) -> Option<u64> {
        AuthenticatedEncryption::decrypt(self, ciphertext)
    }
}

impl EncodableKey for AeKey {
    fn read<R: Read>(reader: &mut R) -> Result<Self, Box<dyn error::Error>> {
        let bytes: [u8; 16] = serde_json::from_reader(reader)?;
        Ok(Self(bytes))
    }

    fn write<W: Write>(&self, writer: &mut W) -> Result<String, Box<dyn error::Error>> {
        let bytes = self.0;
        let json = serde_json::to_string(&bytes.to_vec())?;
        writer.write_all(&json.clone().into_bytes())?;
        Ok(json)
    }
}

impl SeedDerivable for AeKey {
    fn from_seed(seed: &[u8]) -> Result<Self, Box<dyn error::Error>> {
        const MINIMUM_SEED_LEN: usize = 16;

        if seed.len() < MINIMUM_SEED_LEN {
            return Err(AuthenticatedEncryptionError::SeedLengthTooShort.into());
        }

        let mut hasher = Sha3_512::new();
        hasher.update(seed);
        let result = hasher.finalize();

        Ok(Self(result[..16].try_into()?))
    }

    fn from_seed_and_derivation_path(
        _seed: &[u8],
        _derivation_path: Option<DerivationPath>,
    ) -> Result<Self, Box<dyn error::Error>> {
        Err(AuthenticatedEncryptionError::DerivationMethodNotSupported.into())
    }

    fn from_seed_phrase_and_passphrase(
        seed_phrase: &str,
        passphrase: &str,
    ) -> Result<Self, Box<dyn error::Error>> {
        Self::from_seed(&generate_seed_from_seed_phrase_and_passphrase(
            seed_phrase,
            passphrase,
        ))
    }
}

/// For the purpose of encrypting balances for the spl token accounts, the nonce and ciphertext
/// sizes should always be fixed.
type Nonce = [u8; 12];
type Ciphertext = [u8; 24];

/// Authenticated encryption nonce and ciphertext
#[derive(Debug, Default, Clone)]
pub struct AeCiphertext {
    nonce: Nonce,
    ciphertext: Ciphertext,
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
        write!(f, "{}", BASE64_STANDARD.encode(self.to_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{pubkey::Pubkey, signature::Keypair, signer::null_signer::NullSigner},
    };

    #[test]
    fn test_aes_encrypt_decrypt_correctness() {
        let key = AeKey::new_rand();
        let amount = 55;

        let ciphertext = key.encrypt(amount);
        let decrypted_amount = ciphertext.decrypt(&key).unwrap();

        assert_eq!(amount, decrypted_amount);
    }

    #[test]
    fn test_aes_new() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        assert_ne!(
            AeKey::new_from_signer(&keypair1, Pubkey::default().as_ref())
                .unwrap()
                .0,
            AeKey::new_from_signer(&keypair2, Pubkey::default().as_ref())
                .unwrap()
                .0,
        );

        let null_signer = NullSigner::new(&Pubkey::default());
        assert!(AeKey::new_from_signer(&null_signer, Pubkey::default().as_ref()).is_err());
    }
}
