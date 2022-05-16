#[cfg(RUSTC_WITH_SPECIALIZATION)]
use solana_frozen_abi::abi_example::AbiExample;
#[cfg(debug_assertions)]
#[allow(deprecated)]
use solana_sdk::AutoTraitBreakSendSync;
use {
    crate::system_instruction_processor,
    solana_program_runtime::invoke_context::{InvokeContext, ProcessInstructionWithContext},
    solana_sdk::{
        feature_set, instruction::InstructionError, pubkey::Pubkey, stake, system_program,
    },
    std::fmt,
};

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
            process_instruction_with_context: |_, _| Ok(()),
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
enum InnerBuiltinFeatureTransition {
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

#[allow(deprecated)]
#[cfg(debug_assertions)]
impl AutoTraitBreakSendSync for InnerBuiltinFeatureTransition {}

#[derive(AbiExample, Clone, Debug)]
pub struct BuiltinFeatureTransition(InnerBuiltinFeatureTransition);

// https://github.com/solana-labs/solana/pull/23233 added `BuiltinFeatureTransition`
// to `Bank` which triggers https://github.com/rust-lang/rust/issues/92987 while
// attempting to resolve `Sync` on `BankRc` in `AccountsBackgroundService::new` ala,
//
// query stack during panic:
// #0 [evaluate_obligation] evaluating trait selection obligation `bank::BankRc: core::marker::Sync`
// #1 [typeck] type-checking `accounts_background_service::<impl at runtime/src/accounts_background_service.rs:358:1: 520:2>::new`
// #2 [typeck_item_bodies] type-checking all item bodies
// #3 [analysis] running analysis passes on this crate
// end of query stack
//
// Yoloing a `Sync` onto it avoids the auto trait evaluation and thus the ICE.
//
// We should remove this when upgrading to Rust 1.60.0, where the bug has been
// fixed by https://github.com/rust-lang/rust/pull/93064
unsafe impl Send for BuiltinFeatureTransition {}
unsafe impl Sync for BuiltinFeatureTransition {}

impl BuiltinFeatureTransition {
    pub fn to_action(
        &self,
        should_apply_action_for_feature: &impl Fn(&Pubkey) -> bool,
    ) -> Option<BuiltinAction> {
        match &self.0 {
            InnerBuiltinFeatureTransition::Add {
                builtin,
                ref feature_id,
            } => {
                if should_apply_action_for_feature(feature_id) {
                    Some(BuiltinAction::Add(builtin.clone()))
                } else {
                    None
                }
            }
            InnerBuiltinFeatureTransition::RemoveOrRetain {
                previously_added_builtin,
                ref addition_feature_id,
                ref removal_feature_id,
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
            system_instruction_processor::process_instruction,
        ),
        Builtin::new(
            "vote_program",
            solana_vote_program::id(),
            solana_vote_program::vote_processor::process_instruction,
        ),
        Builtin::new(
            "stake_program",
            stake::program::id(),
            solana_stake_program::stake_instruction::process_instruction,
        ),
        Builtin::new(
            "config_program",
            solana_config_program::id(),
            solana_config_program::config_processor::process_instruction,
        ),
    ]
}

/// place holder for precompile programs, remove when the precompile program is deactivated via feature activation
fn dummy_process_instruction(
    _first_instruction_account: usize,
    _invoke_context: &mut InvokeContext,
) -> Result<(), InstructionError> {
    Ok(())
}

/// Dynamic feature transitions for builtin programs
fn builtin_feature_transitions() -> Vec<BuiltinFeatureTransition> {
    vec![
        BuiltinFeatureTransition(InnerBuiltinFeatureTransition::Add {
            builtin: Builtin::new(
                "compute_budget_program",
                solana_sdk::compute_budget::id(),
                solana_compute_budget_program::process_instruction,
            ),
            feature_id: feature_set::add_compute_budget_program::id(),
        }),
        BuiltinFeatureTransition(InnerBuiltinFeatureTransition::RemoveOrRetain {
            previously_added_builtin: Builtin::new(
                "secp256k1_program",
                solana_sdk::secp256k1_program::id(),
                dummy_process_instruction,
            ),
            addition_feature_id: feature_set::secp256k1_program_enabled::id(),
            removal_feature_id: feature_set::prevent_calling_precompiles_as_programs::id(),
        }),
        BuiltinFeatureTransition(InnerBuiltinFeatureTransition::RemoveOrRetain {
            previously_added_builtin: Builtin::new(
                "ed25519_program",
                solana_sdk::ed25519_program::id(),
                dummy_process_instruction,
            ),
            addition_feature_id: feature_set::ed25519_program_enabled::id(),
            removal_feature_id: feature_set::prevent_calling_precompiles_as_programs::id(),
        }),
        BuiltinFeatureTransition(InnerBuiltinFeatureTransition::Add {
            builtin: Builtin::new(
                "address_lookup_table_program",
                solana_address_lookup_table_program::id(),
                solana_address_lookup_table_program::processor::process_instruction,
            ),
            feature_id: feature_set::versioned_tx_message_enabled::id(),
        }),
        BuiltinFeatureTransition(InnerBuiltinFeatureTransition::Add {
            builtin: Builtin::new(
                "zk_token_proof_program",
                solana_zk_token_sdk::zk_token_proof_program::id(),
                solana_zk_token_proof_program::process_instruction,
            ),
            feature_id: feature_set::zk_token_sdk_enabled::id(),
        }),
    ]
}

pub(crate) fn get() -> Builtins {
    Builtins {
        genesis_builtins: genesis_builtins(),
        feature_transitions: builtin_feature_transitions(),
    }
}

pub fn get_pubkeys() -> Vec<Pubkey> {
    let builtins = get();

    let mut pubkeys = Vec::new();
    pubkeys.extend(builtins.genesis_builtins.iter().map(|b| b.id));
    pubkeys.extend(
        builtins
            .feature_transitions
            .iter()
            .filter_map(|f| match &f.0 {
                InnerBuiltinFeatureTransition::Add {
                    builtin,
                    feature_id: _,
                } => Some(builtin.id),
                InnerBuiltinFeatureTransition::RemoveOrRetain {
                    previously_added_builtin: _,
                    addition_feature_id: _,
                    removal_feature_id: _,
                } => None,
            }),
    );
    pubkeys
}
