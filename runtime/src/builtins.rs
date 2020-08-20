use crate::{
    bank::{Builtin, Entrypoint},
    system_instruction_processor,
};
use solana_sdk::{clock::Epoch, genesis_config::OperatingMode, system_program};

/// The entire set of available builtin programs that should be active at the given (operating_mode, epoch)
pub fn get_builtins(_operating_mode: OperatingMode, _epoch: Epoch) -> Vec<Builtin> {
    let mut builtins = vec![];

    builtins.extend(vec![
        Builtin::new(
            "system_program",
            system_program::id(),
            Entrypoint::Program(system_instruction_processor::process_instruction),
        ),
        Builtin::new(
            "config_program",
            solana_config_program::id(),
            Entrypoint::Program(solana_config_program::config_processor::process_instruction),
        ),
        Builtin::new(
            "stake_program",
            solana_stake_program::id(),
            Entrypoint::Program(solana_stake_program::stake_instruction::process_instruction),
        ),
        Builtin::new(
            "vote_program",
            solana_vote_program::id(),
            Entrypoint::Program(solana_vote_program::vote_instruction::process_instruction),
        ),
    ]);

    // if we ever add gated builtins, add here like this
    // if _epoch >= 10 { builtins.extend(....) }

    builtins
}
