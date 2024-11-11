//! RuntimeTransaction is `runtime` facing representation of transaction, while
//! solana_sdk::SanitizedTransaction is client facing representation.
//!
//! It has two states:
//! 1. Statically Loaded: after receiving `packet` from sigverify and deserializing
//!    it into `solana_sdk::VersionedTransaction`, then sanitizing into
//!    `solana_sdk::SanitizedVersionedTransaction`, which can be wrapped into
//!    `RuntimeTransaction` with static transaction metadata extracted.
//! 2. Dynamically Loaded: after successfully loaded account addresses from onchain
//!    ALT, RuntimeTransaction<SanitizedMessage> transits into Dynamically Loaded state,
//!    with its dynamic metadata loaded.
use {
    crate::{
        compute_budget_instruction_details::*,
        transaction_meta::{DynamicMeta, StaticMeta, TransactionMeta},
    },
    core::ops::Deref,
    solana_compute_budget::compute_budget_limits::ComputeBudgetLimits,
    solana_sdk::{
        feature_set::FeatureSet,
        hash::Hash,
        message::{AccountKeys, TransactionSignatureDetails},
        pubkey::Pubkey,
        signature::Signature,
        transaction::Result,
    },
    solana_svm_transaction::{
        instruction::SVMInstruction, message_address_table_lookup::SVMMessageAddressTableLookup,
        svm_message::SVMMessage, svm_transaction::SVMTransaction,
    },
};

mod sdk_transactions;
mod transaction_view;

#[cfg_attr(feature = "dev-context-only-utils", derive(Clone))]
#[derive(Debug)]
pub struct RuntimeTransaction<T> {
    transaction: T,
    // transaction meta is a collection of fields, it is updated
    // during message state transition
    meta: TransactionMeta,
}

impl<T> StaticMeta for RuntimeTransaction<T> {
    fn message_hash(&self) -> &Hash {
        &self.meta.message_hash
    }
    fn is_simple_vote_transaction(&self) -> bool {
        self.meta.is_simple_vote_transaction
    }
    fn signature_details(&self) -> &TransactionSignatureDetails {
        &self.meta.signature_details
    }
    fn compute_budget_limits(&self, _feature_set: &FeatureSet) -> Result<ComputeBudgetLimits> {
        self.meta
            .compute_budget_instruction_details
            .sanitize_and_convert_to_compute_budget_limits()
    }
}

impl<T: SVMMessage> DynamicMeta for RuntimeTransaction<T> {}

impl<T> Deref for RuntimeTransaction<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

impl<T: SVMMessage> SVMMessage for RuntimeTransaction<T> {
    // override to access from the cached meta instead of re-calculating
    fn num_total_signatures(&self) -> u64 {
        self.meta.signature_details.total_signatures()
    }

    fn num_write_locks(&self) -> u64 {
        self.transaction.num_write_locks()
    }

    fn recent_blockhash(&self) -> &Hash {
        self.transaction.recent_blockhash()
    }

    fn num_instructions(&self) -> usize {
        self.transaction.num_instructions()
    }

    fn instructions_iter(&self) -> impl Iterator<Item = SVMInstruction> {
        self.transaction.instructions_iter()
    }

    fn program_instructions_iter(&self) -> impl Iterator<Item = (&Pubkey, SVMInstruction)> {
        self.transaction.program_instructions_iter()
    }

    fn account_keys(&self) -> AccountKeys {
        self.transaction.account_keys()
    }

    fn fee_payer(&self) -> &Pubkey {
        self.transaction.fee_payer()
    }

    fn is_writable(&self, index: usize) -> bool {
        self.transaction.is_writable(index)
    }

    fn is_signer(&self, index: usize) -> bool {
        self.transaction.is_signer(index)
    }

    fn is_invoked(&self, key_index: usize) -> bool {
        self.transaction.is_invoked(key_index)
    }

    fn num_lookup_tables(&self) -> usize {
        self.transaction.num_lookup_tables()
    }

    fn message_address_table_lookups(&self) -> impl Iterator<Item = SVMMessageAddressTableLookup> {
        self.transaction.message_address_table_lookups()
    }
}

impl<T: SVMTransaction> SVMTransaction for RuntimeTransaction<T> {
    fn signature(&self) -> &Signature {
        self.transaction.signature()
    }

    fn signatures(&self) -> &[Signature] {
        self.transaction.signatures()
    }
}
