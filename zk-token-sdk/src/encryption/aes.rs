use {
    ed25519_dalek::SecretKey as SigningKey,
    solana_sdk::pubkey::Pubkey,
    std::convert::TryInto,
    zeroize::Zeroize,
};
#[cfg(not(target_arch = "bpf"))]
use {
    aes_gcm::{aead::Aead, Aes128Gcm, NewAead},
    rand::{CryptoRng, rngs::OsRng, Rng, RngCore},
    sha3::{Digest, Sha3_256},
};

struct AES;
impl AES {
    #[cfg(not(target_arch = "bpf"))]
    #[allow(clippy::new_ret_no_self)]
    fn keygen<T: RngCore + CryptoRng>(rng: &mut T) -> AesKey {
        let random_bytes = OsRng.gen::<[u8; 16]>();
        AesKey(random_bytes)
    }

    #[cfg(not(target_arch = "bpf"))]
    fn encrypt(sk: &AesKey, amount: u64) -> AesCiphertext {
        let plaintext = amount.to_le_bytes();
        let nonce = OsRng.gen::<[u8; 12]>();

        // TODO: it seems like encryption cannot fail, but will need to double check
        let ciphertext = Aes128Gcm::new(&sk.0.into())
            .encrypt(&nonce.into(), plaintext.as_ref()).unwrap();

        AesCiphertext {
            nonce,
            ciphertext: ciphertext.try_into().unwrap(),
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    fn decrypt(sk: &AesKey, ct: &AesCiphertext) -> Option<u64> {
        let plaintext = Aes128Gcm::new(&sk.0.into())
            .decrypt(&ct.nonce.into(), ct.ciphertext.as_ref());

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
    pub fn new(signing_key: &SigningKey, address: &Pubkey) -> Self {
        let mut hashable = [0_u8; 64];
        hashable[..32].copy_from_slice(&signing_key.to_bytes());
        hashable[32..].copy_from_slice(&address.to_bytes());

        let mut hasher = Sha3_256::new();
        hasher.update(hashable);

        let result: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();
        AesKey(result)
    }

    pub fn random<T: RngCore + CryptoRng>(rng: &mut T) -> Self {
        AES::keygen(&mut rng)
    }

    pub fn encrypt(&self, amount: u64) -> AesCiphertext {
        AES::encrypt(self, amount)
    }
}

#[derive(Debug)]
pub struct AesCiphertext {
    pub nonce: [u8; 12],
    pub ciphertext: [u8; 24],
}
impl AesCiphertext {
    pub fn decrypt(&self, key: &AesKey) -> Option<u64> {
        AES::decrypt(key, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_encrypt_decrypt_correctness() {
        let key = AesKey::random(&mut OsRng);
        let amount = 55;

        let ct = key.encrypt(amount);
        let decrypted_amount = ct.decrypt(&key).unwrap();

        assert_eq!(amount, decrypted_amount);
    }
}
