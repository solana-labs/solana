//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::rpc_subscriptions::RpcSubscriptions;
use bs58;
use jsonrpc_core::{Error, ErrorCode, Result};
use jsonrpc_derive::rpc;
use jsonrpc_pubsub::typed::Subscriber;
use jsonrpc_pubsub::{Session, SubscriptionId};
use solana_client::rpc_signature_status::RpcSignatureStatus;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use std::mem;
use std::sync::{atomic, Arc};

#[rpc]
pub trait RpcSolPubSub {
    type Metadata;

    // Get notification every time account data is changed
    // Accepts pubkey parameter as base-58 encoded string
    #[pubsub(
        subscription = "accountNotification",
        subscribe,
        name = "accountSubscribe"
    )]
    fn account_subscribe(&self, _: Self::Metadata, _: Subscriber<Account>, _: String);

    // Unsubscribe from account notification subscription.
    #[pubsub(
        subscription = "accountNotification",
        unsubscribe,
        name = "accountUnsubscribe"
    )]
    fn account_unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;

    // Get notification every time account data owned by a particular program is changed
    // Accepts pubkey parameter as base-58 encoded string
    #[pubsub(
        subscription = "programNotification",
        subscribe,
        name = "programSubscribe"
    )]
    fn program_subscribe(&self, _: Self::Metadata, _: Subscriber<(String, Account)>, _: String);

    // Unsubscribe from account notification subscription.
    #[pubsub(
        subscription = "programNotification",
        unsubscribe,
        name = "programUnsubscribe"
    )]
    fn program_unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;

    // Get notification when signature is verified
    // Accepts signature parameter as base-58 encoded string
    #[pubsub(
        subscription = "signatureNotification",
        subscribe,
        name = "signatureSubscribe"
    )]
    fn signature_subscribe(&self, _: Self::Metadata, _: Subscriber<RpcSignatureStatus>, _: String);

    // Unsubscribe from signature notification subscription.
    #[pubsub(
        subscription = "signatureNotification",
        unsubscribe,
        name = "signatureUnsubscribe"
    )]
    fn signature_unsubscribe(&self, _: Option<Self::Metadata>, _: SubscriptionId) -> Result<bool>;
}

#[derive(Default)]
pub struct RpcSolPubSubImpl {
    uid: Arc<atomic::AtomicUsize>,
    subscriptions: Arc<RpcSubscriptions>,
}

impl RpcSolPubSubImpl {
    pub fn new(subscriptions: Arc<RpcSubscriptions>) -> Self {
        let uid = Arc::new(atomic::AtomicUsize::default());
        Self { uid, subscriptions }
    }
}

impl RpcSolPubSub for RpcSolPubSubImpl {
    type Metadata = Arc<Session>;

