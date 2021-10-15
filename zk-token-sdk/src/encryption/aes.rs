#[cfg(not(target_arch = "bpf"))]
use rand::{rngs::OsRng, Rng};

use {
    aes::{
        cipher::{BlockDecrypt, BlockEncrypt, NewBlockCipher},
        Aes128, Block,
    },
    arrayref::array_ref,
    zeroize::Zeroize,
};

pub struct AES;
impl AES {
    #[cfg(not(target_arch = "bpf"))]
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> AESKey {
        let random_bytes = OsRng.gen::<[u8; 16]>();
        AESKey(random_bytes)
    }

    #[cfg(not(target_arch = "bpf"))]
    pub fn encrypt(sk: &AESKey, amount: u64) -> AESCiphertext {
        let amount_bytes = amount.to_le_bytes();

        let mut aes_block: Block = [0_u8; 16].into();
        aes_block[..8].copy_from_slice(&amount_bytes);

        Aes128::new(&sk.0.into()).encrypt_block(&mut aes_block);
        AESCiphertext(aes_block.into())
    }

    #[cfg(not(target_arch = "bpf"))]
    pub fn decrypt(sk: &AESKey, ct: &AESCiphertext) -> u64 {
        let mut aes_block: Block = ct.0.into();
        Aes128::new(&sk.0.into()).decrypt_block(&mut aes_block);

        let amount_bytes = array_ref![aes_block[..8], 0, 8];
        u64::from_le_bytes(*amount_bytes)
    }
}

#[derive(Debug, Zeroize)]
pub struct AESKey([u8; 16]);
impl AESKey {
    pub fn encrypt(&self, amount: u64) -> AESCiphertext {
        AES::encrypt(self, amount)
    }
}

#[derive(Debug)]
pub struct AESCiphertext(pub [u8; 16]);
impl AESCiphertext {
    pub fn decrypt(&self, sk: &AESKey) -> u64 {
        AES::decrypt(sk, self)
    }
}

impl Default for AESCiphertext {
    fn default() -> Self {
        AESCiphertext([0_u8; 16])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_encrypt_decrypt_correctness() {
        let sk = AES::new();
        let amount = 55;

        let ct = sk.encrypt(amount);
        let decrypted_amount = ct.decrypt(&sk);

        assert_eq!(amount, decrypted_amount);
    }
}
