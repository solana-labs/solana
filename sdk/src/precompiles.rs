//! Solana precompiled programs.

#![cfg(feature = "full")]

use {
    crate::{
        decode_error::DecodeError, feature_set::FeatureSet, instruction::CompiledInstruction,
        pubkey::Pubkey,
    },
    lazy_static::lazy_static,
    thiserror::Error,
};

/// Precompile errors
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum PrecompileError {
    #[error("public key is not valid")]
    InvalidPublicKey,
    #[error("id is not valid")]
    InvalidRecoveryId,
    #[error("signature is not valid")]
    InvalidSignature,
    #[error("offset not valid")]
    InvalidDataOffsets,
    #[error("instruction is incorrect size")]
    InvalidInstructionDataSize,
}
impl<T> DecodeError<T> for PrecompileError {
    fn type_of() -> &'static str {
        "PrecompileError"
    }
}

/// All precompiled programs must implement the `Verify` function
pub type Verify = fn(&[u8], &[&[u8]], &FeatureSet) -> std::result::Result<(), PrecompileError>;

/// Information on a precompiled program
pub struct Precompile {
    /// Program id
    pub program_id: Pubkey,
    /// Feature to enable on, `None` indicates always enabled
    pub feature: Option<Pubkey>,
    /// Verification function
    pub verify_fn: Verify,
}
impl Precompile {
    /// Creates a new `Precompile`
    pub fn new(program_id: Pubkey, feature: Option<Pubkey>, verify_fn: Verify) -> Self {
        Precompile {
            program_id,
            feature,
            verify_fn,
        }
    }
    /// Check if a program id is this precompiled program
    pub fn check_id<F>(&self, program_id: &Pubkey, is_enabled: F) -> bool
    where
        F: Fn(&Pubkey) -> bool,
    {
        #![allow(clippy::redundant_closure)]
        self.feature
            .map_or(true, |ref feature_id| is_enabled(feature_id))
            && self.program_id == *program_id
    }
    /// Verify this precompiled program
    pub fn verify(
        &self,
        data: &[u8],
        instruction_datas: &[&[u8]],
        feature_set: &FeatureSet,
    ) -> std::result::Result<(), PrecompileError> {
        (self.verify_fn)(data, instruction_datas, feature_set)
    }
}

lazy_static! {
    /// The list of all precompiled programs
    static ref PRECOMPILES: Vec<Precompile> = vec![
        Precompile::new(
            crate::secp256k1_program::id(),
            None, // always enabled
            crate::secp256k1_instruction::verify,
        ),
        Precompile::new(
            crate::ed25519_program::id(),
            None, // always enabled
            crate::ed25519_instruction::verify,
        ),
    ];
}

/// Check if a program is a precompiled program
pub fn is_precompile<F>(program_id: &Pubkey, is_enabled: F) -> bool
where
    F: Fn(&Pubkey) -> bool,
{
    PRECOMPILES
        .iter()
        .any(|precompile| precompile.check_id(program_id, |feature_id| is_enabled(feature_id)))
}

pub fn get_precompiles<'a>() -> &'a [Precompile] {
    &PRECOMPILES
}

/// Check that a program is precompiled and if so verify it
pub fn verify_if_precompile(
    program_id: &Pubkey,
    precompile_instruction: &CompiledInstruction,
    all_instructions: &[CompiledInstruction],
    feature_set: &FeatureSet,
) -> Result<(), PrecompileError> {
    for precompile in PRECOMPILES.iter() {
        if precompile.check_id(program_id, |feature_id| feature_set.is_active(feature_id)) {
            let instruction_datas: Vec<_> = all_instructions
                .iter()
                .map(|instruction| instruction.data.as_ref())
                .collect();
            return precompile.verify(
                &precompile_instruction.data,
                &instruction_datas,
                feature_set,
            );
        }
    }
    Ok(())
}
