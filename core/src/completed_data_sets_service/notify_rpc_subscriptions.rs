//! Notifies RPC subscribers that a transaction with the specified signature was received.
//!
//! See [`solana_rpc::rpc_subscriptions::NotificationEntry::Subscribed`].

use {
    super::{CompletedDataSetHandler, SlotEntries},
    solana_entry::entry::Entry,
    solana_rpc::rpc_subscriptions::RpcSubscriptions,
    solana_sdk::signature::Signature,
    std::sync::Arc,
};

pub struct NotifyRpcSubscriptions {
    subscriptions: Arc<RpcSubscriptions>,
}

impl NotifyRpcSubscriptions {
    pub fn handler(subscriptions: Arc<RpcSubscriptions>) -> CompletedDataSetHandler {
        let me = Self { subscriptions };
        Box::new(move |entries| me.handle(entries))
    }

    fn handle(&self, SlotEntries { slot, entries, .. }: SlotEntries) {
        let transactions = get_transaction_signatures(entries);
        if !transactions.is_empty() {
            self.subscriptions
                .notify_signatures_received((slot, transactions));
        }
    }
}

fn get_transaction_signatures(entries: &[Entry]) -> Vec<Signature> {
    entries
        .iter()
        .flat_map(|e| {
            e.transactions
                .iter()
                .filter_map(|t| t.signatures.first().cloned())
        })
        .collect::<Vec<Signature>>()
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        solana_sdk::{
            hash::Hash,
            signature::{Keypair, Signer},
            transaction::Transaction,
        },
    };

    #[test]
    fn test_zero_signatures() {
        let tx = Transaction::new_with_payer(&[], None);
        let entries = vec![Entry::new(&Hash::default(), 1, vec![tx])];
        let signatures = get_transaction_signatures(&entries);
        assert!(signatures.is_empty());
    }

    #[test]
    fn test_multi_signatures() {
        let kp = Keypair::new();
        let tx =
            Transaction::new_signed_with_payer(&[], Some(&kp.pubkey()), &[&kp], Hash::default());
        let entries = vec![Entry::new(&Hash::default(), 1, vec![tx.clone()])];
        let signatures = get_transaction_signatures(&entries);
        assert_eq!(signatures.len(), 1);

        let entries = vec![
            Entry::new(&Hash::default(), 1, vec![tx.clone(), tx.clone()]),
            Entry::new(&Hash::default(), 1, vec![tx]),
        ];
        let signatures = get_transaction_signatures(&entries);
        assert_eq!(signatures.len(), 3);
    }
}