    fn account_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: Subscriber<Account>,
        pubkey_str: String,
    ) {
        let pubkey_vec = bs58::decode(pubkey_str).into_vec().unwrap();
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            subscriber
                .reject(Error {
                    code: ErrorCode::InvalidParams,
                    message: "Invalid Request: Invalid pubkey provided".into(),
                    data: None,
                })
                .unwrap();
            return;
        }
        let pubkey = Pubkey::new(&pubkey_vec);

        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("account_subscribe: account={:?} id={:?}", pubkey, sub_id);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();

        self.subscriptions
            .add_account_subscription(&pubkey, &sub_id, &sink)
    }

    fn account_unsubscribe(
        &self,
        _meta: Option<Self::Metadata>,
        id: SubscriptionId,
    ) -> Result<bool> {
        info!("account_unsubscribe: id={:?}", id);
        if self.subscriptions.remove_account_subscription(&id) {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }

    fn program_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: Subscriber<(String, Account)>,
        pubkey_str: String,
    ) {
        let pubkey_vec = bs58::decode(pubkey_str).into_vec().unwrap();
        if pubkey_vec.len() != mem::size_of::<Pubkey>() {
            subscriber
                .reject(Error {
                    code: ErrorCode::InvalidParams,
                    message: "Invalid Request: Invalid pubkey provided".into(),
                    data: None,
                })
                .unwrap();
            return;
        }
        let pubkey = Pubkey::new(&pubkey_vec);

        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("program_subscribe: account={:?} id={:?}", pubkey, sub_id);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();

        self.subscriptions
            .add_program_subscription(&pubkey, &sub_id, &sink)
    }

    fn program_unsubscribe(
        &self,
        _meta: Option<Self::Metadata>,
        id: SubscriptionId,
    ) -> Result<bool> {
        info!("program_unsubscribe: id={:?}", id);
        if self.subscriptions.remove_program_subscription(&id) {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }

    fn signature_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: Subscriber<RpcSignatureStatus>,
        signature_str: String,
    ) {
        info!("signature_subscribe");
        let signature_vec = bs58::decode(signature_str).into_vec().unwrap();
        if signature_vec.len() != mem::size_of::<Signature>() {
            subscriber
                .reject(Error {
                    code: ErrorCode::InvalidParams,
                    message: "Invalid Request: Invalid signature provided".into(),
                    data: None,
                })
                .unwrap();
            return;
        }
        let signature = Signature::new(&signature_vec);
        let id = self.uid.fetch_add(1, atomic::Ordering::SeqCst);
        let sub_id = SubscriptionId::Number(id as u64);
        let sink = subscriber.assign_id(sub_id.clone()).unwrap();

        self.subscriptions
            .add_signature_subscription(&signature, &sub_id, &sink);
    }

    fn signature_unsubscribe(
        &self,
        _meta: Option<Self::Metadata>,
        id: SubscriptionId,
    ) -> Result<bool> {
        info!("signature_unsubscribe");
        if self.subscriptions.remove_signature_subscription(&id) {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonrpc_core::futures::sync::mpsc;
    use jsonrpc_core::Response;
    use jsonrpc_pubsub::{PubSubHandler, Session};
    use solana_budget_api;
    use solana_budget_api::budget_instruction::BudgetInstruction;
    use solana_runtime::bank::{self, Bank};
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction::SystemTransaction;
    use solana_sdk::transaction::Transaction;
    use std::thread::sleep;
    use std::time::Duration;
    use tokio::prelude::{Async, Stream};

    fn process_transaction_and_notify(
        bank: &Arc<Bank>,
        tx: &Transaction,
        subscriptions: &RpcSubscriptions,
    ) -> bank::Result<Arc<Bank>> {
        bank.process_transaction(tx)?;
        subscriptions.notify_subscribers(&bank);

        // Simulate a block boundary
        Ok(Arc::new(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.slot() + 1,
        )))
    }

    fn create_session() -> Arc<Session> {
        Arc::new(Session::new(mpsc::channel(1).0))
    }

    #[test]
    fn test_signature_subscribe() {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bob = Keypair::new();
        let bob_pubkey = bob.pubkey();
        let bank = Bank::new(&genesis_block);
        let arc_bank = Arc::new(bank);
        let blockhash = arc_bank.last_blockhash();

        let rpc = RpcSolPubSubImpl::default();

        // Test signature subscriptions
        let tx = SystemTransaction::new_move(&alice, &bob_pubkey, 20, blockhash, 0);

        let session = create_session();
        let (subscriber, _id_receiver, mut receiver) =
            Subscriber::new_test("signatureNotification");
        rpc.signature_subscribe(session, subscriber, tx.signatures[0].to_string());

        process_transaction_and_notify(&arc_bank, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification
        let string = receiver.poll();
        if let Async::Ready(Some(response)) = string.unwrap() {
            let expected = format!(r#"{{"jsonrpc":"2.0","method":"signatureNotification","params":{{"result":"Confirmed","subscription":0}}}}"#);
            assert_eq!(expected, response);
        }
    }

    #[test]
    fn test_signature_unsubscribe() {
        let (genesis_block, alice) = GenesisBlock::new(10_000);
        let bob_pubkey = Pubkey::new_rand();
        let bank = Bank::new(&genesis_block);
        let arc_bank = Arc::new(bank);
        let blockhash = arc_bank.last_blockhash();

        let session = create_session();

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();
        io.extend_with(rpc.to_delegate());

        let tx = SystemTransaction::new_move(&alice, &bob_pubkey, 20, blockhash, 0);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signatures[0].to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[0]}}"#);
        let res = io.handle_request_sync(&req, session.clone());

        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);

        // Test bad parameter
        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[1]}}"#);
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#);
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn test_account_subscribe() {
        let (mut genesis_block, alice) = GenesisBlock::new(10_000);

        // This test depends on the budget program
        genesis_block
            .native_programs
            .push(("solana_budget_program".to_string(), solana_budget_api::id()));

        let bob_pubkey = Pubkey::new_rand();
        let witness = Keypair::new();
        let contract_funds = Keypair::new();
        let contract_state = Keypair::new();
        let budget_program_id = solana_budget_api::id();
        let executable = false; // TODO
        let bank = Bank::new(&genesis_block);
        let arc_bank = Arc::new(bank);
        let blockhash = arc_bank.last_blockhash();

        let rpc = RpcSolPubSubImpl::default();
        let session = create_session();
        let (subscriber, _id_receiver, mut receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(session, subscriber, contract_state.pubkey().to_string());

        let tx = SystemTransaction::new_account(&alice, &contract_funds.pubkey(), 51, blockhash, 0);
        let arc_bank = process_transaction_and_notify(&arc_bank, &tx, &rpc.subscriptions).unwrap();

        let ixs = BudgetInstruction::new_when_signed(
            &contract_funds.pubkey(),
            &bob_pubkey,
            &contract_state.pubkey(),
            &witness.pubkey(),
            None,
            51,
        );
        let tx = Transaction::new_signed_instructions(&[&contract_funds], ixs, blockhash);
        let arc_bank = process_transaction_and_notify(&arc_bank, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification #1
        let string = receiver.poll();
        let expected_data = arc_bank.get_account(&contract_state.pubkey()).unwrap().data;
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "owner": budget_program_id,
                   "lamports": 51,
                   "data": expected_data,
                    "executable": executable,
               },
               "subscription": 0,
           }
        });

        if let Async::Ready(Some(response)) = string.unwrap() {
            assert_eq!(serde_json::to_string(&expected).unwrap(), response);
        }

        let tx = SystemTransaction::new_account(&alice, &witness.pubkey(), 1, blockhash, 0);
        let arc_bank = process_transaction_and_notify(&arc_bank, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));
        let ix = BudgetInstruction::new_apply_signature(
            &witness.pubkey(),
            &contract_state.pubkey(),
            &bob_pubkey,
        );
        let tx = Transaction::new_signed_instructions(&[&witness], vec![ix], blockhash);
        let arc_bank = process_transaction_and_notify(&arc_bank, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));

        assert_eq!(arc_bank.get_account(&contract_state.pubkey()), None);
    }

    #[test]
    fn test_account_unsubscribe() {
        let bob_pubkey = Pubkey::new_rand();
        let session = create_session();

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();

        io.extend_with(rpc.to_delegate());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            bob_pubkey.to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[0]}}"#);
        let res = io.handle_request_sync(&req, session.clone());

        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);

        // Test bad parameter
        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[1]}}"#);
        let res = io.handle_request_sync(&req, session.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#);
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);
    }
}
