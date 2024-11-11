use {
    crate::transaction_meta::StaticMeta,
    core::borrow::Borrow,
    solana_sdk::transaction::{SanitizedTransaction, VersionedTransaction},
    solana_svm_transaction::svm_transaction::SVMTransaction,
};

pub trait TransactionWithMeta: StaticMeta + SVMTransaction {
    // Required to interact with geyser plugins.
    fn as_sanitized_transaction(&self) -> impl Borrow<SanitizedTransaction>;
    fn to_versioned_transaction(&self) -> VersionedTransaction;
}
