#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
use {
    crate::system_instruction_processor,
    solana_program_runtime::{
        invoke_context::{InvokeContext, ProcessInstructionWithContext},
        stable_log,
    },
    solana_sdk::{
        feature_set, instruction::InstructionError, pubkey::Pubkey, stake, system_program,
    },
    std::fmt,
};

fn process_instruction_with_program_logging(
    process_instruction: ProcessInstructionWithContext,
    first_instruction_account: usize,
    instruction_data: &[u8],
    invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    let logger = invoke_context.get_log_collector();
    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let program_id = instruction_context.get_program_key(transaction_context)?;
    stable_log::program_invoke(&logger, program_id, invoke_context.get_stack_height());

    let result = process_instruction(first_instruction_account, instruction_data, invoke_context);

    let transaction_context = &invoke_context.transaction_context;
    let instruction_context = transaction_context.get_current_instruction_context()?;
    let program_id = instruction_context.get_program_key(transaction_context)?;
    match &result {
        Ok(()) => stable_log::program_success(&logger, program_id),
        Err(err) => stable_log::program_failure(&logger, program_id, err),
    }
    result
}

macro_rules! with_program_logging {
    ($process_instruction:expr) => {
        |first_instruction_account: usize,
         instruction_data: &[u8],
         invoke_context: &mut InvokeContext| {
            process_instruction_with_program_logging(
                $process_instruction,
                first_instruction_account,
                instruction_data,
                invoke_context,
            )
        }
    };
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

    /// Dynamic feature transitions for builtin programs
    pub feature_transitions: Vec<BuiltinFeatureTransition>,
}

/// Actions taken by a bank when managing the list of active builtin programs.
#[derive(Debug, Clone)]
pub enum BuiltinAction {
    Add(Builtin),
    Remove(Pubkey),
}

/// State transition enum used for adding and removing builtin programs through
/// feature activations.
#[derive(Debug, Clone, AbiExample)]
pub enum BuiltinFeatureTransition {
    /// Add a builtin program if a feature is activated.
    Add {
        builtin: Builtin,
        feature_id: Pubkey,
    },
    /// Remove a builtin program if a feature is activated or
    /// retain a previously added builtin.
    RemoveOrRetain {
        previously_added_builtin: Builtin,
        addition_feature_id: Pubkey,
        removal_feature_id: Pubkey,
    },
}

impl BuiltinFeatureTransition {
    pub fn to_action(
        &self,
        should_apply_action_for_feature: &impl Fn(&Pubkey) -> bool,
    ) -> Option<BuiltinAction> {
        match self {
            Self::Add {
                builtin,
                feature_id,
            } => {
                if should_apply_action_for_feature(feature_id) {
                    Some(BuiltinAction::Add(builtin.clone()))
                } else {
                    None
                }
            }
            Self::RemoveOrRetain {
                previously_added_builtin,
                addition_feature_id,
                removal_feature_id,
            } => {
                if should_apply_action_for_feature(removal_feature_id) {
                    Some(BuiltinAction::Remove(previously_added_builtin.id))
                } else if should_apply_action_for_feature(addition_feature_id) {
                    // Retaining is no different from adding a new builtin.
                    Some(BuiltinAction::Add(previously_added_builtin.clone()))
                } else {
                    None
                }
            }
        }
    }
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
            with_program_logging!(solana_vote_program::vote_processor::process_instruction),
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
    ]
}

/// place holder for precompile programs, remove when the precompile program is deactivated via feature activation
fn dummy_process_instruction(
    _first_instruction_account: usize,
    _data: &[u8],
    _invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    Ok(())
}

/// Dynamic feature transitions for builtin programs
fn builtin_feature_transitions() -> Vec<BuiltinFeatureTransition> {
    vec![
        BuiltinFeatureTransition::Add {
            builtin: Builtin::new(
                "compute_budget_program",
                solana_sdk::compute_budget::id(),
                solana_compute_budget_program::process_instruction,
            ),
            feature_id: feature_set::add_compute_budget_program::id(),
        },
        BuiltinFeatureTransition::RemoveOrRetain {
            previously_added_builtin: Builtin::new(
                "secp256k1_program",
                solana_sdk::secp256k1_program::id(),
                dummy_process_instruction,
            ),
            addition_feature_id: feature_set::secp256k1_program_enabled::id(),
            removal_feature_id: feature_set::prevent_calling_precompiles_as_programs::id(),
        },
        BuiltinFeatureTransition::RemoveOrRetain {
            previously_added_builtin: Builtin::new(
                "ed25519_program",
                solana_sdk::ed25519_program::id(),
                dummy_process_instruction,
            ),
            addition_feature_id: feature_set::ed25519_program_enabled::id(),
            removal_feature_id: feature_set::prevent_calling_precompiles_as_programs::id(),
        },
        BuiltinFeatureTransition::Add {
            builtin: Builtin::new(
                "address_lookup_table_program",
                solana_address_lookup_table_program::id(),
                solana_address_lookup_table_program::processor::process_instruction,
            ),
            feature_id: feature_set::versioned_tx_message_enabled::id(),
        },
        BuiltinFeatureTransition::Add {
            builtin: Builtin::new(
                "zk_token_proof_program",
                solana_zk_token_sdk::zk_token_proof_program::id(),
                with_program_logging!(solana_zk_token_proof_program::process_instruction),
            ),
            feature_id: feature_set::zk_token_sdk_enabled::id(),
        },
    ]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_transitions: builtin_feature_transitions(),
    }
}
