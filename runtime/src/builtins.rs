use crate::{
    bank::{Builtin, Entrypoint},
    legacy_system_instruction_processor0, system_instruction_processor,
};
use solana_sdk::{clock::Epoch, genesis_config::OperatingMode, system_program};

pub(crate) fn new_system_program_activation_epoch(operating_mode: OperatingMode) -> Epoch {
    match operating_mode {
        OperatingMode::Development => 0,
        OperatingMode::Preview => 60,
        OperatingMode::Stable => 40,
    }
}

/// All builtin programs that should be active at the given (operating_mode, epoch)
pub fn get_builtins(operating_mode: OperatingMode, epoch: Epoch) -> Vec<Builtin> {
    vec![
        if epoch < new_system_program_activation_epoch(operating_mode) {
            Builtin::new(
                "system_program",
                system_program::id(),
                Entrypoint::Program(legacy_system_instruction_processor0::process_instruction),
            )
        } else {
            Builtin::new(
                "system_program",
                system_program::id(),
                Entrypoint::Program(system_instruction_processor::process_instruction),
            )
        },
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
    ]
}

/// Builtin programs that activate at the given (operating_mode, epoch)
pub fn get_epoch_activated_builtins(
    operating_mode: OperatingMode,
    epoch: Epoch,
) -> Option<Vec<Builtin>> {
    if epoch == new_system_program_activation_epoch(operating_mode) {
        Some(vec![Builtin::new(
            "system_program",
            system_program::id(),
            Entrypoint::Program(system_instruction_processor::process_instruction),
        )])
    } else {
        None
    }
}
