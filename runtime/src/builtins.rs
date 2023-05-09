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

/// Transitions of built-in programs at epoch bondaries when features are activated.
#[derive(Debug, Default, Clone)]
pub struct BuiltinFeatureTransition {
    pub feature_id: Pubkey,
    pub program_id: Pubkey,
    pub builtin: Arc<LoadedProgram>,
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl solana_frozen_abi::abi_example::AbiExample for BuiltinFeatureTransition {
    fn example() -> Self {
        // BuiltinFeatureTransition isn't serializable by definition.
        Self::default()
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
    vec![BuiltinFeatureTransition {
        feature_id: feature_set::zk_token_sdk_enabled::id(),
        program_id: solana_zk_token_sdk::zk_token_proof_program::id(),
        builtin: create_builtin(
            "zk_token_proof_program".to_string(),
            solana_zk_token_proof_program::process_instruction,
        ),
    }]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_transitions: builtin_feature_transitions(),
    }
}
