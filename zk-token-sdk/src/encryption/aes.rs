#[cfg(not(target_arch = "bpf"))]
use {
    aes_gcm::{aead::Aead, Aes128Gcm, NewAead},
    rand::{rngs::OsRng, CryptoRng, Rng, RngCore},
};
use {
    arrayref::{array_ref, array_refs},
    solana_sdk::{
        instruction::Instruction,
        message::Message,
        pubkey::Pubkey,
        signature::Signature,
        signer::{Signer, SignerError},
    },
    std::convert::TryInto,
    zeroize::Zeroize,
};

struct Aes;
impl Aes {
    #[cfg(not(target_arch = "bpf"))]
    #[allow(clippy::new_ret_no_self)]
    fn keygen<T: RngCore + CryptoRng>(rng: &mut T) -> AesKey {
        let random_bytes = rng.gen::<[u8; 16]>();
        AesKey(random_bytes)
    }

    #[cfg(not(target_arch = "bpf"))]
    fn encrypt(sk: &AesKey, amount: u64) -> AesCiphertext {
        let plaintext = amount.to_le_bytes();
        let nonce = OsRng.gen::<[u8; 12]>();

        // TODO: it seems like encryption cannot fail, but will need to double check
        let ciphertext = Aes128Gcm::new(&sk.0.into())
            .encrypt(&nonce.into(), plaintext.as_ref())
            .unwrap();

        AesCiphertext {
            nonce,
            ciphertext: ciphertext.try_into().unwrap(),
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    fn decrypt(sk: &AesKey, ct: &AesCiphertext) -> Option<u64> {
        let plaintext =
            Aes128Gcm::new(&sk.0.into()).decrypt(&ct.nonce.into(), ct.ciphertext.as_ref());

        if let Ok(plaintext) = plaintext {
            let amount_bytes: [u8; 8] = plaintext.try_into().unwrap();
            Some(u64::from_le_bytes(amount_bytes))
        } else {
            None
        }
    }
}

#[derive(Debug, Zeroize)]
pub struct AesKey([u8; 16]);
impl AesKey {
    pub fn new(signer: &dyn Signer, address: &Pubkey) -> Result<Self, SignerError> {
        let message = Message::new(
            &[Instruction::new_with_bytes(*address, b"AesKey", vec![])],
            Some(&signer.try_pubkey()?),
        );
        let signature = signer.try_sign_message(&message.serialize())?;

        // Some `Signer` implementations return the default signature, which is not suitable for
        // use as key material
        if signature == Signature::default() {
            Err(SignerError::Custom("Rejecting default signature".into()))
        } else {
            Ok(AesKey(signature.as_ref()[..16].try_into().unwrap()))
        }
    }

    pub fn random<T: RngCore + CryptoRng>(rng: &mut T) -> Self {
        Aes::keygen(rng)
    }

    pub fn encrypt(&self, amount: u64) -> AesCiphertext {
        Aes::encrypt(self, amount)
    }

    pub fn decrypt(&self, ct: &AesCiphertext) -> Option<u64> {
        Aes::decrypt(self, ct)
    }
}

#[derive(Debug)]
pub struct AesCiphertext {
    pub nonce: [u8; 12],
    pub ciphertext: [u8; 24],
}
impl AesCiphertext {
    pub fn decrypt(&self, key: &AesKey) -> Option<u64> {
        Aes::decrypt(key, self)
    }

    pub fn to_bytes(&self) -> [u8; 36] {
        let mut buf = [0_u8; 36];
        buf[..12].copy_from_slice(&self.nonce);
        buf[12..].copy_from_slice(&self.ciphertext);
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<AesCiphertext> {
        if bytes.len() != 36 {
            return None;
        }

        let bytes = array_ref![bytes, 0, 36];
        let (nonce, ciphertext) = array_refs![bytes, 12, 24];
        Some(AesCiphertext {
            nonce: *nonce,
            ciphertext: *ciphertext,
        })
    }
}

impl Default for AesCiphertext {
    fn default() -> Self {
        AesCiphertext {
            nonce: [0_u8; 12],
            ciphertext: [0_u8; 24],
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{signature::Keypair, signer::null_signer::NullSigner},
    };

    #[test]
    fn test_aes_encrypt_decrypt_correctness() {
        let key = AesKey::random(&mut OsRng);
        let amount = 55;

        let ct = key.encrypt(amount);
        let decrypted_amount = ct.decrypt(&key).unwrap();

        assert_eq!(amount, decrypted_amount);
    }

    #[test]
    fn test_aes_new() {
        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();

        assert_ne!(
            AesKey::new(&keypair1, &Pubkey::default()).unwrap().0,
            AesKey::new(&keypair2, &Pubkey::default()).unwrap().0,
        );

        let null_signer = NullSigner::new(&Pubkey::default());
        assert!(AesKey::new(&null_signer, &Pubkey::default()).is_err());
    }
}
