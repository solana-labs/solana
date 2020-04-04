//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::rpc_subscriptions::{Confirmations, RpcSubscriptions, SlotInfo};
use jsonrpc_core::{Error, ErrorCode, Result};
use jsonrpc_derive::rpc;
use jsonrpc_pubsub::{typed::Subscriber, Session, SubscriptionId};
use solana_client::rpc_response::{
    Response as RpcResponse, RpcAccount, RpcKeyedAccount, RpcSignatureResult,
};
use solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature};
use std::sync::{atomic, Arc};

// Suppress needless_return due to
//   https://github.com/paritytech/jsonrpc/blob/2d38e6424d8461cdf72e78425ce67d51af9c6586/derive/src/lib.rs#L204
// Once https://github.com/paritytech/jsonrpc/issues/418 is resolved, try to remove this clippy allow
#[allow(clippy::needless_return)]
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
    fn account_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<RpcResponse<RpcAccount>>,
        pubkey_str: String,
        confirmations: Option<Confirmations>,
    );

    // Unsubscribe from account notification subscription.
    #[pubsub(
        subscription = "accountNotification",
        unsubscribe,
        name = "accountUnsubscribe"
    )]
    fn account_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId)
        -> Result<bool>;

    // Get notification every time account data owned by a particular program is changed
    // Accepts pubkey parameter as base-58 encoded string
    #[pubsub(
        subscription = "programNotification",
        subscribe,
        name = "programSubscribe"
    )]
    fn program_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<RpcResponse<RpcKeyedAccount>>,
        pubkey_str: String,
        confirmations: Option<Confirmations>,
    );

    // Unsubscribe from account notification subscription.
    #[pubsub(
        subscription = "programNotification",
        unsubscribe,
        name = "programUnsubscribe"
    )]
    fn program_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId)
        -> Result<bool>;

    // Get notification when signature is verified
    // Accepts signature parameter as base-58 encoded string
    #[pubsub(
        subscription = "signatureNotification",
        subscribe,
        name = "signatureSubscribe"
    )]
    fn signature_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<RpcResponse<RpcSignatureResult>>,
        signature_str: String,
        confirmations: Option<Confirmations>,
    );

    // Unsubscribe from signature notification subscription.
    #[pubsub(
        subscription = "signatureNotification",
        unsubscribe,
        name = "signatureUnsubscribe"
    )]
    fn signature_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: SubscriptionId,
    ) -> Result<bool>;

    // Get notification when slot is encountered
    #[pubsub(subscription = "slotNotification", subscribe, name = "slotSubscribe")]
    fn slot_subscribe(&self, meta: Self::Metadata, subscriber: Subscriber<SlotInfo>);

    // Unsubscribe from slot notification subscription.
    #[pubsub(
        subscription = "slotNotification",
        unsubscribe,
        name = "slotUnsubscribe"
    )]
    fn slot_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool>;

    // Get notification when a new root is set
    #[pubsub(subscription = "rootNotification", subscribe, name = "rootSubscribe")]
    fn root_subscribe(&self, meta: Self::Metadata, subscriber: Subscriber<Slot>);

    // Unsubscribe from slot notification subscription.
    #[pubsub(
        subscription = "rootNotification",
        unsubscribe,
        name = "rootUnsubscribe"
    )]
    fn root_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool>;
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

use std::str::FromStr;

fn param<T: FromStr>(param_str: &str, thing: &str) -> Result<T> {
    param_str.parse::<T>().map_err(|_e| Error {
        code: ErrorCode::InvalidParams,
        message: format!("Invalid Request: Invalid {} provided", thing),
        data: None,
    })
}

impl RpcSolPubSub for RpcSolPubSubImpl {
    type Metadata = Arc<Session>;

