//! Solana precompiled programs.

#![cfg(feature = "full")]

#[deprecated(since = "2.1.0", note = "Use `solana-precompile-error` crate instead.")]
pub use solana_precompile_error::PrecompileError;
use {
    lazy_static::lazy_static, solana_feature_set::FeatureSet,
    solana_program::instruction::CompiledInstruction, solana_pubkey::Pubkey,
    solana_secp256r1_program as secp256r1_program,
};

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
        Precompile::new(
            secp256r1_program::id(),
            Some(solana_feature_set::enable_secp256r1_precompile::id()),
            secp256r1_program::verify,
        )
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

/// Find an enabled precompiled program
pub fn get_precompile<F>(program_id: &Pubkey, is_enabled: F) -> Option<&Precompile>
where
    F: Fn(&Pubkey) -> bool,
{
    PRECOMPILES
        .iter()
        .find(|precompile| precompile.check_id(program_id, |feature_id| is_enabled(feature_id)))
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
