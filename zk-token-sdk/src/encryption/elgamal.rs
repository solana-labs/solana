#[cfg(not(target_arch = "bpf"))]
use rand::{rngs::OsRng, CryptoRng, RngCore};
use {
    crate::encryption::{
        discrete_log::DiscreteLog,
        pedersen::{Pedersen, PedersenBase, PedersenComm, PedersenDecHandle, PedersenOpen},
    },
    arrayref::{array_ref, array_refs},
    core::ops::{Add, Div, Mul, Sub},
    curve25519_dalek::{
        ristretto::{CompressedRistretto, RistrettoPoint},
        scalar::Scalar,
    },
    serde::{Deserialize, Serialize},
    std::collections::HashMap,
    std::convert::TryInto,
    subtle::{Choice, ConstantTimeEq},
    zeroize::Zeroize,
};

/// Handle for the (twisted) ElGamal encryption scheme
pub struct ElGamal;
impl ElGamal {
    /// Generates the public and secret keys for ElGamal encryption.
    #[cfg(not(target_arch = "bpf"))]
    pub fn keygen() -> (ElGamalPubkey, ElGamalSecretKey) {
        ElGamal::keygen_with(&mut OsRng) // using OsRng for now
    }

    /// On input a randomness generator, the function generates the public and
    /// secret keys for ElGamal encryption.
    #[cfg(not(target_arch = "bpf"))]
    #[allow(non_snake_case)]
    pub fn keygen_with<T: RngCore + CryptoRng>(rng: &mut T) -> (ElGamalPubkey, ElGamalSecretKey) {
        // sample a non-zero scalar
        let mut s: Scalar;
        loop {
            s = Scalar::random(rng);

            if s != Scalar::zero() {
                break;
            }
        }

        let H = PedersenBase::default().H;
        let P = s.invert() * H;

        (ElGamalPubkey(P), ElGamalSecretKey(s))
    }

    /// On input a public key and a message to be encrypted, the function
    /// returns an ElGamal ciphertext of the message under the public key.
    #[cfg(not(target_arch = "bpf"))]
    pub fn encrypt<T: Into<Scalar>>(pk: &ElGamalPubkey, amount: T) -> ElGamalCiphertext {
        let (message_comm, open) = Pedersen::commit(amount);
        let decrypt_handle = pk.gen_decrypt_handle(&open);

        ElGamalCiphertext {
            message_comm,
            decrypt_handle,
        }
    }

    /// On input a public key, message, and Pedersen opening, the function
    /// returns an ElGamal ciphertext of the message under the public key using
    /// the opening.
    pub fn encrypt_with<T: Into<Scalar>>(
        pk: &ElGamalPubkey,
        amount: T,
        open: &PedersenOpen,
    ) -> ElGamalCiphertext {
        let message_comm = Pedersen::commit_with(amount, open);
        let decrypt_handle = pk.gen_decrypt_handle(open);

        ElGamalCiphertext {
            message_comm,
            decrypt_handle,
        }
    }

    /// On input a secret key and a ciphertext, the function decrypts the ciphertext.
    ///
    /// The output of the function is of type `DiscreteLog`. The exact message
    /// can be recovered via the DiscreteLog's decode method.
    pub fn decrypt(sk: &ElGamalSecretKey, ct: &ElGamalCiphertext) -> DiscreteLog {
        let ElGamalSecretKey(s) = sk;
        let ElGamalCiphertext {
            message_comm,
            decrypt_handle,
        } = ct;

        DiscreteLog {
            generator: PedersenBase::default().G,
            target: message_comm.get_point() - s * decrypt_handle.get_point(),
        }
    }

    /// On input a secret key and a ciphertext, the function decrypts the
    /// ciphertext for a u32 value.
    pub fn decrypt_u32(sk: &ElGamalSecretKey, ct: &ElGamalCiphertext) -> Option<u32> {
        let discrete_log_instance = ElGamal::decrypt(sk, ct);
        discrete_log_instance.decode_u32()
    }

    /// On input a secret key, ciphertext, and hashmap, the function decrypts the
    /// ciphertext for a u32 value.
    pub fn decrypt_u32_online(
        sk: &ElGamalSecretKey,
        ct: &ElGamalCiphertext,
        hashmap: &HashMap<[u8; 32], u32>,
    ) -> Option<u32> {
        let discrete_log_instance = ElGamal::decrypt(sk, ct);
        discrete_log_instance.decode_u32_online(hashmap)
    }
}

