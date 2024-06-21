//! Plain Old Data types for the Grouped ElGamal encryption scheme.

#[cfg(not(target_os = "solana"))]
use crate::encryption::grouped_elgamal::GroupedElGamalCiphertext;
use {
    crate::{
        errors::ElGamalError,
        zk_token_elgamal::pod::{
            elgamal::{ElGamalCiphertext, DECRYPT_HANDLE_LEN, ELGAMAL_CIPHERTEXT_LEN},
            pedersen::{PedersenCommitment, PEDERSEN_COMMITMENT_LEN},
        },
    },
    bytemuck::Zeroable,
    std::fmt,
};

macro_rules! impl_extract {
    (TYPE = $type:ident) => {
        impl $type {
            /// Extract the commitment component from a grouped ciphertext
            pub fn extract_commitment(&self) -> PedersenCommitment {
                // `GROUPED_ELGAMAL_CIPHERTEXT_2_HANDLES` guaranteed to be at least `PEDERSEN_COMMITMENT_LEN`
                let commitment = self.0[..PEDERSEN_COMMITMENT_LEN].try_into().unwrap();
                PedersenCommitment(commitment)
            }

            /// Extract a regular ElGamal ciphertext using the decrypt handle at a specified index.
            pub fn try_extract_ciphertext(
                &self,
                index: usize,
            ) -> Result<ElGamalCiphertext, ElGamalError> {
                let mut ciphertext_bytes = [0u8; ELGAMAL_CIPHERTEXT_LEN];
                ciphertext_bytes[..PEDERSEN_COMMITMENT_LEN]
                    .copy_from_slice(&self.0[..PEDERSEN_COMMITMENT_LEN]);

                let handle_start = DECRYPT_HANDLE_LEN
                    .checked_mul(index)
                    .and_then(|n| n.checked_add(PEDERSEN_COMMITMENT_LEN))
                    .ok_or(ElGamalError::CiphertextDeserialization)?;
                let handle_end = handle_start
                    .checked_add(DECRYPT_HANDLE_LEN)
                    .ok_or(ElGamalError::CiphertextDeserialization)?;
                ciphertext_bytes[PEDERSEN_COMMITMENT_LEN..].copy_from_slice(
                    self.0
                        .get(handle_start..handle_end)
                        .ok_or(ElGamalError::CiphertextDeserialization)?,
                );

                Ok(ElGamalCiphertext(ciphertext_bytes))
            }
        }
    };
}

/// Byte length of a grouped ElGamal ciphertext with 2 handles
const GROUPED_ELGAMAL_CIPHERTEXT_2_HANDLES: usize =
    PEDERSEN_COMMITMENT_LEN + DECRYPT_HANDLE_LEN + DECRYPT_HANDLE_LEN;

/// Byte length of a grouped ElGamal ciphertext with 3 handles
const GROUPED_ELGAMAL_CIPHERTEXT_3_HANDLES: usize =
    PEDERSEN_COMMITMENT_LEN + DECRYPT_HANDLE_LEN + DECRYPT_HANDLE_LEN + DECRYPT_HANDLE_LEN;

/// The `GroupedElGamalCiphertext` type with two decryption handles as a `Pod`
#[derive(Clone, Copy, bytemuck_derive::Pod, bytemuck_derive::Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct GroupedElGamalCiphertext2Handles(pub [u8; GROUPED_ELGAMAL_CIPHERTEXT_2_HANDLES]);

