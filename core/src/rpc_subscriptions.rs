//! The `pubsub` module implements a threaded subscription service on client RPC request

use bs58;
use core::hash::Hash;
use jsonrpc_core::futures::Future;
use jsonrpc_pubsub::typed::Sink;
use jsonrpc_pubsub::SubscriptionId;
use solana_runtime::bank::Bank;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction;
use std::collections::HashMap;
use std::sync::RwLock;

type RpcAccountSubscriptions = RwLock<HashMap<Pubkey, HashMap<SubscriptionId, Sink<Account>>>>;
type RpcProgramSubscriptions =
    RwLock<HashMap<Pubkey, HashMap<SubscriptionId, Sink<(String, Account)>>>>;
type RpcSignatureSubscriptions =
    RwLock<HashMap<Signature, HashMap<SubscriptionId, Sink<Option<transaction::Result<()>>>>>>;

fn add_subscription<K, S>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, Sink<S>>>,
    hashmap_key: &K,
    sub_id: &SubscriptionId,
    sink: &Sink<S>,
) where
    K: Eq + Hash + Clone + Copy,
    S: Clone,
{
    if let Some(current_hashmap) = subscriptions.get_mut(hashmap_key) {
        current_hashmap.insert(sub_id.clone(), sink.clone());
        return;
    }
    let mut hashmap = HashMap::new();
    hashmap.insert(sub_id.clone(), sink.clone());
    subscriptions.insert(*hashmap_key, hashmap);
}

fn remove_subscription<K, S>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, Sink<S>>>,
    sub_id: &SubscriptionId,
) -> bool
where
    K: Eq + Hash + Clone + Copy,
    S: Clone,
{
    let mut found = false;
    subscriptions.retain(|_, v| {
        v.retain(|k, _| {
            if *k == *sub_id {
                found = true;
            }
            !found
        });
        !v.is_empty()
    });
    found
}

pub struct RpcSubscriptions {
    account_subscriptions: RpcAccountSubscriptions,
    program_subscriptions: RpcProgramSubscriptions,
    signature_subscriptions: RpcSignatureSubscriptions,
}

impl Default for RpcSubscriptions {
    fn default() -> Self {
        RpcSubscriptions {
            account_subscriptions: RpcAccountSubscriptions::default(),
            program_subscriptions: RpcProgramSubscriptions::default(),
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

    pub fn check_program(&self, program_id: &Pubkey, pubkey: &Pubkey, account: &Account) {
        let subscriptions = self.program_subscriptions.write().unwrap();
        if let Some(hashmap) = subscriptions.get(program_id) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok((bs58::encode(pubkey).into_string(), account.clone())))
                    .wait()
                    .unwrap();
            }
        }
    }

    pub fn check_signature(&self, signature: &Signature, bank_error: &transaction::Result<()>) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        if let Some(hashmap) = subscriptions.get(signature) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(Some(bank_error.clone()))).wait().unwrap();
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
        add_subscription(&mut subscriptions, pubkey, sub_id, sink);
    }

    pub fn remove_account_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    pub fn add_program_subscription(
        &self,
        program_id: &Pubkey,
        sub_id: &SubscriptionId,
        sink: &Sink<(String, Account)>,
    ) {
        let mut subscriptions = self.program_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, program_id, sub_id, sink);
    }

    pub fn remove_program_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.program_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    pub fn add_signature_subscription(
        &self,
        signature: &Signature,
        sub_id: &SubscriptionId,
        sink: &Sink<Option<transaction::Result<()>>>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, signature, sub_id, sink);
    }

    pub fn remove_signature_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
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

        let programs: Vec<_> = {
            let subs = self.program_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for program_id in &programs {
            let accounts = &bank.get_program_accounts_modified_since_parent(program_id);
            for (pubkey, account) in accounts.iter() {
                self.check_program(program_id, pubkey, account);
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
    use solana_budget_api;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use tokio::prelude::{Async, Stream};

    #[test]
    fn test_check_account_subscribe() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let blockhash = bank.last_blockhash();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice.pubkey(),
            blockhash,
            1,
            16,
            &solana_budget_api::id(),
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
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"accountNotification","params":{{"result":{{"data":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"executable":false,"lamports":1,"owner":[2,203,81,223,225,24,34,35,203,214,138,130,144,208,35,77,63,16,87,51,47,198,115,123,98,188,19,160,0,0,0,0]}},"subscription":0}}}}"#);
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
    fn test_check_program_subscribe() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let blockhash = bank.last_blockhash();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice.pubkey(),
            blockhash,
            1,
            16,
            &solana_budget_api::id(),
            0,
        );
        bank.process_transaction(&tx).unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("programNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_program_subscription(&solana_budget_api::id(), &sub_id, &sink);

        assert!(subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_budget_api::id()));

        let account = bank.get_account(&alice.pubkey()).unwrap();
        subscriptions.check_program(&solana_budget_api::id(), &alice.pubkey(), &account);
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"programNotification","params":{{"result":["{:?}",{{"data":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"executable":false,"lamports":1,"owner":[2,203,81,223,225,24,34,35,203,214,138,130,144,208,35,77,63,16,87,51,47,198,115,123,98,188,19,160,0,0,0,0]}}],"subscription":0}}}}"#, alice.pubkey());
            assert_eq!(expected, response);
        }

        subscriptions.remove_program_subscription(&sub_id);
        assert!(!subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_budget_api::id()));
    }
    #[test]
    fn test_check_signature_subscribe() {
        let (genesis_block, mint_keypair) = GenesisBlock::new(100);
        let bank = Bank::new(&genesis_block);
        let alice = Keypair::new();
        let blockhash = bank.last_blockhash();
        let tx = system_transaction::transfer(&mint_keypair, &alice.pubkey(), 20, blockhash, 0);
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
            let expected_res: Option<transaction::Result<()>> = Some(Ok(()));
            let expected_res_str =
                serde_json::to_string(&serde_json::to_value(expected_res).unwrap()).unwrap();
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":{},"subscription":0}}}}"#, expected_res_str);
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