/// Public key for the ElGamal encryption scheme.
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElGamalPubkey(RistrettoPoint);
impl ElGamalPubkey {
    pub fn get_point(&self) -> RistrettoPoint {
        self.0
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.compress().to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalPubkey> {
        Some(ElGamalPubkey(
            CompressedRistretto::from_slice(bytes).decompress()?,
        ))
    }

    /// Utility method for code ergonomics.
    #[cfg(not(target_arch = "bpf"))]
    pub fn encrypt<T: Into<Scalar>>(&self, msg: T) -> ElGamalCiphertext {
        ElGamal::encrypt(self, msg)
    }

    /// Utility method for code ergonomics.
    pub fn encrypt_with<T: Into<Scalar>>(&self, msg: T, open: &PedersenOpen) -> ElGamalCiphertext {
        ElGamal::encrypt_with(self, msg, open)
    }

    /// Generate a decryption token from an ElGamal public key and a Pedersen
    /// opening.
    pub fn gen_decrypt_handle(self, open: &PedersenOpen) -> PedersenDecHandle {
        PedersenDecHandle::generate_handle(open, &self)
    }
}

impl From<RistrettoPoint> for ElGamalPubkey {
    fn from(point: RistrettoPoint) -> ElGamalPubkey {
        ElGamalPubkey(point)
    }
}

/// Secret key for the ElGamal encryption scheme.
#[derive(Serialize, Deserialize, Debug, Zeroize)]
#[zeroize(drop)]
pub struct ElGamalSecretKey(Scalar);
impl ElGamalSecretKey {
    pub fn get_scalar(&self) -> Scalar {
        self.0
    }

    /// Utility method for code ergonomics.
    pub fn decrypt(&self, ct: &ElGamalCiphertext) -> DiscreteLog {
        ElGamal::decrypt(self, ct)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32(&self, ct: &ElGamalCiphertext) -> Option<u32> {
        ElGamal::decrypt_u32(self, ct)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32_online(
        &self,
        ct: &ElGamalCiphertext,
        hashmap: &HashMap<[u8; 32], u32>,
    ) -> Option<u32> {
        ElGamal::decrypt_u32_online(self, ct, hashmap)
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        self.0.to_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalSecretKey> {
        match bytes.try_into() {
            Ok(bytes) => Scalar::from_canonical_bytes(bytes).map(ElGamalSecretKey),
            _ => None,
        }
    }
}

impl From<Scalar> for ElGamalSecretKey {
    fn from(scalar: Scalar) -> ElGamalSecretKey {
        ElGamalSecretKey(scalar)
    }
}

impl Eq for ElGamalSecretKey {}
impl PartialEq for ElGamalSecretKey {
    fn eq(&self, other: &Self) -> bool {
        self.ct_eq(other).unwrap_u8() == 1u8
    }
}
impl ConstantTimeEq for ElGamalSecretKey {
    fn ct_eq(&self, other: &Self) -> Choice {
        self.0.ct_eq(&other.0)
    }
}

/// Ciphertext for the ElGamal encryption scheme.
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize, Default, Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElGamalCiphertext {
    pub message_comm: PedersenComm,
    pub decrypt_handle: PedersenDecHandle,
}
impl ElGamalCiphertext {
    pub fn add_to_msg<T: Into<Scalar>>(&self, message: T) -> Self {
        let diff_comm = Pedersen::commit_with(message, &PedersenOpen::default());
        ElGamalCiphertext {
            message_comm: self.message_comm + diff_comm,
            decrypt_handle: self.decrypt_handle,
        }
    }

    pub fn sub_to_msg<T: Into<Scalar>>(&self, message: T) -> Self {
        let diff_comm = Pedersen::commit_with(message, &PedersenOpen::default());
        ElGamalCiphertext {
            message_comm: self.message_comm - diff_comm,
            decrypt_handle: self.decrypt_handle,
        }
    }

    #[allow(clippy::wrong_self_convention)]
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];

        bytes[..32].copy_from_slice(self.message_comm.get_point().compress().as_bytes());
        bytes[32..].copy_from_slice(self.decrypt_handle.get_point().compress().as_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<ElGamalCiphertext> {
        let bytes = array_ref![bytes, 0, 64];
        let (message_comm, decrypt_handle) = array_refs![bytes, 32, 32];

        let message_comm = CompressedRistretto::from_slice(message_comm).decompress()?;
        let decrypt_handle = CompressedRistretto::from_slice(decrypt_handle).decompress()?;

        Some(ElGamalCiphertext {
            message_comm: PedersenComm(message_comm),
            decrypt_handle: PedersenDecHandle(decrypt_handle),
        })
    }

    /// Utility method for code ergonomics.
    pub fn decrypt(&self, sk: &ElGamalSecretKey) -> DiscreteLog {
        ElGamal::decrypt(sk, self)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32(&self, sk: &ElGamalSecretKey) -> Option<u32> {
        ElGamal::decrypt_u32(sk, self)
    }

    /// Utility method for code ergonomics.
    pub fn decrypt_u32_online(
        &self,
        sk: &ElGamalSecretKey,
        hashmap: &HashMap<[u8; 32], u32>,
    ) -> Option<u32> {
        ElGamal::decrypt_u32_online(sk, self, hashmap)
    }
}

impl<'a, 'b> Add<&'b ElGamalCiphertext> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn add(self, other: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            message_comm: self.message_comm + other.message_comm,
            decrypt_handle: self.decrypt_handle + other.decrypt_handle,
        }
    }
}

define_add_variants!(
    LHS = ElGamalCiphertext,
    RHS = ElGamalCiphertext,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Sub<&'b ElGamalCiphertext> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn sub(self, other: &'b ElGamalCiphertext) -> ElGamalCiphertext {
        ElGamalCiphertext {
            message_comm: self.message_comm - other.message_comm,
            decrypt_handle: self.decrypt_handle - other.decrypt_handle,
        }
    }
}

define_sub_variants!(
    LHS = ElGamalCiphertext,
    RHS = ElGamalCiphertext,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Mul<&'b Scalar> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn mul(self, other: &'b Scalar) -> ElGamalCiphertext {
        ElGamalCiphertext {
            message_comm: self.message_comm * other,
            decrypt_handle: self.decrypt_handle * other,
        }
    }
}

