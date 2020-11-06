use crate::{
    bank::{Builtin, Builtins, Entrypoint},
    feature_set, system_instruction_processor,
};
use solana_sdk::{pubkey::Pubkey, system_program};

/// Builtin programs that are always available
fn genesis_builtins() -> Vec<Builtin> {
    vec![
        Builtin::new(
            "system_program",
            system_program::id(),
            Entrypoint::Program(system_instruction_processor::process_instruction),
        ),
        Builtin::new(
            "vote_program",
            solana_vote_program::id(),
            Entrypoint::Program(solana_vote_program::vote_instruction::process_instruction),
        ),
        // Remove legacy_stake_processor and move stake_instruction::process_instruction back to
        // genesis_builtins around the v1.6 timeframe
        Builtin::new(
            "stake_program",
            solana_stake_program::id(),
<<<<<<< HEAD
            Entrypoint::Program(solana_stake_program::stake_instruction::process_instruction),
=======
            solana_stake_program::legacy_stake_processor::process_instruction,
>>>>>>> 1b1d9f6b0... Feature-gate stake program (#13394)
        ),
        Builtin::new(
            "config_program",
            solana_config_program::id(),
            Entrypoint::Program(solana_config_program::config_processor::process_instruction),
        ),
    ]
}

#[derive(AbiExample, Debug, Clone)]
pub enum ActivationType {
    NewProgram,
    NewVersion,
}

/// Builtin programs activated dynamically by feature
///
/// Note: If the feature_builtin is intended to replace another builtin program, it must have a new
/// name.
/// This is to enable the runtime to determine categorically whether the builtin update has
/// occurred, and preserve idempotency in Bank::add_native_program across genesis, snapshot, and
/// normal child Bank creation.
/// https://github.com/solana-labs/solana/blob/84b139cc94b5be7c9e0c18c2ad91743231b85a0d/runtime/src/bank.rs#L1723
fn feature_builtins() -> Vec<(Builtin, Pubkey, ActivationType)> {
<<<<<<< HEAD
    vec![(
        Builtin::new(
            "secp256k1_program",
            solana_sdk::secp256k1_program::id(),
            Entrypoint::Program(solana_secp256k1_program::process_instruction),
=======
    vec![
        (
            Builtin::new(
                "secp256k1_program",
                solana_sdk::secp256k1_program::id(),
                solana_secp256k1_program::process_instruction,
            ),
            feature_set::secp256k1_program_enabled::id(),
            ActivationType::NewProgram,
>>>>>>> 1b1d9f6b0... Feature-gate stake program (#13394)
        ),
        (
            Builtin::new(
                "stake_program_v2",
                solana_stake_program::id(),
                solana_stake_program::stake_instruction::process_instruction,
            ),
            feature_set::stake_program_v2::id(),
            ActivationType::NewVersion,
        ),
    ]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_builtins: feature_builtins(),
    }
}
