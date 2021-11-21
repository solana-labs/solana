use {
    solana_sdk::{clock::Slot, signature::Signature, transaction::SanitizedTransaction},
    solana_transaction_status::TransactionStatusMeta,
    std::sync::{Arc, RwLock},
};

pub trait TransactionNotifierInterface {
    fn notify_transaction(
        &self,
        slot: Slot,
        signature: &Signature,
        transaction_status_meta: &TransactionStatusMeta,
        transaction: &SanitizedTransaction,
    );
}

pub type TransactionNotifier = Arc<RwLock<dyn TransactionNotifierInterface + Sync + Send>>;
