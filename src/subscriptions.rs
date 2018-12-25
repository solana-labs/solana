use crate::bank::{BankError, Result};
use crate::jsonrpc_macros::pubsub::Sink;
use crate::rpc::RpcSignatureStatus;
use hashbrown::HashMap;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::Transaction;
use std::sync::RwLock;
use tokio::prelude::Future;

pub struct Subscriptions {
    // Mapping of account ids to Subscriber ids and sinks to notify on userdata update
    pub accounts: RwLock<HashMap<Pubkey, HashMap<Pubkey, Sink<Account>>>>,
    // Mapping of signatures to Subscriber ids and sinks to notify on confirmation
    pub signatures: RwLock<HashMap<Signature, HashMap<Pubkey, Sink<RpcSignatureStatus>>>>,
}

impl Default for Subscriptions {
    fn default() -> Self {
        Self {
            accounts: RwLock::new(HashMap::new()),
            signatures: RwLock::new(HashMap::new()),
        }
    }
}

impl Subscriptions {
    pub fn send_account_notifications(
        &self,
        txs: &[Transaction],
        res: &[Result<()>],
        loaded: &[Result<Vec<Account>>],
    ) {
        for (i, racc) in loaded.iter().enumerate() {
            if res[i].is_err() || racc.is_err() {
                continue;
            }

            let tx = &txs[i];
            let acc = racc.as_ref().unwrap();
            for (key, account) in tx.account_keys.iter().zip(acc.iter()) {
                self.check_account_subscriptions(&key, account);
            }
        }
    }
    pub fn add_account_subscription(
        &self,
        bank_sub_id: Pubkey,
        pubkey: Pubkey,
        sink: Sink<Account>,
    ) {
        let mut subscriptions = self.accounts.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(&pubkey) {
            current_hashmap.insert(bank_sub_id, sink);
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(bank_sub_id, sink);
        subscriptions.insert(pubkey, hashmap);
    }
    pub fn remove_account_subscription(&self, bank_sub_id: &Pubkey, pubkey: &Pubkey) -> bool {
        let mut subscriptions = self.accounts.write().unwrap();
        match subscriptions.get_mut(pubkey) {
            Some(ref current_hashmap) if current_hashmap.len() == 1 => {}
            Some(current_hashmap) => {
                return current_hashmap.remove(bank_sub_id).is_some();
            }
            None => {
                return false;
            }
        }
        subscriptions.remove(pubkey).is_some()
    }

    pub fn check_account_subscriptions(&self, pubkey: &Pubkey, account: &Account) {
        let subscriptions = self.accounts.read().unwrap();
        if let Some(hashmap) = subscriptions.get(pubkey) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(account.clone())).wait().unwrap();
            }
        }
    }

    pub fn add_signature_subscription(
        &self,
        bank_sub_id: Pubkey,
        signature: Signature,
        sink: Sink<RpcSignatureStatus>,
    ) {
        let mut subscriptions = self.signatures.write().unwrap();
        if let Some(current_hashmap) = subscriptions.get_mut(&signature) {
            current_hashmap.insert(bank_sub_id, sink);
            return;
        }
        let mut hashmap = HashMap::new();
        hashmap.insert(bank_sub_id, sink);
        subscriptions.insert(signature, hashmap);
    }

    pub fn remove_signature_subscription(
        &self,
        bank_sub_id: &Pubkey,
        signature: &Signature,
    ) -> bool {
        let mut subscriptions = self.signatures.write().unwrap();
        match subscriptions.get_mut(signature) {
            Some(ref current_hashmap) if current_hashmap.len() == 1 => {}
            Some(current_hashmap) => {
                return current_hashmap.remove(bank_sub_id).is_some();
            }
            None => {
                return false;
            }
        }
        subscriptions.remove(signature).is_some()
    }

    pub fn check_signature_subscriptions(&self, signature: &Signature, status: RpcSignatureStatus) {
        let mut subscriptions = self.signatures.write().unwrap();
        if let Some(hashmap) = subscriptions.get(signature) {
            for (_bank_sub_id, sink) in hashmap.iter() {
                sink.notify(Ok(status)).wait().unwrap();
            }
        }
        subscriptions.remove(&signature);
    }
    pub fn update_transaction_statuses(&self, txs: &[Transaction], res: &[Result<()>]) {
        for (i, tx) in txs.iter().enumerate() {
            let status = match res[i] {
                Ok(_) => RpcSignatureStatus::Confirmed,
                Err(BankError::AccountInUse) => RpcSignatureStatus::AccountInUse,
                Err(BankError::ProgramError(_, _)) => RpcSignatureStatus::ProgramRuntimeError,
                Err(_) => RpcSignatureStatus::GenericFailure,
            };
            if status != RpcSignatureStatus::SignatureNotFound {
                self.check_signature_subscriptions(&tx.signatures[0], status);
            }
        }
    }
}