    fn account_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: Subscriber<RpcResponse<RpcAccount>>,
        pubkey_str: String,
        confirmations: Option<Confirmations>,
    ) {
        match param::<Pubkey>(&pubkey_str, "pubkey") {
            Ok(pubkey) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!("account_subscribe: account={:?} id={:?}", pubkey, sub_id);
                self.subscriptions.add_account_subscription(
                    pubkey,
                    confirmations,
                    sub_id,
                    subscriber,
                )
            }
            Err(e) => subscriber.reject(e).unwrap(),
        }
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
        subscriber: Subscriber<RpcResponse<RpcKeyedAccount>>,
        pubkey_str: String,
        confirmations: Option<Confirmations>,
    ) {
        match param::<Pubkey>(&pubkey_str, "pubkey") {
            Ok(pubkey) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!("program_subscribe: account={:?} id={:?}", pubkey, sub_id);
                self.subscriptions.add_program_subscription(
                    pubkey,
                    confirmations,
                    sub_id,
                    subscriber,
                )
            }
            Err(e) => subscriber.reject(e).unwrap(),
        }
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
        subscriber: Subscriber<RpcResponse<RpcSignatureResult>>,
        signature_str: String,
        confirmations: Option<Confirmations>,
    ) {
        info!("signature_subscribe");
        match param::<Signature>(&signature_str, "signature") {
            Ok(signature) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!(
                    "signature_subscribe: signature={:?} id={:?}",
                    signature, sub_id
                );
                self.subscriptions.add_signature_subscription(
                    signature,
                    confirmations,
                    sub_id,
                    subscriber,
                );
            }
            Err(e) => subscriber.reject(e).unwrap(),
        }
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

    fn slot_subscribe(&self, _meta: Self::Metadata, subscriber: Subscriber<SlotInfo>) {
        info!("slot_subscribe");
        let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("slot_subscribe: id={:?}", sub_id);
        self.subscriptions.add_slot_subscription(sub_id, subscriber);
    }

    fn slot_unsubscribe(&self, _meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
        info!("slot_unsubscribe");
        if self.subscriptions.remove_slot_subscription(&id) {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid Request: Subscription id does not exist".into(),
                data: None,
            })
        }
    }

    fn root_subscribe(&self, _meta: Self::Metadata, subscriber: Subscriber<Slot>) {
        info!("root_subscribe");
        let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("root_subscribe: id={:?}", sub_id);
        self.subscriptions.add_root_subscription(sub_id, subscriber);
    }

    fn root_unsubscribe(&self, _meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
        info!("root_unsubscribe");
        if self.subscriptions.remove_root_subscription(&id) {
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
    use crate::{
        commitment::{BlockCommitment, BlockCommitmentCache},
        rpc_subscriptions::tests::robust_poll_or_panic,
    };
    use jsonrpc_core::{futures::sync::mpsc, Response};
    use jsonrpc_pubsub::{PubSubHandler, Session};
    use solana_budget_program::{self, budget_instruction};
    use solana_ledger::bank_forks::BankForks;
    use solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo};
    use solana_runtime::bank::Bank;
    use solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_program, system_transaction,
        transaction::{self, Transaction},
    };
    use std::{
        collections::HashMap,
        sync::{atomic::AtomicBool, RwLock},
        thread::sleep,
        time::Duration,
    };

    fn process_transaction_and_notify(
        bank_forks: &Arc<RwLock<BankForks>>,
        tx: &Transaction,
        subscriptions: &RpcSubscriptions,
    ) -> transaction::Result<()> {
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(tx)?;
        subscriptions.notify_subscribers(0, &bank_forks);
        Ok(())
    }

    fn create_session() -> Arc<Session> {
        Arc::new(Session::new(mpsc::channel(1).0))
    }

    #[test]
    fn test_signature_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bob = Keypair::new();
        let bob_pubkey = bob.pubkey();
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let rpc = RpcSolPubSubImpl {
            subscriptions: Arc::new(RpcSubscriptions::new(
                &Arc::new(AtomicBool::new(false)),
                Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            )),
            ..RpcSolPubSubImpl::default()
        };

        // Test signature subscriptions
        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);

        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("signatureNotification");
        rpc.signature_subscribe(session, subscriber, tx.signatures[0].to_string(), None);

        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions).unwrap();

        // Test signature confirmation notification
        let (response, _) = robust_poll_or_panic(receiver);
        let expected_res = RpcSignatureResult { err: None };
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "signatureNotification",
           "params": {
               "result": {
                   "context": { "slot": 0 },
                   "value": expected_res,
               },
               "subscription": 0,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
    }

    #[test]
    fn test_signature_unsubscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bob_pubkey = Pubkey::new_rand();
        let bank = Bank::new(&genesis_config);
        let arc_bank = Arc::new(bank);
        let blockhash = arc_bank.last_blockhash();

        let session = create_session();

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default();
        io.extend_with(rpc.to_delegate());

        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
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
        let expected = format!(
            r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#
        );
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn test_account_subscribe() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);

        // This test depends on the budget program
        genesis_config
            .native_instruction_processors
            .push(solana_budget_program!());

        let bob_pubkey = Pubkey::new_rand();
        let witness = Keypair::new();
        let contract_funds = Keypair::new();
        let contract_state = Keypair::new();
        let budget_program_id = solana_budget_program::id();
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));

        let rpc = RpcSolPubSubImpl {
            subscriptions: Arc::new(RpcSubscriptions::new(
                &Arc::new(AtomicBool::new(false)),
                Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            )),
            ..RpcSolPubSubImpl::default()
        };
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            contract_state.pubkey().to_string(),
            None,
        );

        let tx = system_transaction::transfer(&alice, &contract_funds.pubkey(), 51, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions).unwrap();

        let ixs = budget_instruction::when_signed(
            &contract_funds.pubkey(),
            &bob_pubkey,
            &contract_state.pubkey(),
            &witness.pubkey(),
            None,
            51,
        );
        let tx = Transaction::new_signed_instructions(
            &[&contract_funds, &contract_state],
            ixs,
            blockhash,
        );
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification #1
        let expected_data = bank_forks
            .read()
            .unwrap()
            .get(0)
            .unwrap()
            .get_account(&contract_state.pubkey())
            .unwrap()
            .data;
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 0 },
                   "value": {
                       "owner": budget_program_id.to_string(),
                       "lamports": 51,
                       "data": bs58::encode(expected_data).into_string(),
                       "executable": false,
                       "rentEpoch": 1,
                   },
               },
               "subscription": 0,
           }
        });

        let (response, _) = robust_poll_or_panic(receiver);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        let tx = system_transaction::transfer(&alice, &witness.pubkey(), 1, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));
        let ix = budget_instruction::apply_signature(
            &witness.pubkey(),
            &contract_state.pubkey(),
            &bob_pubkey,
        );
        let tx = Transaction::new_signed_instructions(&[&witness], vec![ix], blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions).unwrap();
        sleep(Duration::from_millis(200));

        assert_eq!(
            bank_forks
                .read()
                .unwrap()
                .get(0)
                .unwrap()
                .get_account(&contract_state.pubkey()),
            None
        );
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
        let expected = format!(
            r#"{{"jsonrpc":"2.0","error":{{"code":-32602,"message":"Invalid Request: Subscription id does not exist"}},"id":1}}"#
        );
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    #[should_panic]
    fn test_account_confirmations_not_fulfilled() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let bob = Keypair::new();

        let mut rpc = RpcSolPubSubImpl::default();
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
        );
        rpc.subscriptions = Arc::new(subscriptions);
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(session, subscriber, bob.pubkey().to_string(), Some(2));

        let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        rpc.subscriptions.notify_subscribers(0, &bank_forks);
        // allow 200ms for notification thread to wake
        std::thread::sleep(Duration::from_millis(200));
        let _panic = robust_poll_or_panic(receiver);
    }

    #[test]
    fn test_account_confirmations() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(0, bank)));
        let bob = Keypair::new();

        let mut rpc = RpcSolPubSubImpl::default();
        let exit = Arc::new(AtomicBool::new(false));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests()));

        let subscriptions = RpcSubscriptions::new(&exit, block_commitment_cache.clone());
        rpc.subscriptions = Arc::new(subscriptions);
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(session, subscriber, bob.pubkey().to_string(), Some(2));

        let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
        bank_forks
            .write()
            .unwrap()
            .get(0)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        rpc.subscriptions.notify_subscribers(0, &bank_forks);

        let bank0 = bank_forks.read().unwrap()[0].clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bank1 = bank_forks.read().unwrap()[1].clone();

        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(1, 10);
        let mut block_commitment = HashMap::new();
        block_commitment.entry(0).or_insert(cache0.clone());
        let mut new_block_commitment =
            BlockCommitmentCache::new(block_commitment, 10, bank1.clone(), 0);
        let mut w_block_commitment_cache = block_commitment_cache.write().unwrap();
        std::mem::swap(&mut *w_block_commitment_cache, &mut new_block_commitment);
        drop(w_block_commitment_cache);

        rpc.subscriptions.notify_subscribers(1, &bank_forks);
        let bank2 = Bank::new_from_parent(&bank1, &Pubkey::default(), 2);
        bank_forks.write().unwrap().insert(bank2);
        let bank2 = bank_forks.read().unwrap()[2].clone();

        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(2, 10);
        let mut block_commitment = HashMap::new();
        block_commitment.entry(0).or_insert(cache0.clone());
        let mut new_block_commitment = BlockCommitmentCache::new(block_commitment, 10, bank2, 0);
        let mut w_block_commitment_cache = block_commitment_cache.write().unwrap();
        std::mem::swap(&mut *w_block_commitment_cache, &mut new_block_commitment);
        drop(w_block_commitment_cache);

        rpc.subscriptions.notify_subscribers(2, &bank_forks);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 0 },
                   "value": {
                       "owner": system_program::id().to_string(),
                       "lamports": 100,
                       "data": "",
                       "executable": false,
                       "rentEpoch": 1,
                   },
               },
               "subscription": 0,
           }
        });
        let (response, _) = robust_poll_or_panic(receiver);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
    }

    #[test]
    fn test_slot_subscribe() {
        let rpc = RpcSolPubSubImpl::default();
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("slotNotification");
        rpc.slot_subscribe(session, subscriber);

        rpc.subscriptions.notify_slot(0, 0, 0);
        // Test slot confirmation notification
        let (response, _) = robust_poll_or_panic(receiver);
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

    #[test]
    fn test_slot_unsubscribe() {
        let rpc = RpcSolPubSubImpl::default();
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("slotNotification");
        rpc.slot_subscribe(session, subscriber);
        rpc.subscriptions.notify_slot(0, 0, 0);
        let (response, _) = robust_poll_or_panic(receiver);
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

        let session = create_session();
        assert!(rpc
            .slot_unsubscribe(Some(session), SubscriptionId::Number(42))
            .is_err());

        let session = create_session();
        assert!(rpc
            .slot_unsubscribe(Some(session), SubscriptionId::Number(0))
            .is_ok());
    }
}
