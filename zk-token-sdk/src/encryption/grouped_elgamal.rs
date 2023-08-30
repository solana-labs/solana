//! The twisted ElGamal group encryption implementation.
//!
//! The message space consists of any number that is representable as a scalar (a.k.a. "exponent")
//! for Curve25519.
//!
//! A regular twisted ElGamal ciphertext consists of two components:
//! - A Pedersen commitment that encodes a message to be encrypted
//! - A "decryption handle" that binds the Pedersen opening to a specific public key
//! The ciphertext can be generalized to hold not a single decryption handle, but multiple handles
//! pertaining to multiple ElGamal public keys. These ciphertexts are referred to as a "grouped"
//! ElGamal ciphertext.
//!

use {
    crate::{
        encryption::{
            discrete_log::DiscreteLog,
            elgamal::{DecryptHandle, ElGamalCiphertext, ElGamalPubkey, ElGamalSecretKey},
            pedersen::{Pedersen, PedersenCommitment, PedersenOpening},
        },
        RISTRETTO_POINT_LEN,
    },
    curve25519_dalek::scalar::Scalar,
    thiserror::Error,
};

#[derive(Error, Clone, Debug, Eq, PartialEq)]
pub enum GroupedElGamalError {
    #[error("index out of bounds")]
    IndexOutOfBounds,
}

/// Algorithm handle for the grouped ElGamal encryption
pub struct GroupedElGamal<const N: usize>;
impl<const N: usize> GroupedElGamal<N> {
    /// Encrypts an amount under an array of ElGamal public keys.
    ///
    /// This function is randomized. It internally samples a scalar element using `OsRng`.
    pub fn encrypt<T: Into<Scalar>>(
        pubkeys: [&ElGamalPubkey; N],
        amount: T,
    ) -> GroupedElGamalCiphertext<N> {
        let (commitment, opening) = Pedersen::new(amount);
        let handles: [DecryptHandle; N] = pubkeys
            .iter()
            .map(|handle| handle.decrypt_handle(&opening))
            .collect::<Vec<DecryptHandle>>()
            .try_into()
            .unwrap();

        GroupedElGamalCiphertext {
            commitment,
            handles,
        }
    }

    /// Encrypts an amount under an array of ElGamal public keys using a specified Pedersen
    /// opening.
    pub fn encrypt_with<T: Into<Scalar>>(
        pubkeys: [&ElGamalPubkey; N],
        amount: T,
        opening: &PedersenOpening,
    ) -> GroupedElGamalCiphertext<N> {
        let commitment = Pedersen::with(amount, opening);
        let handles: [DecryptHandle; N] = pubkeys
            .iter()
            .map(|handle| handle.decrypt_handle(opening))
            .collect::<Vec<DecryptHandle>>()
            .try_into()
            .unwrap();

        GroupedElGamalCiphertext {
            commitment,
            handles,
        }
    }

    /// Converts a grouped ElGamal ciphertext into a regular ElGamal ciphertext using the decrypt
    /// handle at a specified index.
    fn to_elgamal_ciphertext(
        grouped_ciphertext: &GroupedElGamalCiphertext<N>,
        index: usize,
    ) -> Result<ElGamalCiphertext, GroupedElGamalError> {
        let handle = grouped_ciphertext
            .handles
            .get(index)
            .ok_or(GroupedElGamalError::IndexOutOfBounds)?;

        Ok(ElGamalCiphertext {
            commitment: grouped_ciphertext.commitment,
            handle: *handle,
        })
    }

    /// Decrypts a grouped ElGamal ciphertext using an ElGamal secret key pertaining to a
    /// decryption handle at a specified index.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover the originally encrypted
    /// amount, use `DiscreteLog::decode`.
    fn decrypt(
        grouped_ciphertext: &GroupedElGamalCiphertext<N>,
        secret: &ElGamalSecretKey,
        index: usize,
    ) -> Result<DiscreteLog, GroupedElGamalError> {
        Self::to_elgamal_ciphertext(grouped_ciphertext, index)
            .map(|ciphertext| ciphertext.decrypt(secret))
    }

    /// Decrypts a grouped ElGamal ciphertext to a number that is interpreted as a positive 32-bit
    /// number (but still of type `u64`).
    ///
    /// If the originally encrypted amount is not a positive 32-bit number, then the function
    /// Result contains `None`.
    fn decrypt_u32(
        grouped_ciphertext: &GroupedElGamalCiphertext<N>,
        secret: &ElGamalSecretKey,
        index: usize,
    ) -> Result<Option<u64>, GroupedElGamalError> {
        Self::to_elgamal_ciphertext(grouped_ciphertext, index)
            .map(|ciphertext| ciphertext.decrypt_u32(secret))
    }
}

/// A grouped ElGamal ciphertext.
///
/// The type is defined with a generic constant parameter that specifies the number of
/// decryption handles that the ciphertext holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupedElGamalCiphertext<const N: usize> {
    pub commitment: PedersenCommitment,
    pub handles: [DecryptHandle; N],
}

