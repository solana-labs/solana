//! Functionality for public and private keys.
#![cfg(feature = "full")]

// legacy module paths
pub use crate::signer::{keypair::*, null_signer::*, presigner::*, *};
use {
    crate::pubkey::Pubkey,
    generic_array::{typenum::U64, GenericArray},
    std::{
        borrow::{Borrow, Cow},
        convert::TryInto,
        fmt,
        str::FromStr,
    },
    thiserror::Error,
};

/// Number of bytes in a signature
pub const SIGNATURE_BYTES: usize = 64;
/// Maximum string length of a base58 encoded signature
const MAX_BASE58_SIGNATURE_LEN: usize = 88;

#[repr(transparent)]
#[derive(
    Serialize, Deserialize, Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd, Hash, AbiExample,
)]
pub struct Signature(GenericArray<u8, U64>);

impl crate::sanitize::Sanitize for Signature {}

impl Signature {
    #[deprecated(
        since = "1.16.4",
        note = "Please use 'Signature::from' or 'Signature::try_from' instead"
    )]
    pub fn new(signature_slice: &[u8]) -> Self {
        Self(GenericArray::clone_from_slice(signature_slice))
    }

    pub fn new_unique() -> Self {
        Self::from(std::array::from_fn(|_| rand::random()))
    }

    pub(self) fn verify_verbose(
        &self,
        pubkey_bytes: &[u8],
        message_bytes: &[u8],
    ) -> Result<(), ed25519_dalek::SignatureError> {
        let publickey = ed25519_dalek::PublicKey::from_bytes(pubkey_bytes)?;
        let signature = self.0.as_slice().try_into()?;
        publickey.verify_strict(message_bytes, &signature)
    }

    pub fn verify(&self, pubkey_bytes: &[u8], message_bytes: &[u8]) -> bool {
        self.verify_verbose(pubkey_bytes, message_bytes).is_ok()
    }
}

pub trait Signable {
    fn sign(&mut self, keypair: &Keypair) {
        let signature = keypair.sign_message(self.signable_data().borrow());
        self.set_signature(signature);
    }
    fn verify(&self) -> bool {
        self.get_signature()
            .verify(self.pubkey().as_ref(), self.signable_data().borrow())
    }

    fn pubkey(&self) -> Pubkey;
    fn signable_data(&self) -> Cow<[u8]>;
    fn get_signature(&self) -> Signature;
    fn set_signature(&mut self, signature: Signature);
}

impl AsRef<[u8]> for Signature {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", bs58::encode(self.0).into_string())
    }
}

impl From<Signature> for [u8; 64] {
    fn from(signature: Signature) -> Self {
        signature.0.into()
    }
}

impl From<[u8; SIGNATURE_BYTES]> for Signature {
    #[inline]
    fn from(signature: [u8; SIGNATURE_BYTES]) -> Self {
        Self(GenericArray::from(signature))
    }
}

impl<'a> TryFrom<&'a [u8]> for Signature {
    type Error = <[u8; SIGNATURE_BYTES] as TryFrom<&'a [u8]>>::Error;

    #[inline]
    fn try_from(signature: &'a [u8]) -> Result<Self, Self::Error> {
        <[u8; SIGNATURE_BYTES]>::try_from(signature).map(Self::from)
    }
}

impl TryFrom<Vec<u8>> for Signature {
    type Error = <[u8; SIGNATURE_BYTES] as TryFrom<Vec<u8>>>::Error;

    #[inline]
    fn try_from(signature: Vec<u8>) -> Result<Self, Self::Error> {
        <[u8; SIGNATURE_BYTES]>::try_from(signature).map(Self::from)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseSignatureError {
    #[error("string decoded to wrong size for signature")]
    WrongSize,
    #[error("failed to decode string to signature")]
    Invalid,
}

impl FromStr for Signature {
    type Err = ParseSignatureError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() > MAX_BASE58_SIGNATURE_LEN {
            return Err(ParseSignatureError::WrongSize);
        }
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| ParseSignatureError::Invalid)?;
        Signature::try_from(bytes).map_err(|_| ParseSignatureError::WrongSize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_signature_fromstr() {
        let signature = Keypair::new().sign_message(&[0u8]);

        let mut signature_base58_str = bs58::encode(signature).into_string();

        assert_eq!(signature_base58_str.parse::<Signature>(), Ok(signature));

        signature_base58_str.push_str(&bs58::encode(signature.0).into_string());
        assert_eq!(
            signature_base58_str.parse::<Signature>(),
            Err(ParseSignatureError::WrongSize)
        );

        signature_base58_str.truncate(signature_base58_str.len() / 2);
        assert_eq!(signature_base58_str.parse::<Signature>(), Ok(signature));

        signature_base58_str.truncate(signature_base58_str.len() / 2);
        assert_eq!(
            signature_base58_str.parse::<Signature>(),
            Err(ParseSignatureError::WrongSize)
        );

        let mut signature_base58_str = bs58::encode(signature.0).into_string();
        assert_eq!(signature_base58_str.parse::<Signature>(), Ok(signature));

        // throw some non-base58 stuff in there
        signature_base58_str.replace_range(..1, "I");
        assert_eq!(
            signature_base58_str.parse::<Signature>(),
            Err(ParseSignatureError::Invalid)
        );

        // too long input string
        // longest valid encoding
        let mut too_long = bs58::encode(&[255u8; SIGNATURE_BYTES]).into_string();
        // and one to grow on
        too_long.push('1');
        assert_eq!(
            too_long.parse::<Signature>(),
            Err(ParseSignatureError::WrongSize)
        );
    }

    #[test]
    fn test_off_curve_pubkey_verify_fails() {
        // Golden point off the ed25519 curve
        let off_curve_bytes = bs58::decode("9z5nJyQar1FUxVJxpBXzon6kHehbomeYiDaLi9WAMhCq")
            .into_vec()
            .unwrap();

        // Confirm golden's off-curvedness
        let mut off_curve_bits = [0u8; 32];
        off_curve_bits.copy_from_slice(&off_curve_bytes);
        let off_curve_point = curve25519_dalek::edwards::CompressedEdwardsY(off_curve_bits);
        assert_eq!(off_curve_point.decompress(), None);

        let pubkey = Pubkey::try_from(off_curve_bytes).unwrap();
        let signature = Signature::default();
        // Unfortunately, ed25519-dalek doesn't surface the internal error types that we'd ideally
        // `source()` out of the `SignatureError` returned by `verify_strict()`.  So the best we
        // can do is `is_err()` here.
        assert!(signature.verify_verbose(pubkey.as_ref(), &[0u8]).is_err());
    }
}
