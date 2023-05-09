use {
    solana_program_runtime::{builtin_program::create_builtin, loaded_programs::LoadedProgram},
    solana_sdk::{
        bpf_loader, bpf_loader_deprecated, bpf_loader_upgradeable, feature_set, pubkey::Pubkey,
    },
    std::sync::Arc,
};

#[derive(Clone, Debug)]
pub struct Builtins {
    /// Builtin programs that are always available
    pub genesis_builtins: Vec<(Pubkey, Arc<LoadedProgram>)>,

    /// Dynamic feature transitions for builtin programs
    pub feature_transitions: Vec<BuiltinFeatureTransition>,
}

/// Actions taken by a bank when managing the list of active builtin programs.
#[derive(Debug, Clone)]
pub enum BuiltinAction {
    Add(Pubkey, Arc<LoadedProgram>),
    Remove(Pubkey),
}

/// State transition enum used for adding and removing builtin programs through
/// feature activations.
#[derive(Debug, Clone, AbiExample)]
pub enum BuiltinFeatureTransition {
    /// Add a builtin program if a feature is activated.
    Add {
        program_id: Pubkey,
        builtin: Arc<LoadedProgram>,
        feature_id: Pubkey,
    },
    /// Remove a builtin program if a feature is activated or
    /// retain a previously added builtin.
    RemoveOrRetain {
        program_id: Pubkey,
        previously_added_builtin: Arc<LoadedProgram>,
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
                program_id,
                builtin,
                feature_id,
            } => {
                if should_apply_action_for_feature(feature_id) {
                    Some(BuiltinAction::Add(*program_id, builtin.clone()))
                } else {
                    None
                }
            }
            Self::RemoveOrRetain {
                program_id,
                previously_added_builtin,
                addition_feature_id,
                removal_feature_id,
            } => {
                if should_apply_action_for_feature(removal_feature_id) {
                    Some(BuiltinAction::Remove(*program_id))
                } else if should_apply_action_for_feature(addition_feature_id) {
                    // Retaining is no different from adding a new builtin.
                    Some(BuiltinAction::Add(
                        *program_id,
                        previously_added_builtin.clone(),
                    ))
                } else {
                    None
                }
            }
        }
    }
}

/// Built-in programs that are always available
fn genesis_builtins() -> Vec<(Pubkey, Arc<LoadedProgram>)> {
    vec![
        (
            solana_system_program::id(),
            create_builtin(
                "system_program".to_string(),
                solana_system_program::system_processor::process_instruction,
            ),
        ),
        (
            solana_vote_program::id(),
            create_builtin(
                "vote_program".to_string(),
                solana_vote_program::vote_processor::process_instruction,
            ),
        ),
        (
            solana_stake_program::id(),
            create_builtin(
                "stake_program".to_string(),
                solana_stake_program::stake_instruction::process_instruction,
            ),
        ),
        (
            solana_config_program::id(),
            create_builtin(
                "config_program".to_string(),
                solana_config_program::config_processor::process_instruction,
            ),
        ),
        (
            bpf_loader_deprecated::id(),
            create_builtin(
                "solana_bpf_loader_deprecated_program".to_string(),
                solana_bpf_loader_program::process_instruction,
            ),
        ),
        (
            bpf_loader::id(),
            create_builtin(
                "solana_bpf_loader_program".to_string(),
                solana_bpf_loader_program::process_instruction,
            ),
        ),
        (
            bpf_loader_upgradeable::id(),
            create_builtin(
                "solana_bpf_loader_upgradeable_program".to_string(),
                solana_bpf_loader_program::process_instruction,
            ),
        ),
        (
            solana_sdk::compute_budget::id(),
            create_builtin(
                "compute_budget_program".to_string(),
                solana_compute_budget_program::process_instruction,
            ),
        ),
        (
            solana_address_lookup_table_program::id(),
            create_builtin(
                "address_lookup_table_program".to_string(),
                solana_address_lookup_table_program::processor::process_instruction,
            ),
        ),
    ]
}

/// Dynamic feature transitions for builtin programs
fn builtin_feature_transitions() -> Vec<BuiltinFeatureTransition> {
    vec![BuiltinFeatureTransition::Add {
        program_id: solana_zk_token_sdk::zk_token_proof_program::id(),
        builtin: create_builtin(
            "zk_token_proof_program".to_string(),
            solana_zk_token_proof_program::process_instruction,
        ),
        feature_id: feature_set::zk_token_sdk_enabled::id(),
    }]
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
    pubkeys.extend(
        builtins
            .genesis_builtins
            .iter()
            .map(|(program_id, _builtin)| program_id),
    );
    pubkeys.extend(builtins.feature_transitions.iter().filter_map(|f| match f {
        BuiltinFeatureTransition::Add { program_id, .. } => Some(program_id),
        BuiltinFeatureTransition::RemoveOrRetain { .. } => None,
    }));
    pubkeys
}
