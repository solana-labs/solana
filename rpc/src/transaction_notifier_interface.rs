use {
    solana_entry::entry::Entry,
    solana_sdk::{clock::Slot, signature::Signature, transaction::SanitizedTransaction},
    solana_transaction_status::TransactionStatusMeta,
    std::sync::{Arc, RwLock},
};

pub trait TransactionNotifier {
    fn notify_transaction(
        &self,
        slot: Slot,
        transaction_slot_index: usize,
        signature: &Signature,
        transaction_status_meta: &TransactionStatusMeta,
        transaction: &SanitizedTransaction,
    );

    fn notify_entry(&self, slot: Slot, index: usize, entry: &Entry);
}

pub type TransactionNotifierLock = Arc<RwLock<dyn TransactionNotifier + Sync + Send>>;