impl fmt::Debug for GroupedElGamalCiphertext2Handles {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Default for GroupedElGamalCiphertext2Handles {
    fn default() -> Self {
        Self::zeroed()
    }
}
#[cfg(not(target_os = "solana"))]
impl From<GroupedElGamalCiphertext<2>> for GroupedElGamalCiphertext2Handles {
    fn from(decoded_ciphertext: GroupedElGamalCiphertext<2>) -> Self {
        Self(decoded_ciphertext.to_bytes().try_into().unwrap())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<GroupedElGamalCiphertext2Handles> for GroupedElGamalCiphertext<2> {
    type Error = ElGamalError;

    fn try_from(pod_ciphertext: GroupedElGamalCiphertext2Handles) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_ciphertext.0).ok_or(ElGamalError::CiphertextDeserialization)
    }
}

impl_extract!(TYPE = GroupedElGamalCiphertext2Handles);

/// The `GroupedElGamalCiphertext` type with three decryption handles as a `Pod`
#[derive(Clone, Copy, bytemuck_derive::Pod, bytemuck_derive::Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct GroupedElGamalCiphertext3Handles(pub [u8; GROUPED_ELGAMAL_CIPHERTEXT_3_HANDLES]);

impl fmt::Debug for GroupedElGamalCiphertext3Handles {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Default for GroupedElGamalCiphertext3Handles {
    fn default() -> Self {
        Self::zeroed()
    }
}

#[cfg(not(target_os = "solana"))]
impl From<GroupedElGamalCiphertext<3>> for GroupedElGamalCiphertext3Handles {
    fn from(decoded_ciphertext: GroupedElGamalCiphertext<3>) -> Self {
        Self(decoded_ciphertext.to_bytes().try_into().unwrap())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<GroupedElGamalCiphertext3Handles> for GroupedElGamalCiphertext<3> {
    type Error = ElGamalError;

    fn try_from(pod_ciphertext: GroupedElGamalCiphertext3Handles) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_ciphertext.0).ok_or(ElGamalError::CiphertextDeserialization)
    }
}

impl_extract!(TYPE = GroupedElGamalCiphertext3Handles);

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            encryption::{
                elgamal::ElGamalKeypair, grouped_elgamal::GroupedElGamal, pedersen::Pedersen,
            },
            zk_token_elgamal::pod::pedersen::PedersenCommitment,
        },
    };

    #[test]
    fn test_2_handles_ciphertext_extraction() {
        let elgamal_keypair_0 = ElGamalKeypair::new_rand();
        let elgamal_keypair_1 = ElGamalKeypair::new_rand();

        let amount: u64 = 10;
        let (commitment, opening) = Pedersen::new(amount);

        let grouped_ciphertext = GroupedElGamal::encrypt_with(
            [elgamal_keypair_0.pubkey(), elgamal_keypair_1.pubkey()],
            amount,
            &opening,
        );
        let pod_grouped_ciphertext: GroupedElGamalCiphertext2Handles = grouped_ciphertext.into();

        let expected_pod_commitment: PedersenCommitment = commitment.into();
        let actual_pod_commitment = pod_grouped_ciphertext.extract_commitment();
        assert_eq!(expected_pod_commitment, actual_pod_commitment);

        let expected_ciphertext_0 = elgamal_keypair_0.pubkey().encrypt_with(amount, &opening);
        let expected_pod_ciphertext_0: ElGamalCiphertext = expected_ciphertext_0.into();
        let actual_pod_ciphertext_0 = pod_grouped_ciphertext.try_extract_ciphertext(0).unwrap();
        assert_eq!(expected_pod_ciphertext_0, actual_pod_ciphertext_0);

        let expected_ciphertext_1 = elgamal_keypair_1.pubkey().encrypt_with(amount, &opening);
        let expected_pod_ciphertext_1: ElGamalCiphertext = expected_ciphertext_1.into();
        let actual_pod_ciphertext_1 = pod_grouped_ciphertext.try_extract_ciphertext(1).unwrap();
        assert_eq!(expected_pod_ciphertext_1, actual_pod_ciphertext_1);

        let err = pod_grouped_ciphertext
            .try_extract_ciphertext(2)
            .unwrap_err();
        assert_eq!(err, ElGamalError::CiphertextDeserialization);
    }

    #[test]
    fn test_3_handles_ciphertext_extraction() {
        let elgamal_keypair_0 = ElGamalKeypair::new_rand();
        let elgamal_keypair_1 = ElGamalKeypair::new_rand();
        let elgamal_keypair_2 = ElGamalKeypair::new_rand();

        let amount: u64 = 10;
        let (commitment, opening) = Pedersen::new(amount);

        let grouped_ciphertext = GroupedElGamal::encrypt_with(
            [
                elgamal_keypair_0.pubkey(),
                elgamal_keypair_1.pubkey(),
                elgamal_keypair_2.pubkey(),
            ],
            amount,
            &opening,
        );
        let pod_grouped_ciphertext: GroupedElGamalCiphertext3Handles = grouped_ciphertext.into();

        let expected_pod_commitment: PedersenCommitment = commitment.into();
        let actual_pod_commitment = pod_grouped_ciphertext.extract_commitment();
        assert_eq!(expected_pod_commitment, actual_pod_commitment);

        let expected_ciphertext_0 = elgamal_keypair_0.pubkey().encrypt_with(amount, &opening);
        let expected_pod_ciphertext_0: ElGamalCiphertext = expected_ciphertext_0.into();
        let actual_pod_ciphertext_0 = pod_grouped_ciphertext.try_extract_ciphertext(0).unwrap();
        assert_eq!(expected_pod_ciphertext_0, actual_pod_ciphertext_0);

        let expected_ciphertext_1 = elgamal_keypair_1.pubkey().encrypt_with(amount, &opening);
        let expected_pod_ciphertext_1: ElGamalCiphertext = expected_ciphertext_1.into();
        let actual_pod_ciphertext_1 = pod_grouped_ciphertext.try_extract_ciphertext(1).unwrap();
        assert_eq!(expected_pod_ciphertext_1, actual_pod_ciphertext_1);

        let expected_ciphertext_2 = elgamal_keypair_2.pubkey().encrypt_with(amount, &opening);
        let expected_pod_ciphertext_2: ElGamalCiphertext = expected_ciphertext_2.into();
        let actual_pod_ciphertext_2 = pod_grouped_ciphertext.try_extract_ciphertext(2).unwrap();
        assert_eq!(expected_pod_ciphertext_2, actual_pod_ciphertext_2);

        let err = pod_grouped_ciphertext
            .try_extract_ciphertext(3)
            .unwrap_err();
        assert_eq!(err, ElGamalError::CiphertextDeserialization);
    }
}
