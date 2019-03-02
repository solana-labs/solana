//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::rpc_status::RpcSignatureStatus;
use jsonrpc_core::futures::Future;
use jsonrpc_pubsub::typed::Sink;
use jsonrpc_pubsub::SubscriptionId;
use solana_runtime::bank::{self, Bank, BankError};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::collections::HashMap;
use std::sync::RwLock;

type RpcAccountSubscriptions = RwLock<HashMap<Pubkey, HashMap<SubscriptionId, Sink<Account>>>>;
type RpcSignatureSubscriptions =
    RwLock<HashMap<Signature, HashMap<SubscriptionId, Sink<RpcSignatureStatus>>>>;
pub struct RpcSubscriptions {
    account_subscriptions: RpcAccountSubscriptions,
    signature_subscriptions: RpcSignatureSubscriptions,
}

impl Default for RpcSubscriptions {
    fn default() -> Self {
        RpcSubscriptions {
            account_subscriptions: RpcAccountSubscriptions::default(),
            signature_subscriptions: RpcSignatureSubscriptions::default(),
        }
    }
}

impl RpcSubscriptions {
    pub fn check_account(&self, pubkey: &Pubkey, account: &Account) {
        let subscriptions = self.account_subscriptions.read().unwrap();
        if let Some(hashmap) = subscriptions.get(pubkey) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(account.clone())).wait().unwrap();
            }
        }
    }

    pub fn check_signature(&self, signature: &Signature, bank_error: &bank::Result<()>) {
        let status = match bank_error {
            Ok(_) => RpcSignatureStatus::Confirmed,
            Err(BankError::AccountInUse) => RpcSignatureStatus::AccountInUse,
            Err(BankError::ProgramError(_, _)) => RpcSignatureStatus::ProgramRuntimeError,
            Err(_) => RpcSignatureStatus::GenericFailure,
        };

        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(hashmap) = subscriptions.get(signature) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(status)).wait().unwrap();
            }
        }
        subscriptions.remove(&signature);
    }

    pub fn add_account_subscription(
        &self,
        pubkey: &Pubkey,
        sub_id: &SubscriptionId,
        sink: &Sink<Account>,
    ) {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(pubkey) {
            current_hashmap.insert(sub_id.clone(), sink.clone());
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(sub_id.clone(), sink.clone());
        subscriptions.insert(*pubkey, hashmap);
    }

    pub fn remove_account_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        let mut found = false;
        subscriptions.retain(|_, v| {
            v.retain(|k, _| {
                if *k == *id {
                    found = true;
                }
                !found
            });
            !v.is_empty()
        });
        found
    }

    pub fn add_signature_subscription(
        &self,
        signature: &Signature,
        sub_id: &SubscriptionId,
        sink: &Sink<RpcSignatureStatus>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(signature) {
            current_hashmap.insert(sub_id.clone(), sink.clone());
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(sub_id.clone(), sink.clone());
        subscriptions.insert(*signature, hashmap);
    }

    pub fn remove_signature_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        let mut found = false;
        subscriptions.retain(|_, v| {
            v.retain(|k, _| {
                if *k == *id {
                    found = true;
                }
                !found
            });
            !v.is_empty()
        });
        found
    }

    /// Notify subscribers of changes to any accounts or new signatures since
    /// the bank's last checkpoint.
    pub fn notify_subscribers(&self, bank: &Bank) {
        let pubkeys: Vec<_> = {
            let subs = self.account_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for pubkey in &pubkeys {
            if let Some(account) = &bank.get_account_modified_since_parent(pubkey) {
                self.check_account(pubkey, account);
            }
        }

        let signatures: Vec<_> = {
            let subs = self.signature_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for signature in &signatures {
            let status = bank.get_signature_status(signature).unwrap();
            self.check_signature(signature, &status);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonrpc_pubsub::typed::Subscriber;
    use solana_sdk::budget_program;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;
    use tokio::prelude::{Async, Stream};

    #[test]
    fn test_check_account_subscribe() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let last_id = bank.last_block_hash();
        let tx = SystemTransaction::new_program_account(
            &mint_keypair,
            alice.pubkey(),
            last_id,
            1,
            16,
            budget_program::id(),
            0,
        );
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("accountNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_account_subscription(&alice.pubkey(), &sub_id, &sink);

        assert!(subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));

        let account = bank.get_account(&alice.pubkey()).unwrap();
        subscriptions.check_account(&alice.pubkey(), &account);
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"accountNotification","params":{{"result":{{"executable":false,"owner":[129,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"tokens":1,"userdata":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}},"subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        subscriptions.remove_account_subscription(&sub_id);
        assert!(!subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));
    }
    #[test]
    fn test_check_signature_subscribe() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let last_id = bank.last_block_hash();
        let tx = SystemTransaction::new_move(&mint_keypair, alice.pubkey(), 20, last_id, 0);
        let signature = tx.signatures[0];
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("signatureNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_signature_subscription(&signature, &sub_id, &sink);

        assert!(subscriptions
            .signature_subscriptions
            .read()
            .unwrap()
            .contains_key(&signature));

        subscriptions.check_signature(&signature, &Ok(()));
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":"Confirmed","subscription":0}}}}"#);
            assert_eq!(expected, response);
        }

        subscriptions.remove_signature_subscription(&sub_id);
        assert!(!subscriptions
            .signature_subscriptions
            .read()
            .unwrap()
            .contains_key(&signature));
    }
}
