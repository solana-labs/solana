use {
    core::borrow::Borrow,
    solana_sdk::transaction::{SanitizedTransaction, VersionedTransaction},
    solana_svm_transaction::svm_transaction::SVMTransaction,
};

/// Adapter trait to allow for conversion to legacy transaction types
/// that are required by some outside interfaces.
/// Eventually this trait should go away, but for now it is necessary.
pub trait SVMTransactionAdapter: SVMTransaction {
    fn as_sanitized_transaction(&self) -> impl Borrow<SanitizedTransaction>;
    fn to_versioned_transaction(&self) -> VersionedTransaction;
}

impl SVMTransactionAdapter for SanitizedTransaction {
    #[inline]
    fn as_sanitized_transaction(&self) -> impl Borrow<SanitizedTransaction> {
        self
    }

    #[inline]
    fn to_versioned_transaction(&self) -> VersionedTransaction {
        self.to_versioned_transaction()
    }
}