define_mul_variants!(
    LHS = ElGamalCiphertext,
    RHS = Scalar,
    Output = ElGamalCiphertext
);

impl<'a, 'b> Div<&'b Scalar> for &'a ElGamalCiphertext {
    type Output = ElGamalCiphertext;

    fn div(self, other: &'b Scalar) -> ElGamalCiphertext {
        ElGamalCiphertext {
            message_comm: self.message_comm * other.invert(),
            decrypt_handle: self.decrypt_handle * other.invert(),
        }
    }
}

define_div_variants!(
    LHS = ElGamalCiphertext,
    RHS = Scalar,
    Output = ElGamalCiphertext
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encryption::pedersen::Pedersen;

    #[test]
    fn test_encrypt_decrypt_correctness() {
        let (pk, sk) = ElGamal::keygen();
        let msg: u32 = 57;
        let ct = ElGamal::encrypt(&pk, msg);

        let expected_instance = DiscreteLog {
            generator: PedersenBase::default().G,
            target: Scalar::from(msg) * PedersenBase::default().G,
        };

        assert_eq!(expected_instance, ElGamal::decrypt(&sk, &ct));

        // Commenting out for faster testing
        // assert_eq!(msg, ElGamal::decrypt_u32(&sk, &ct).unwrap());
    }

    #[test]
    fn test_decrypt_handle() {
        let (pk_1, sk_1) = ElGamal::keygen();
        let (pk_2, sk_2) = ElGamal::keygen();

        let msg: u32 = 77;
        let (comm, open) = Pedersen::commit(msg);

        let decrypt_handle_1 = pk_1.gen_decrypt_handle(&open);
        let decrypt_handle_2 = pk_2.gen_decrypt_handle(&open);

        let ct_1 = decrypt_handle_1.to_elgamal_ctxt(comm);
        let ct_2 = decrypt_handle_2.to_elgamal_ctxt(comm);

        let expected_instance = DiscreteLog {
            generator: PedersenBase::default().G,
            target: Scalar::from(msg) * PedersenBase::default().G,
        };

        assert_eq!(expected_instance, sk_1.decrypt(&ct_1));
        assert_eq!(expected_instance, sk_2.decrypt(&ct_2));

        // Commenting out for faster testing
        // assert_eq!(msg, sk_1.decrypt_u32(&ct_1).unwrap());
        // assert_eq!(msg, sk_2.decrypt_u32(&ct_2).unwrap());
    }

    #[test]
    fn test_homomorphic_addition() {
        let (pk, _) = ElGamal::keygen();
        let msg_0: u64 = 57;
        let msg_1: u64 = 77;

        // Add two ElGamal ciphertexts
        let open_0 = PedersenOpen::random(&mut OsRng);
        let open_1 = PedersenOpen::random(&mut OsRng);

        let ct_0 = ElGamal::encrypt_with(&pk, msg_0, &open_0);
        let ct_1 = ElGamal::encrypt_with(&pk, msg_1, &open_1);

        let ct_sum = ElGamal::encrypt_with(&pk, msg_0 + msg_1, &(open_0 + open_1));

        assert_eq!(ct_sum, ct_0 + ct_1);

        // Add to ElGamal ciphertext
        let open = PedersenOpen::random(&mut OsRng);
        let ct = ElGamal::encrypt_with(&pk, msg_0, &open);
        let ct_sum = ElGamal::encrypt_with(&pk, msg_0 + msg_1, &open);

        assert_eq!(ct_sum, ct.add_to_msg(msg_1));
    }

    #[test]
    fn test_homomorphic_subtraction() {
        let (pk, _) = ElGamal::keygen();
        let msg_0: u64 = 77;
        let msg_1: u64 = 55;

        // Subtract two ElGamal ciphertexts
        let open_0 = PedersenOpen::random(&mut OsRng);
        let open_1 = PedersenOpen::random(&mut OsRng);

        let ct_0 = ElGamal::encrypt_with(&pk, msg_0, &open_0);
        let ct_1 = ElGamal::encrypt_with(&pk, msg_1, &open_1);

        let ct_sub = ElGamal::encrypt_with(&pk, msg_0 - msg_1, &(open_0 - open_1));

        assert_eq!(ct_sub, ct_0 - ct_1);

        // Subtract to ElGamal ciphertext
        let open = PedersenOpen::random(&mut OsRng);
        let ct = ElGamal::encrypt_with(&pk, msg_0, &open);
        let ct_sub = ElGamal::encrypt_with(&pk, msg_0 - msg_1, &open);

        assert_eq!(ct_sub, ct.sub_to_msg(msg_1));
    }

    #[test]
    fn test_homomorphic_multiplication() {
        let (pk, _) = ElGamal::keygen();
        let msg_0: u64 = 57;
        let msg_1: u64 = 77;

        let open = PedersenOpen::random(&mut OsRng);

        let ct = ElGamal::encrypt_with(&pk, msg_0, &open);
        let scalar = Scalar::from(msg_1);

        let ct_prod = ElGamal::encrypt_with(&pk, msg_0 * msg_1, &(open * scalar));

        assert_eq!(ct_prod, ct * scalar);
    }

    #[test]
    fn test_homomorphic_division() {
        let (pk, _) = ElGamal::keygen();
        let msg_0: u64 = 55;
        let msg_1: u64 = 5;

        let open = PedersenOpen::random(&mut OsRng);

        let ct = ElGamal::encrypt_with(&pk, msg_0, &open);
        let scalar = Scalar::from(msg_1);

        let ct_div = ElGamal::encrypt_with(&pk, msg_0 / msg_1, &(open / scalar));

        assert_eq!(ct_div, ct / scalar);
    }

    #[test]
    fn test_serde_ciphertext() {
        let (pk, _) = ElGamal::keygen();
        let msg: u64 = 77;
        let ct = pk.encrypt(msg);

        let encoded = bincode::serialize(&ct).unwrap();
        let decoded: ElGamalCiphertext = bincode::deserialize(&encoded).unwrap();

        assert_eq!(ct, decoded);
    }

    #[test]
    fn test_serde_pubkey() {
        let (pk, _) = ElGamal::keygen();

        let encoded = bincode::serialize(&pk).unwrap();
        let decoded: ElGamalPubkey = bincode::deserialize(&encoded).unwrap();

        assert_eq!(pk, decoded);
    }

    #[test]
    fn test_serde_secretkey() {
        let (_, sk) = ElGamal::keygen();

        let encoded = bincode::serialize(&sk).unwrap();
        let decoded: ElGamalSecretKey = bincode::deserialize(&encoded).unwrap();

        assert_eq!(sk, decoded);
    }
}