impl<const N: usize> GroupedElGamalCiphertext<N> {
    /// Decrypts the grouped ElGamal ciphertext using an ElGamal secret key pertaining to a
    /// specified index.
    ///
    /// The output of this function is of type `DiscreteLog`. To recover the originally encrypted
    /// amount, use `DiscreteLog::decode`.
    pub fn decrypt(
        &self,
        secret: &ElGamalSecretKey,
        index: usize,
    ) -> Result<DiscreteLog, GroupedElGamalError> {
        GroupedElGamal::decrypt(self, secret, index)
    }

    /// Decrypts the grouped ElGamal ciphertext to a number that is interpreted as a positive 32-bit
    /// number (but still of type `u64`).
    ///
    /// If the originally encrypted amount is not a positive 32-bit number, then the function
    /// returns `None`.
    pub fn decrypt_u32(
        &self,
        secret: &ElGamalSecretKey,
        index: usize,
    ) -> Result<Option<u64>, GroupedElGamalError> {
        GroupedElGamal::decrypt_u32(self, secret, index)
    }

    /// The expected length of a serialized grouped ElGamal ciphertext.
    ///
    /// A grouped ElGamal ciphertext consists of a Pedersen commitment and an array of decryption
    /// handles. The commitment and decryption handles are each a single Curve25519 group element
    /// that is serialized as 32 bytes. Therefore, the total byte length of a grouped ciphertext is
    /// `(N+1) * 32`.
    fn expected_byte_length() -> usize {
        N.checked_add(1)
            .and_then(|length| length.checked_mul(RISTRETTO_POINT_LEN))
            .unwrap()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Self::expected_byte_length());
        buf.extend_from_slice(&self.commitment.to_bytes());
        self.handles
            .iter()
            .for_each(|handle| buf.extend_from_slice(&handle.to_bytes()));
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::expected_byte_length() {
            return None;
        }

        let mut iter = bytes.chunks(RISTRETTO_POINT_LEN);
        let commitment = PedersenCommitment::from_bytes(iter.next()?)?;

        let mut handles = Vec::with_capacity(N);
        for handle_bytes in iter {
            handles.push(DecryptHandle::from_bytes(handle_bytes)?);
        }

        Some(Self {
            commitment,
            handles: handles.try_into().unwrap(),
        })
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair};

    #[test]
    fn test_grouped_elgamal_encrypt_decrypt_correctness() {
        let elgamal_keypair_0 = ElGamalKeypair::new_rand();
        let elgamal_keypair_1 = ElGamalKeypair::new_rand();
        let elgamal_keypair_2 = ElGamalKeypair::new_rand();

        let amount: u64 = 10;
        let grouped_ciphertext = GroupedElGamal::encrypt(
            [
                elgamal_keypair_0.pubkey(),
                elgamal_keypair_1.pubkey(),
                elgamal_keypair_2.pubkey(),
            ],
            amount,
        );

        assert_eq!(
            Some(amount),
            grouped_ciphertext
                .decrypt_u32(elgamal_keypair_0.secret(), 0)
                .unwrap()
        );

        assert_eq!(
            Some(amount),
            grouped_ciphertext
                .decrypt_u32(elgamal_keypair_1.secret(), 1)
                .unwrap()
        );

        assert_eq!(
            Some(amount),
            grouped_ciphertext
                .decrypt_u32(elgamal_keypair_2.secret(), 2)
                .unwrap()
        );

        assert_eq!(
            GroupedElGamalError::IndexOutOfBounds,
            grouped_ciphertext
                .decrypt_u32(elgamal_keypair_0.secret(), 3)
                .unwrap_err()
        );
    }

    #[test]
    fn test_grouped_ciphertext_bytes() {
        let elgamal_keypair_0 = ElGamalKeypair::new_rand();
        let elgamal_keypair_1 = ElGamalKeypair::new_rand();
        let elgamal_keypair_2 = ElGamalKeypair::new_rand();

        let amount: u64 = 10;
        let grouped_ciphertext = GroupedElGamal::encrypt(
            [
                elgamal_keypair_0.pubkey(),
                elgamal_keypair_1.pubkey(),
                elgamal_keypair_2.pubkey(),
            ],
            amount,
        );

        let produced_bytes = grouped_ciphertext.to_bytes();
        assert_eq!(produced_bytes.len(), 128);

        let decoded_grouped_ciphertext =
            GroupedElGamalCiphertext::<3>::from_bytes(&produced_bytes).unwrap();
        assert_eq!(
            Some(amount),
            decoded_grouped_ciphertext
                .decrypt_u32(elgamal_keypair_0.secret(), 0)
                .unwrap()
        );

        assert_eq!(
            Some(amount),
            decoded_grouped_ciphertext
                .decrypt_u32(elgamal_keypair_1.secret(), 1)
                .unwrap()
        );

        assert_eq!(
            Some(amount),
            decoded_grouped_ciphertext
                .decrypt_u32(elgamal_keypair_2.secret(), 2)
                .unwrap()
        );
    }
}
