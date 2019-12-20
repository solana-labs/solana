//! The `pubsub` module implements a threaded subscription service on client RPC request

use core::hash::Hash;
use jsonrpc_core::futures::Future;
use jsonrpc_pubsub::{typed::Sink, SubscriptionId};
use serde::Serialize;
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::Account, clock::Slot, pubkey::Pubkey, signature::Signature, transaction,
};
use solana_vote_program::vote_state::MAX_LOCKOUT_HISTORY;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type Confirmations = usize;

#[derive(Serialize, Clone)]
pub struct SlotInfo {
    pub slot: Slot,
    pub parent: Slot,
    pub root: Slot,
}

type RpcAccountSubscriptions =
    RwLock<HashMap<Pubkey, HashMap<SubscriptionId, (Sink<Account>, Confirmations)>>>;
type RpcProgramSubscriptions =
    RwLock<HashMap<Pubkey, HashMap<SubscriptionId, (Sink<(String, Account)>, Confirmations)>>>;
type RpcSignatureSubscriptions = RwLock<
    HashMap<Signature, HashMap<SubscriptionId, (Sink<transaction::Result<()>>, Confirmations)>>,
>;
type RpcSlotSubscriptions = RwLock<HashMap<SubscriptionId, Sink<SlotInfo>>>;

fn add_subscription<K, S>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, (Sink<S>, Confirmations)>>,
    hashmap_key: &K,
    confirmations: Option<Confirmations>,
    sub_id: &SubscriptionId,
    sink: &Sink<S>,
) where
    K: Eq + Hash + Clone + Copy,
    S: Clone,
{
    let confirmations = confirmations.unwrap_or(0);
    let confirmations = if confirmations > MAX_LOCKOUT_HISTORY {
        MAX_LOCKOUT_HISTORY
    } else {
        confirmations
    };
    if let Some(current_hashmap) = subscriptions.get_mut(hashmap_key) {
        current_hashmap.insert(sub_id.clone(), (sink.clone(), confirmations));
        return;
    }
    let mut hashmap = HashMap::new();
    hashmap.insert(sub_id.clone(), (sink.clone(), confirmations));
    subscriptions.insert(*hashmap_key, hashmap);
}

