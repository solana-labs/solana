use {
    crate::transaction_meta::StaticMeta,
    solana_sdk::transaction::{SanitizedTransaction, VersionedTransaction},
    solana_svm_transaction::svm_transaction::SVMTransaction,
    std::borrow::Cow,
};

pub trait TransactionWithMeta: StaticMeta + SVMTransaction {
    // Required to interact with geyser plugins.
    fn as_sanitized_transaction(&self) -> Cow<SanitizedTransaction>;
    fn to_versioned_transaction(&self) -> VersionedTransaction;
}
