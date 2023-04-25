use {
    solana_program_runtime::builtin_program::BuiltinProgram,
    solana_sdk::{feature_set, pubkey::Pubkey},
};

#[derive(Clone, Debug)]
pub struct Builtins {
    /// Builtin programs that are always available
    pub genesis_builtins: Vec<BuiltinProgram>,

    /// Dynamic feature transitions for builtin programs
    pub feature_transitions: Vec<BuiltinFeatureTransition>,
}

/// Actions taken by a bank when managing the list of active builtin programs.
#[derive(Debug, Clone)]
pub enum BuiltinAction {
    Add(BuiltinProgram),
    Remove(Pubkey),
}

/// State transition enum used for adding and removing builtin programs through
/// feature activations.
#[derive(Debug, Clone, AbiExample)]
pub enum BuiltinFeatureTransition {
    /// Add a builtin program if a feature is activated.
    Add {
        builtin: BuiltinProgram,
        feature_id: Pubkey,
    },
    /// Remove a builtin program if a feature is activated or
    /// retain a previously added builtin.
    RemoveOrRetain {
        previously_added_builtin: BuiltinProgram,
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
                    Some(BuiltinAction::Remove(previously_added_builtin.program_id))
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

/// Built-in programs that are always available
fn genesis_builtins() -> Vec<BuiltinProgram> {
    vec![
        BuiltinProgram {
            name: "system_program".to_string(),
            program_id: solana_system_program::id(),
            process_instruction: solana_system_program::system_processor::process_instruction,
        },
        BuiltinProgram {
            name: "vote_program".to_string(),
            program_id: solana_vote_program::id(),
            process_instruction: solana_vote_program::vote_processor::process_instruction,
        },
        BuiltinProgram {
            name: "stake_program".to_string(),
            program_id: solana_stake_program::id(),
            process_instruction: solana_stake_program::stake_instruction::process_instruction,
        },
        BuiltinProgram {
            name: "config_program".to_string(),
            program_id: solana_config_program::id(),
            process_instruction: solana_config_program::config_processor::process_instruction,
        },
    ]
}

/// Dynamic feature transitions for builtin programs
fn builtin_feature_transitions() -> Vec<BuiltinFeatureTransition> {
    vec![
        BuiltinFeatureTransition::Add {
            builtin: BuiltinProgram {
                name: "compute_budget_program".to_string(),
                program_id: solana_sdk::compute_budget::id(),
                process_instruction: solana_compute_budget_program::process_instruction,
            },
            feature_id: feature_set::add_compute_budget_program::id(),
        },
        BuiltinFeatureTransition::Add {
            builtin: BuiltinProgram {
                name: "address_lookup_table_program".to_string(),
                program_id: solana_address_lookup_table_program::id(),
                process_instruction:
                    solana_address_lookup_table_program::processor::process_instruction,
            },
            feature_id: feature_set::versioned_tx_message_enabled::id(),
        },
        BuiltinFeatureTransition::Add {
            builtin: BuiltinProgram {
                name: "zk_token_proof_program".to_string(),
                program_id: solana_zk_token_sdk::zk_token_proof_program::id(),
                process_instruction: solana_zk_token_proof_program::process_instruction,
            },
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

/// Returns the addresses of all builtin programs.
pub fn get_pubkeys() -> Vec<Pubkey> {
    let builtins = get();

    let mut pubkeys = Vec::new();
    pubkeys.extend(builtins.genesis_builtins.iter().map(|b| b.program_id));
    pubkeys.extend(builtins.feature_transitions.iter().filter_map(|f| match f {
        BuiltinFeatureTransition::Add { builtin, .. } => Some(builtin.program_id),
        BuiltinFeatureTransition::RemoveOrRetain { .. } => None,
    }));
    pubkeys
}
