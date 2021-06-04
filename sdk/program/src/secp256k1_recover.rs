use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use thiserror::Error;
use core::convert::TryFrom;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Secp256k1RecoverError {
    #[error("The digest provided to a secp256k1_recover has an invalid size")]
    InvalidDigestLength,
    #[error("The signature provided to a secp256k1_recover has an invalid size")]
    InvalidSignatureLength,
    #[error("The recovery_id provided to a secp256k1_recover is invalid")]
    InvalidRecoveryId,
    #[error("The signature provided to a secp256k1_recover is invalid")]
    InvalidSignature,
}

#[repr(transparent)]
#[derive(
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    Clone,
    Copy,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    AbiExample,
)]
pub struct Secp256k1Pubkey(pub [u8; 64]);

impl Secp256k1Pubkey {
    pub fn new(pubkey_vec: &[u8]) -> Self {
        Self(
            <[u8; 64]>::try_from(<&[u8]>::clone(&pubkey_vec))
                .expect("Slice must be the same length as a Pubkey"),
        )
    }

    pub fn to_bytes(self) -> [u8; 64] {
        self.0
    }
}

pub fn secp256k1_recover(
    digest: &[u8],
    recovery_id: u8,
    signature: &[u8],
) -> Result<Secp256k1Pubkey, Secp256k1RecoverError> {
    #[cfg(target_arch = "bpf")]
    {
        extern "C" {
            fn sol_secp256k1_recover(
                hash: *const u8,
                recovery_id: u64,
                signature: *const u8,
                result: *mut u8,
            ) -> u64;
        };

        let mut pubkey_buffer = [0u8; 64];
        let result = unsafe {
            sol_secp256k1_recover(
                digest.as_ptr(),
                recovery_id as u64,
                signature.as_ptr(),
                pubkey_buffer.as_mut_ptr(),
            )
        };

        match result {
            0 => Ok(Secp256k1Pubkey::new(&pubkey_buffer)),
            1 => Err(Secp256k1RecoverError::InvalidDigestLength),
            2 => Err(Secp256k1RecoverError::InvalidRecoveryId),
            3 => Err(Secp256k1RecoverError::InvalidSignatureLength),
            4 => Err(Secp256k1RecoverError::InvalidSignature),
            _ => panic!("Unsupported Secp256k1RecoverError"),
        }
    }

    #[cfg(not(target_arch = "bpf"))]
    {
        let message = libsecp256k1::Message::parse_slice(digest)
            .map_err(|_| Secp256k1RecoverError::InvalidDigestLength)?;
        let recovery_id = libsecp256k1::RecoveryId::parse(recovery_id)
            .map_err(|_| Secp256k1RecoverError::InvalidRecoveryId)?;
        let signature = libsecp256k1::Signature::parse_standard_slice(signature)
            .map_err(|_| Secp256k1RecoverError::InvalidSignatureLength)?;

        let secp256k1_key = libsecp256k1::recover(&message, &signature, &recovery_id)
            .map_err(|_| Secp256k1RecoverError::InvalidSignature)?;
        Ok(Secp256k1Pubkey::new(&secp256k1_key.serialize()[1..65]))
    }
}
