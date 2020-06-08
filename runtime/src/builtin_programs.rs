use crate::{legacy_system_instruction_processor0, system_instruction_processor};
use solana_sdk::{
    clock::Epoch, entrypoint_native::ProcessInstruction, genesis_config::OperatingMode,
    pubkey::Pubkey, system_program,
};

pub struct BuiltinProgram {
    pub name: String,
    pub id: Pubkey,
    pub process_instruction: ProcessInstruction,
}
impl BuiltinProgram {
    pub fn new(name: &str, id: Pubkey, process_instruction: ProcessInstruction) -> Self {
        Self {
            name: name.to_string(),
            id,
            process_instruction,
        }
    }
}

pub(crate) fn new_system_program_activation_epoch(operating_mode: OperatingMode) -> Epoch {
    match operating_mode {
        OperatingMode::Development => 0,
        OperatingMode::Preview => 60,
        OperatingMode::Stable => 40,
    }
}

/// All builtin programs that should be active at the given (operating_mode, epoch)
pub fn get_builtin_programs(operating_mode: OperatingMode, epoch: Epoch) -> Vec<BuiltinProgram> {
    vec![
        if epoch < new_system_program_activation_epoch(operating_mode) {
            BuiltinProgram::new(
                "system_program",
                system_program::id(),
                legacy_system_instruction_processor0::process_instruction,
            )
        } else {
            BuiltinProgram::new(
                "system_program",
                system_program::id(),
                system_instruction_processor::process_instruction,
            )
        },
        BuiltinProgram::new(
            "config_program",
            solana_config_program::id(),
            solana_config_program::config_processor::process_instruction,
        ),
        BuiltinProgram::new(
            "stake_program",
            solana_stake_program::id(),
            solana_stake_program::stake_instruction::process_instruction,
        ),
        BuiltinProgram::new(
            "vote_program",
            solana_vote_program::id(),
            solana_vote_program::vote_instruction::process_instruction,
        ),
    ]
}

/// Builtin programs that activate at the given (operating_mode, epoch)
pub fn get_epoch_activated_builtin_programs(
    operating_mode: OperatingMode,
    epoch: Epoch,
) -> Option<Vec<BuiltinProgram>> {
    if epoch == new_system_program_activation_epoch(operating_mode) {
        Some(vec![BuiltinProgram::new(
            "system_program",
            system_program::id(),
            system_instruction_processor::process_instruction,
        )])
    } else {
        None
    }
}
