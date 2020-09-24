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
        Builtin::new(
            "stake_program",
            solana_stake_program::id(),
            Entrypoint::Program(solana_stake_program::stake_instruction::process_instruction),
        ),
        Builtin::new(
            "config_program",
            solana_config_program::id(),
            Entrypoint::Program(solana_config_program::config_processor::process_instruction),
        ),
    ]
}

/// Builtin programs activated dynamically by feature
fn feature_builtins() -> Vec<(Builtin, Pubkey)> {
    vec![(
        Builtin::new(
            "secp256k1_program",
            solana_sdk::secp256k1_program::id(),
            Entrypoint::Program(solana_secp256k1_program::process_instruction),
        ),
        feature_set::secp256k1_program_enabled::id(),
    )]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_builtins: feature_builtins(),
    }
}
