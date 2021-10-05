use crate::system_instruction_processor;
use solana_sdk::{
    feature_set,
    instruction::InstructionError,
    process_instruction::{stable_log, InvokeContext, ProcessInstructionWithContext},
    pubkey::Pubkey,
    stake, system_program,
};
use std::fmt;

#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;

fn process_instruction_with_program_logging(
    process_instruction: ProcessInstructionWithContext,
    program_id: &Pubkey,
    instruction_data: &[u8],
    invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    let logger = invoke_context.get_logger();
    stable_log::program_invoke(&logger, program_id, invoke_context.invoke_depth());

    let result = process_instruction(program_id, instruction_data, invoke_context);

    match &result {
        Ok(()) => stable_log::program_success(&logger, program_id),
        Err(err) => stable_log::program_failure(&logger, program_id, err),
    }
    result
}

macro_rules! with_program_logging {
    ($process_instruction:expr) => {
        |program_id: &Pubkey, instruction_data: &[u8], invoke_context: &mut dyn InvokeContext| {
            process_instruction_with_program_logging(
                $process_instruction,
                program_id,
                instruction_data,
                invoke_context,
            )
        }
    };
}

#[derive(AbiExample, Debug, Clone)]
pub enum ActivationType {
    NewProgram,
    NewVersion,
    RemoveProgram,
}

#[derive(Clone)]
pub struct Builtin {
    pub name: String,
    pub id: Pubkey,
    pub process_instruction_with_context: ProcessInstructionWithContext,
}

impl Builtin {
    pub fn new(
        name: &str,
        id: Pubkey,
        process_instruction_with_context: ProcessInstructionWithContext,
    ) -> Self {
        Self {
            name: name.to_string(),
            id,
            process_instruction_with_context,
        }
    }
}

impl fmt::Debug for Builtin {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Builtin [name={}, id={}]", self.name, self.id)
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl AbiExample for Builtin {
    fn example() -> Self {
        Self {
            name: String::default(),
            id: Pubkey::default(),
            process_instruction_with_context: |_, _, _| Ok(()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Builtins {
    /// Builtin programs that are always available
    pub genesis_builtins: Vec<Builtin>,

    /// Builtin programs activated or deactivated dynamically by feature
    pub feature_builtins: Vec<(Builtin, Pubkey, ActivationType)>,
}

/// Builtin programs that are always available
fn genesis_builtins() -> Vec<Builtin> {
    vec![
        Builtin::new(
            "system_program",
            system_program::id(),
            with_program_logging!(system_instruction_processor::process_instruction),
        ),
        Builtin::new(
            "vote_program",
            solana_vote_program::id(),
            with_program_logging!(solana_vote_program::vote_instruction::process_instruction),
        ),
        Builtin::new(
            "stake_program",
            stake::program::id(),
            with_program_logging!(solana_stake_program::stake_instruction::process_instruction),
        ),
        Builtin::new(
            "config_program",
            solana_config_program::id(),
            with_program_logging!(solana_config_program::config_processor::process_instruction),
        ),
        Builtin::new(
            "secp256k1_program",
            solana_sdk::secp256k1_program::id(),
            dummy_process_instruction,
        ),
    ]
}

/// place holder for secp256k1, remove when the precompile program is deactivated via feature activation
fn dummy_process_instruction(
    _program_id: &Pubkey,
    _data: &[u8],
    _invoke_context: &mut dyn InvokeContext,
) -> Result<(), InstructionError> {
    Ok(())
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
    vec![
        (
            Builtin::new(
                "compute_budget_program",
                solana_sdk::compute_budget::id(),
                solana_compute_budget_program::process_instruction,
            ),
            feature_set::tx_wide_compute_cap::id(),
            ActivationType::NewProgram,
        ),
        // TODO when feature `prevent_calling_precompiles_as_programs` is
        // cleaned up also remove "secp256k1_program" from the main builtins
        // list
        (
            Builtin::new(
                "secp256k1_program",
                solana_sdk::secp256k1_program::id(),
                dummy_process_instruction,
            ),
            feature_set::prevent_calling_precompiles_as_programs::id(),
            ActivationType::RemoveProgram,
        ),
    ]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_builtins: feature_builtins(),
    }
}
