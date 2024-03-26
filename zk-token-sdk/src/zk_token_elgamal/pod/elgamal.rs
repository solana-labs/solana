//! Plain Old Data types for the ElGamal encryption scheme.

#[cfg(not(target_os = "solana"))]
use {
    crate::encryption::elgamal::{self as decoded, ElGamalError},
    curve25519_dalek::ristretto::CompressedRistretto,
};
use {
    crate::{
        zk_token_elgamal::pod::{impl_from_str, pedersen::PEDERSEN_COMMITMENT_LEN, Pod, Zeroable},
        RISTRETTO_POINT_LEN,
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    std::fmt,
};

/// Byte length of an ElGamal public key
const ELGAMAL_PUBKEY_LEN: usize = RISTRETTO_POINT_LEN;

/// Maximum length of a base64 encoded ElGamal public key
const ELGAMAL_PUBKEY_MAX_BASE64_LEN: usize = 44;

/// Byte length of a decrypt handle
pub(crate) const DECRYPT_HANDLE_LEN: usize = RISTRETTO_POINT_LEN;

/// Byte length of an ElGamal ciphertext
const ELGAMAL_CIPHERTEXT_LEN: usize = PEDERSEN_COMMITMENT_LEN + DECRYPT_HANDLE_LEN;

/// Maximum length of a base64 encoded ElGamal ciphertext
const ELGAMAL_CIPHERTEXT_MAX_BASE64_LEN: usize = 88;

/// The `ElGamalCiphertext` type as a `Pod`.
#[derive(Clone, Copy, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct ElGamalCiphertext(pub [u8; ELGAMAL_CIPHERTEXT_LEN]);

impl fmt::Debug for ElGamalCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl fmt::Display for ElGamalCiphertext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl Default for ElGamalCiphertext {
    fn default() -> Self {
        Self::zeroed()
    }
}

impl_from_str!(
    TYPE = ElGamalCiphertext,
    BYTES_LEN = ELGAMAL_CIPHERTEXT_LEN,
    BASE64_LEN = ELGAMAL_CIPHERTEXT_MAX_BASE64_LEN
);

#[cfg(not(target_os = "solana"))]
impl From<decoded::ElGamalCiphertext> for ElGamalCiphertext {
    fn from(decoded_ciphertext: decoded::ElGamalCiphertext) -> Self {
        Self(decoded_ciphertext.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ElGamalCiphertext> for decoded::ElGamalCiphertext {
    type Error = ElGamalError;

    fn try_from(pod_ciphertext: ElGamalCiphertext) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_ciphertext.0).ok_or(ElGamalError::CiphertextDeserialization)
    }
}

/// The `ElGamalPubkey` type as a `Pod`.
#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct ElGamalPubkey(pub [u8; ELGAMAL_PUBKEY_LEN]);

impl fmt::Debug for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl fmt::Display for ElGamalPubkey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", BASE64_STANDARD.encode(self.0))
    }
}

impl_from_str!(
    TYPE = ElGamalPubkey,
    BYTES_LEN = ELGAMAL_PUBKEY_LEN,
    BASE64_LEN = ELGAMAL_PUBKEY_MAX_BASE64_LEN
);

#[cfg(not(target_os = "solana"))]
impl From<decoded::ElGamalPubkey> for ElGamalPubkey {
    fn from(decoded_pubkey: decoded::ElGamalPubkey) -> Self {
        Self(decoded_pubkey.to_bytes())
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<ElGamalPubkey> for decoded::ElGamalPubkey {
    type Error = ElGamalError;

    fn try_from(pod_pubkey: ElGamalPubkey) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_pubkey.0).ok_or(ElGamalError::PubkeyDeserialization)
    }
}

/// The `DecryptHandle` type as a `Pod`.
#[derive(Clone, Copy, Default, Pod, Zeroable, PartialEq, Eq)]
#[repr(transparent)]
pub struct DecryptHandle(pub [u8; DECRYPT_HANDLE_LEN]);

impl fmt::Debug for DecryptHandle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl From<decoded::DecryptHandle> for DecryptHandle {
    fn from(decoded_handle: decoded::DecryptHandle) -> Self {
        Self(decoded_handle.to_bytes())
    }
}

// For proof verification, interpret pod::DecryptHandle as CompressedRistretto
#[cfg(not(target_os = "solana"))]
impl From<DecryptHandle> for CompressedRistretto {
    fn from(pod_handle: DecryptHandle) -> Self {
        Self(pod_handle.0)
    }
}

#[cfg(not(target_os = "solana"))]
impl TryFrom<DecryptHandle> for decoded::DecryptHandle {
    type Error = ElGamalError;

    fn try_from(pod_handle: DecryptHandle) -> Result<Self, Self::Error> {
        Self::from_bytes(&pod_handle.0).ok_or(ElGamalError::CiphertextDeserialization)
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::encryption::elgamal::ElGamalKeypair, std::str::FromStr};

    #[test]
    fn elgamal_pubkey_fromstr() {
        let elgamal_keypair = ElGamalKeypair::new_rand();
        let expected_elgamal_pubkey: ElGamalPubkey = (*elgamal_keypair.pubkey()).into();

        let elgamal_pubkey_base64_str = format!("{}", expected_elgamal_pubkey);
        let computed_elgamal_pubkey = ElGamalPubkey::from_str(&elgamal_pubkey_base64_str).unwrap();

        assert_eq!(expected_elgamal_pubkey, computed_elgamal_pubkey);
    }

    #[test]
    fn elgamal_ciphertext_fromstr() {
        let elgamal_keypair = ElGamalKeypair::new_rand();
        let expected_elgamal_ciphertext: ElGamalCiphertext =
            elgamal_keypair.pubkey().encrypt(0_u64).into();

        let elgamal_ciphertext_base64_str = format!("{}", expected_elgamal_ciphertext);
        let computed_elgamal_ciphertext =
            ElGamalCiphertext::from_str(&elgamal_ciphertext_base64_str).unwrap();

        assert_eq!(expected_elgamal_ciphertext, computed_elgamal_ciphertext);
    }
}
