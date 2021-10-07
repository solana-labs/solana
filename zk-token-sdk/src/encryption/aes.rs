

pub struct AES;
impl AES {
    pub fn new() -> AESKey {
        AESKey
    }

    pub fn encrypt(sk: &AESKey, amount: u64) -> AESCiphertext {
        AESCiphertext
    }

    pub fn decrypt(sk: &AESKey, ct: &AESCiphertext) -> u64 {
        0_u64
    }
}

pub struct AESKey;
impl AESKey {
    pub fn encrypt(&self, amount: u64) -> AESCiphertext {
        AES::encrypt(self, amount)
    }
}

pub struct AESCiphertext;
impl AESCiphertext {
    pub fn decrypt(&self, sk: &AESKey) -> u64 {
        AES::decrypt(sk, self)
    }
}