fn remove_subscription<K, S>(
    subscriptions: &mut HashMap<K, HashMap<SubscriptionId, (Sink<S>, Confirmations)>>,
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

fn check_confirmations_and_notify<K, S, F, N, X>(
    subscriptions: &HashMap<K, HashMap<SubscriptionId, (Sink<S>, Confirmations)>>,
    hashmap_key: &K,
    current_slot: Slot,
    bank_forks: &Arc<RwLock<BankForks>>,
    bank_method: F,
    notify: N,
) where
    K: Eq + Hash + Clone + Copy,
    S: Clone + Serialize,
    F: Fn(&Bank, &K) -> X,
    N: Fn(X, &Sink<S>, u64),
    X: Clone + Serialize,
{
    let current_ancestors = bank_forks
        .read()
        .unwrap()
        .get(current_slot)
        .unwrap()
        .ancestors
        .clone();
    if let Some(hashmap) = subscriptions.get(hashmap_key) {
        for (_bank_sub_id, (sink, confirmations)) in hashmap.iter() {
            let desired_slot: Vec<u64> = current_ancestors
                .iter()
                .filter(|(_, &v)| v == *confirmations)
                .map(|(k, _)| k)
                .cloned()
                .collect();
            let root: Vec<u64> = current_ancestors
                .iter()
                .filter(|(_, &v)| v == 32)
                .map(|(k, _)| k)
                .cloned()
                .collect();
            let root = if root.len() == 1 { root[0] } else { 0 };
            if desired_slot.len() == 1 {
                let desired_bank = bank_forks
                    .read()
                    .unwrap()
                    .get(desired_slot[0])
                    .unwrap()
                    .clone();
                let result = bank_method(&desired_bank, hashmap_key);
                notify(result, &sink, root);
            }
        }
    }
}

fn notify_account<S>(result: Option<(S, Slot)>, sink: &Sink<S>, root: Slot)
where
    S: Clone + Serialize,
{
    if let Some((account, fork)) = result {
        if fork >= root {
            sink.notify(Ok(account)).wait().unwrap();
        }
    }
}

fn notify_signature<S>(result: Option<S>, sink: &Sink<S>, _root: Slot)
where
    S: Clone + Serialize,
{
    if let Some(result) = result {
        sink.notify(Ok(result)).wait().unwrap();
    }
}

fn notify_program(accounts: Vec<(Pubkey, Account)>, sink: &Sink<(String, Account)>, _root: Slot) {
    for (pubkey, account) in accounts.iter() {
        sink.notify(Ok((pubkey.to_string(), account.clone())))
            .wait()
            .unwrap();
    }
}

pub struct RpcSubscriptions {
    account_subscriptions: RpcAccountSubscriptions,
    program_subscriptions: RpcProgramSubscriptions,
    signature_subscriptions: RpcSignatureSubscriptions,
    slot_subscriptions: RpcSlotSubscriptions,
}

impl Default for RpcSubscriptions {
    fn default() -> Self {
        RpcSubscriptions {
            account_subscriptions: RpcAccountSubscriptions::default(),
            program_subscriptions: RpcProgramSubscriptions::default(),
            signature_subscriptions: RpcSignatureSubscriptions::default(),
            slot_subscriptions: RpcSlotSubscriptions::default(),
        }
    }
}

impl RpcSubscriptions {
    pub fn check_account(
        &self,
        pubkey: &Pubkey,
        current_slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        let subscriptions = self.account_subscriptions.read().unwrap();
        check_confirmations_and_notify(
            &subscriptions,
            pubkey,
            current_slot,
            bank_forks,
            Bank::get_account_modified_since_parent,
            notify_account,
        );
    }

    pub fn check_program(
        &self,
        program_id: &Pubkey,
        current_slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        let subscriptions = self.program_subscriptions.write().unwrap();
        check_confirmations_and_notify(
            &subscriptions,
            program_id,
            current_slot,
            bank_forks,
            Bank::get_program_accounts_modified_since_parent,
            notify_program,
        );
    }

    pub fn check_signature(
        &self,
        signature: &Signature,
        current_slot: Slot,
        bank_forks: &Arc<RwLock<BankForks>>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        check_confirmations_and_notify(
            &subscriptions,
            signature,
            current_slot,
            bank_forks,
            Bank::get_signature_status,
            notify_signature,
        );
        subscriptions.remove(&signature);
    }

    pub fn add_account_subscription(
        &self,
        pubkey: &Pubkey,
        confirmations: Option<Confirmations>,
        sub_id: &SubscriptionId,
        sink: &Sink<Account>,
    ) {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, pubkey, confirmations, sub_id, sink);
    }

    pub fn remove_account_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.account_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    pub fn add_program_subscription(
        &self,
        program_id: &Pubkey,
        confirmations: Option<Confirmations>,
        sub_id: &SubscriptionId,
        sink: &Sink<(String, Account)>,
    ) {
        let mut subscriptions = self.program_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, program_id, confirmations, sub_id, sink);
    }

    pub fn remove_program_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.program_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    pub fn add_signature_subscription(
        &self,
        signature: &Signature,
        confirmations: Option<Confirmations>,
        sub_id: &SubscriptionId,
        sink: &Sink<transaction::Result<()>>,
    ) {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        add_subscription(&mut subscriptions, signature, confirmations, sub_id, sink);
    }

    pub fn remove_signature_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.signature_subscriptions.write().unwrap();
        remove_subscription(&mut subscriptions, id)
    }

    /// Notify subscribers of changes to any accounts or new signatures since
    /// the bank's last checkpoint.
    pub fn notify_subscribers(&self, current_slot: Slot, bank_forks: &Arc<RwLock<BankForks>>) {
        let pubkeys: Vec<_> = {
            let subs = self.account_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for pubkey in &pubkeys {
            self.check_account(pubkey, current_slot, bank_forks);
        }

        let programs: Vec<_> = {
            let subs = self.program_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for program_id in &programs {
            self.check_program(program_id, current_slot, bank_forks);
        }

        let signatures: Vec<_> = {
            let subs = self.signature_subscriptions.read().unwrap();
            subs.keys().cloned().collect()
        };
        for signature in &signatures {
            self.check_signature(signature, current_slot, bank_forks);
        }
    }

    pub fn add_slot_subscription(&self, sub_id: &SubscriptionId, sink: &Sink<SlotInfo>) {
        let mut subscriptions = self.slot_subscriptions.write().unwrap();
        subscriptions.insert(sub_id.clone(), sink.clone());
    }

    pub fn remove_slot_subscription(&self, id: &SubscriptionId) -> bool {
        let mut subscriptions = self.slot_subscriptions.write().unwrap();
        subscriptions.remove(id).is_some()
    }

    pub fn notify_slot(&self, slot: Slot, parent: Slot, root: Slot) {
        let subscriptions = self.slot_subscriptions.read().unwrap();
        for (_, sink) in subscriptions.iter() {
            sink.notify(Ok(SlotInfo { slot, parent, root }))
                .wait()
                .unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use jsonrpc_pubsub::typed::Subscriber;
    use solana_budget_program;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use tokio::prelude::{Async, Stream};

    #[test]
    fn test_check_account_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_budget_program::id(),
        );
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("accountNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_account_subscription(&alice.pubkey(), None, &sub_id, &sink);

        assert!(subscriptions
            .account_subscriptions
            .read()
            .unwrap()
            .contains_key(&alice.pubkey()));

        subscriptions.check_account(&alice.pubkey(), 0, &bank_forks);
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(
                r#"{{"jsonrpc":"2.0","method":"accountNotification","params":{{"result":{{"data":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"executable":false,"lamports":1,"owner":[2,203,81,223,225,24,34,35,203,214,138,130,144,208,35,77,63,16,87,51,47,198,115,123,98,188,19,160,0,0,0,0],"rent_epoch":1}},"subscription":0}}}}"#
            );
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
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let alice = Keypair::new();
        let tx = system_transaction::create_account(
            &mint_keypair,
            &alice,
            blockhash,
            1,
            16,
            &solana_budget_program::id(),
        );
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("programNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_program_subscription(&solana_budget_program::id(), None, &sub_id, &sink);

        assert!(subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_budget_program::id()));

        subscriptions.check_program(&solana_budget_program::id(), 0, &bank_forks);
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(
                r#"{{"jsonrpc":"2.0","method":"programNotification","params":{{"result":["{:?}",{{"data":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"executable":false,"lamports":1,"owner":[2,203,81,223,225,24,34,35,203,214,138,130,144,208,35,77,63,16,87,51,47,198,115,123,98,188,19,160,0,0,0,0],"rent_epoch":1}}],"subscription":0}}}}"#,
                alice.pubkey()
            );
            assert_eq!(expected, response);
        }

        subscriptions.remove_program_subscription(&sub_id);
        assert!(!subscriptions
            .program_subscriptions
            .read()
            .unwrap()
            .contains_key(&solana_budget_program::id()));
    }
    #[test]
    fn test_check_signature_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(100);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let alice = Keypair::new();
        let tx = system_transaction::transfer(&mint_keypair, &alice.pubkey(), 20, blockhash);
        let signature = tx.signatures[0];
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();

        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("signatureNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_signature_subscription(&signature, None, &sub_id, &sink);

        assert!(subscriptions
            .signature_subscriptions
            .read()
            .unwrap()
            .contains_key(&signature));

        subscriptions.check_signature(&signature, 0, &bank_forks);
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected_res: Option<transaction::Result<()>> = Some(Ok(()));
            let expected_res_str =
                serde_json::to_string(&serde_json::to_value(expected_res).unwrap()).unwrap();
            let expected = format!(
                r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":{},"subscription":0}}}}"#,
                expected_res_str
            );
            assert_eq!(expected, response);
        }

        subscriptions.remove_signature_subscription(&sub_id);
        assert!(!subscriptions
            .signature_subscriptions
            .read()
            .unwrap()
            .contains_key(&signature));
    }
    #[test]
    fn test_check_slot_subscribe() {
        let (subscriber, _id_receiver, mut transport_receiver) =
            Subscriber::new_test("slotNotification");
        let sub_id = SubscriptionId::Number(0 as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();
        let subscriptions = RpcSubscriptions::default();
        subscriptions.add_slot_subscription(&sub_id, &sink);

        assert!(subscriptions
            .slot_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));

        subscriptions.notify_slot(0, 0, 0);
        let string = transport_receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected_res = SlotInfo {
                parent: 0,
                slot: 0,
                root: 0,
            };
            let expected_res_str =
                serde_json::to_string(&serde_json::to_value(expected_res).unwrap()).unwrap();
            let expected = format!(
                r#"{{"jsonrpc":"2.0","method":"slotNotification","params":{{"result":{},"subscription":0}}}}"#,
                expected_res_str
            );
            assert_eq!(expected, response);
        }

        subscriptions.remove_slot_subscription(&sub_id);
        assert!(!subscriptions
            .slot_subscriptions
            .read()
            .unwrap()
            .contains_key(&sub_id));
    }
}
