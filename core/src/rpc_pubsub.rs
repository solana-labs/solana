//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::rpc_subscriptions::{RpcSubscriptions, RpcVote};
use jsonrpc_core::{Error, ErrorCode, Result};
use jsonrpc_derive::rpc;
use jsonrpc_pubsub::{typed::Subscriber, Session, SubscriptionId};
use solana_account_decoder::UiAccount;
use solana_client::{
    rpc_config::{
        RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSignatureSubscribeConfig,
        RpcTransactionLogsConfig, RpcTransactionLogsFilter,
    },
    rpc_response::{
        Response as RpcResponse, RpcKeyedAccount, RpcLogsResponse, RpcSignatureResult, SlotInfo,
    },
};
#[cfg(test)]
use solana_runtime::bank_forks::BankForks;
use solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature};
#[cfg(test)]
use std::sync::RwLock;
use std::{
    str::FromStr,
    sync::{atomic, Arc},
};

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
        subscriber: Subscriber<RpcResponse<UiAccount>>,
        pubkey_str: String,
        config: Option<RpcAccountInfoConfig>,
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
        config: Option<RpcProgramAccountsConfig>,
    );

    // Unsubscribe from account notification subscription.
    #[pubsub(
        subscription = "programNotification",
        unsubscribe,
        name = "programUnsubscribe"
    )]
    fn program_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId)
        -> Result<bool>;

    // Get logs for all transactions that reference the specified address
    #[pubsub(subscription = "logsNotification", subscribe, name = "logsSubscribe")]
    fn logs_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<RpcResponse<RpcLogsResponse>>,
        filter: RpcTransactionLogsFilter,
        config: RpcTransactionLogsConfig,
    );

    // Unsubscribe from logs notification subscription.
    #[pubsub(
        subscription = "logsNotification",
        unsubscribe,
        name = "logsUnsubscribe"
    )]
    fn logs_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool>;

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
        config: Option<RpcSignatureSubscribeConfig>,
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

    // Get notification when vote is encountered
    #[pubsub(subscription = "voteNotification", subscribe, name = "voteSubscribe")]
    fn vote_subscribe(&self, meta: Self::Metadata, subscriber: Subscriber<RpcVote>);

    // Unsubscribe from vote notification subscription.
    #[pubsub(
        subscription = "voteNotification",
        unsubscribe,
        name = "voteUnsubscribe"
    )]
    fn vote_unsubscribe(&self, meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool>;

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

pub struct RpcSolPubSubImpl {
    uid: Arc<atomic::AtomicUsize>,
    subscriptions: Arc<RpcSubscriptions>,
}

impl RpcSolPubSubImpl {
    pub fn new(subscriptions: Arc<RpcSubscriptions>) -> Self {
        let uid = Arc::new(atomic::AtomicUsize::default());
        Self { uid, subscriptions }
    }

    #[cfg(test)]
    fn default_with_bank_forks(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let uid = Arc::new(atomic::AtomicUsize::default());
        let subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(bank_forks));
        Self { uid, subscriptions }
    }
}

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
        subscriber: Subscriber<RpcResponse<UiAccount>>,
        pubkey_str: String,
        config: Option<RpcAccountInfoConfig>,
    ) {
        match param::<Pubkey>(&pubkey_str, "pubkey") {
            Ok(pubkey) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!("account_subscribe: account={:?} id={:?}", pubkey, sub_id);
                self.subscriptions
                    .add_account_subscription(pubkey, config, sub_id, subscriber)
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
        config: Option<RpcProgramAccountsConfig>,
    ) {
        match param::<Pubkey>(&pubkey_str, "pubkey") {
            Ok(pubkey) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!("program_subscribe: account={:?} id={:?}", pubkey, sub_id);
                self.subscriptions
                    .add_program_subscription(pubkey, config, sub_id, subscriber)
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

    fn logs_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: Subscriber<RpcResponse<RpcLogsResponse>>,
        filter: RpcTransactionLogsFilter,
        config: RpcTransactionLogsConfig,
    ) {
        info!("logs_subscribe");

        let (address, include_votes) = match filter {
            RpcTransactionLogsFilter::All => (None, false),
            RpcTransactionLogsFilter::AllWithVotes => (None, true),
            RpcTransactionLogsFilter::Mentions(addresses) => {
                match addresses.len() {
                    1 => match param::<Pubkey>(&addresses[0], "mentions") {
                        Ok(address) => (Some(address), false),
                        Err(e) => {
                            subscriber.reject(e).unwrap();
                            return;
                        }
                    },
                    _ => {
                        // Room is reserved in the API to support multiple addresses, but for now
                        // the implementation only supports one
                        subscriber
                            .reject(Error {
                                code: ErrorCode::InvalidParams,
                                message: "Invalid Request: Only 1 address supported".into(),
                                data: None,
                            })
                            .unwrap();
                        return;
                    }
                }
            }
        };

        let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
        let sub_id = SubscriptionId::Number(id as u64);
        self.subscriptions.add_logs_subscription(
            address,
            include_votes,
            config.commitment,
            sub_id,
            subscriber,
        )
    }

    fn logs_unsubscribe(&self, _meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
        info!("logs_unsubscribe: id={:?}", id);
        if self.subscriptions.remove_logs_subscription(&id) {
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
        signature_subscribe_config: Option<RpcSignatureSubscribeConfig>,
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
                    signature_subscribe_config,
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

    fn vote_subscribe(&self, _meta: Self::Metadata, subscriber: Subscriber<RpcVote>) {
        info!("vote_subscribe");
        let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("vote_subscribe: id={:?}", sub_id);
        self.subscriptions.add_vote_subscription(sub_id, subscriber);
    }

    fn vote_unsubscribe(&self, _meta: Option<Self::Metadata>, id: SubscriptionId) -> Result<bool> {
        info!("vote_unsubscribe");
        if self.subscriptions.remove_vote_subscription(&id) {
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
        cluster_info_vote_listener::{ClusterInfoVoteListener, VoteTracker},
        optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
        rpc_subscriptions::tests::robust_poll_or_panic,
    };
    use crossbeam_channel::unbounded;
    use jsonrpc_core::{futures::sync::mpsc, Response};
    use jsonrpc_pubsub::{PubSubHandler, Session};
    use serial_test_derive::serial;
    use solana_account_decoder::{parse_account_data::parse_account_data, UiAccountEncoding};
    use solana_client::rpc_response::{ProcessedSignatureResult, ReceivedSignatureResult};
    use solana_runtime::{
        bank::Bank,
        bank_forks::BankForks,
        commitment::{BlockCommitmentCache, CommitmentSlots},
        genesis_utils::{
            create_genesis_config, create_genesis_config_with_vote_accounts, GenesisConfigInfo,
            ValidatorVoteKeypairs,
        },
    };
    use solana_sdk::{
        commitment_config::CommitmentConfig,
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction, system_program, system_transaction,
        transaction::{self, Transaction},
    };
    use solana_stake_program::{
        self, stake_instruction,
        stake_state::{Authorized, Lockup, StakeAuthorize, StakeState},
    };
    use solana_vote_program::vote_transaction;
    use std::{
        sync::{atomic::AtomicBool, RwLock},
        thread::sleep,
        time::Duration,
    };

    fn process_transaction_and_notify(
        bank_forks: &RwLock<BankForks>,
        tx: &Transaction,
        subscriptions: &RpcSubscriptions,
        current_slot: Slot,
    ) -> transaction::Result<()> {
        bank_forks
            .write()
            .unwrap()
            .get(current_slot)
            .unwrap()
            .process_transaction(tx)?;
        let mut commitment_slots = CommitmentSlots::default();
        commitment_slots.slot = current_slot;
        subscriptions.notify_subscribers(commitment_slots);
        Ok(())
    }

    fn create_session() -> Arc<Session> {
        Arc::new(Session::new(mpsc::channel(1).0))
    }

    #[test]
    #[serial]
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
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let rpc = RpcSolPubSubImpl {
            subscriptions: Arc::new(RpcSubscriptions::new(
                &Arc::new(AtomicBool::new(false)),
                bank_forks.clone(),
                Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
                OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
            )),
            uid: Arc::new(atomic::AtomicUsize::default()),
        };

        // Test signature subscriptions
        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);

        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("signatureNotification");
        rpc.signature_subscribe(
            session,
            subscriber,
            tx.signatures[0].to_string(),
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::single()),
                ..RpcSignatureSubscribeConfig::default()
            }),
        );

        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 0).unwrap();

        // Test signature confirmation notification
        let (response, _) = robust_poll_or_panic(receiver);
        let expected_res =
            RpcSignatureResult::ProcessedSignature(ProcessedSignatureResult { err: None });
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

        // Test "received"
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("signatureNotification");
        rpc.signature_subscribe(
            session,
            subscriber,
            tx.signatures[0].to_string(),
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::single()),
                enable_received_notification: Some(true),
            }),
        );
        let received_slot = 1;
        rpc.subscriptions
            .notify_signatures_received((received_slot, vec![tx.signatures[0]]));
        // Test signature confirmation notification
        let (response, _) = robust_poll_or_panic(receiver);
        let expected_res =
            RpcSignatureResult::ReceivedSignature(ReceivedSignatureResult::ReceivedSignature);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "signatureNotification",
           "params": {
               "result": {
                   "context": { "slot": received_slot },
                   "value": expected_res,
               },
               "subscription": 1,
           }
        });
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
    }

    #[test]
    #[serial]
    fn test_signature_unsubscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        let session = create_session();

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks);
        io.extend_with(rpc.to_delegate());

        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signatures[0].to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[0]}"#;
        let res = io.handle_request_sync(&req, session.clone());

        let expected = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);

        // Test bad parameter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[1]}"#;
        let res = io.handle_request_sync(&req, session);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid Request: Subscription id does not exist"},"id":1}"#;
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    #[serial]
    fn test_account_subscribe() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);

        let new_stake_authority = solana_sdk::pubkey::new_rand();
        let stake_authority = Keypair::new();
        let from = Keypair::new();
        let stake_account = Keypair::new();
        let stake_program_id = solana_stake_program::id();
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);

        let rpc = RpcSolPubSubImpl {
            subscriptions: Arc::new(RpcSubscriptions::new(
                &Arc::new(AtomicBool::new(false)),
                bank_forks.clone(),
                Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                    1, 1,
                ))),
                OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
            )),
            uid: Arc::new(atomic::AtomicUsize::default()),
        };
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            stake_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::recent()),
                encoding: None,
                data_slice: None,
            }),
        );

        let tx = system_transaction::transfer(&alice, &from.pubkey(), 51, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 1).unwrap();

        let authorized = Authorized::auto(&stake_authority.pubkey());
        let ixs = stake_instruction::create_account(
            &from.pubkey(),
            &stake_account.pubkey(),
            &authorized,
            &Lockup::default(),
            51,
        );
        let message = Message::new(&ixs, Some(&from.pubkey()));
        let tx = Transaction::new(&[&from, &stake_account], message, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 1).unwrap();
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification #1
        let expected_data = bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .get_account(&stake_account.pubkey())
            .unwrap()
            .data;
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 1 },
                   "value": {
                       "owner": stake_program_id.to_string(),
                       "lamports": 51,
                       "data": bs58::encode(expected_data).into_string(),
                       "executable": false,
                       "rentEpoch": 0,
                   },
               },
               "subscription": 0,
           }
        });

        let (response, _) = robust_poll_or_panic(receiver);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);

        let tx = system_transaction::transfer(&alice, &stake_authority.pubkey(), 1, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 1).unwrap();
        sleep(Duration::from_millis(200));
        let ix = stake_instruction::authorize(
            &stake_account.pubkey(),
            &stake_authority.pubkey(),
            &new_stake_authority,
            StakeAuthorize::Staker,
        );
        let message = Message::new(&[ix], Some(&stake_authority.pubkey()));
        let tx = Transaction::new(&[&stake_authority], message, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 1).unwrap();
        sleep(Duration::from_millis(200));

        let bank = bank_forks.read().unwrap()[1].clone();
        let account = bank.get_account(&stake_account.pubkey()).unwrap();
        assert_eq!(
            StakeState::authorized_from(&account).unwrap().staker,
            new_stake_authority
        );
    }

    #[test]
    #[serial]
    fn test_account_subscribe_with_encoding() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);

        let nonce_account = Keypair::new();
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);

        let rpc = RpcSolPubSubImpl {
            subscriptions: Arc::new(RpcSubscriptions::new(
                &Arc::new(AtomicBool::new(false)),
                bank_forks.clone(),
                Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                    1, 1,
                ))),
                OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
            )),
            uid: Arc::new(atomic::AtomicUsize::default()),
        };
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            nonce_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::recent()),
                encoding: Some(UiAccountEncoding::JsonParsed),
                data_slice: None,
            }),
        );

        let ixs = system_instruction::create_nonce_account(
            &alice.pubkey(),
            &nonce_account.pubkey(),
            &alice.pubkey(),
            100,
        );
        let message = Message::new(&ixs, Some(&alice.pubkey()));
        let tx = Transaction::new(&[&alice, &nonce_account], message, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 1).unwrap();
        sleep(Duration::from_millis(200));

        // Test signature confirmation notification #1
        let expected_data = bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .get_account(&nonce_account.pubkey())
            .unwrap()
            .data;
        let expected_data = parse_account_data(
            &nonce_account.pubkey(),
            &system_program::id(),
            &expected_data,
            None,
        )
        .unwrap();
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 1 },
                   "value": {
                       "owner": system_program::id().to_string(),
                       "lamports": 100,
                       "data": expected_data,
                       "executable": false,
                       "rentEpoch": 0,
                   },
               },
               "subscription": 0,
           }
        });

        let (response, _) = robust_poll_or_panic(receiver);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
    }

    #[test]
    #[serial]
    fn test_account_unsubscribe() {
        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let session = create_session();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::new(&genesis_config))));

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks);

        io.extend_with(rpc.to_delegate());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            bob_pubkey.to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[0]}"#;
        let res = io.handle_request_sync(&req, session.clone());

        let expected = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);

        // Test bad parameter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[1]}"#;
        let res = io.handle_request_sync(&req, session);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid Request: Subscription id does not exist"},"id":1}"#;
        let expected: Response = serde_json::from_str(&expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    #[should_panic]
    fn test_account_commitment_not_fulfilled() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bob = Keypair::new();

        let mut rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks.clone());
        let exit = Arc::new(AtomicBool::new(false));
        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        );
        rpc.subscriptions = Arc::new(subscriptions);
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            bob.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::root()),
                encoding: None,
                data_slice: None,
            }),
        );

        let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
        bank_forks
            .write()
            .unwrap()
            .get(1)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        rpc.subscriptions
            .notify_subscribers(CommitmentSlots::default());
        // allow 200ms for notification thread to wake
        std::thread::sleep(Duration::from_millis(200));
        let _panic = robust_poll_or_panic(receiver);
    }

    #[test]
    fn test_account_commitment() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let bank0 = bank_forks.read().unwrap().get(0).unwrap().clone();
        let bank1 = Bank::new_from_parent(&bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bob = Keypair::new();

        let mut rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks.clone());
        let exit = Arc::new(AtomicBool::new(false));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests()));

        let subscriptions = RpcSubscriptions::new(
            &exit,
            bank_forks.clone(),
            block_commitment_cache,
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        );
        rpc.subscriptions = Arc::new(subscriptions);
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            bob.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::root()),
                encoding: None,
                data_slice: None,
            }),
        );

        let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
        bank_forks
            .write()
            .unwrap()
            .get(1)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        let mut commitment_slots = CommitmentSlots::default();
        commitment_slots.slot = 1;
        rpc.subscriptions.notify_subscribers(commitment_slots);

        let commitment_slots = CommitmentSlots {
            slot: 2,
            root: 1,
            highest_confirmed_slot: 1,
            highest_confirmed_root: 1,
        };
        rpc.subscriptions.notify_subscribers(commitment_slots);
        let expected = json!({
           "jsonrpc": "2.0",
           "method": "accountNotification",
           "params": {
               "result": {
                   "context": { "slot": 1 },
                   "value": {
                       "owner": system_program::id().to_string(),
                       "lamports": 100,
                       "data": "",
                       "executable": false,
                       "rentEpoch": 0,
                   },
               },
               "subscription": 0,
           }
        });
        let (response, _) = robust_poll_or_panic(receiver);
        assert_eq!(serde_json::to_string(&expected).unwrap(), response);
    }

    #[test]
    #[serial]
    fn test_slot_subscribe() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks);
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
    #[serial]
    fn test_slot_unsubscribe() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks);
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

    #[test]
    #[serial]
    fn test_vote_subscribe() {
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests()));

        let validator_voting_keypairs: Vec<_> =
            (0..10).map(|_| ValidatorVoteKeypairs::new_rand()).collect();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
            10_000,
            &validator_voting_keypairs,
            vec![100; validator_voting_keypairs.len()],
        );
        let exit = Arc::new(AtomicBool::new(false));
        let bank = Bank::new(&genesis_config);
        let bank_forks = BankForks::new(bank);
        let bank = bank_forks.get(0).unwrap().clone();
        let bank_forks = Arc::new(RwLock::new(bank_forks));

        // Setup RPC
        let mut rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks.clone());
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("voteNotification");

        // Setup Subscriptions
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = RpcSubscriptions::new_with_vote_subscription(
            &exit,
            bank_forks,
            block_commitment_cache,
            optimistically_confirmed_bank,
            true,
        );
        rpc.subscriptions = Arc::new(subscriptions);
        rpc.vote_subscribe(session, subscriber);

        // Create some voters at genesis
        let vote_tracker = VoteTracker::new(&bank);
        let (votes_sender, votes_receiver) = unbounded();
        let (vote_tracker, validator_voting_keypairs) =
            (Arc::new(vote_tracker), validator_voting_keypairs);

        let vote_slots = vec![1, 2];
        validator_voting_keypairs.iter().for_each(|keypairs| {
            let node_keypair = &keypairs.node_keypair;
            let vote_keypair = &keypairs.vote_keypair;
            let vote_tx = vote_transaction::new_vote_transaction(
                vote_slots.clone(),
                Hash::default(),
                Hash::default(),
                node_keypair,
                vote_keypair,
                vote_keypair,
                None,
            );
            votes_sender.send(vec![vote_tx]).unwrap();
        });

        // Process votes and check they were notified.
        let (s, _r) = unbounded();
        let (_replay_votes_sender, replay_votes_receiver) = unbounded();
        ClusterInfoVoteListener::get_and_process_votes_for_tests(
            &votes_receiver,
            &vote_tracker,
            &bank,
            &rpc.subscriptions,
            &s,
            &replay_votes_receiver,
        )
        .unwrap();

        let (response, _) = robust_poll_or_panic(receiver);
        assert_eq!(
            response,
            r#"{"jsonrpc":"2.0","method":"voteNotification","params":{"result":{"hash":"11111111111111111111111111111111","slots":[1,2],"timestamp":null},"subscription":0}}"#
        );
    }

    #[test]
    #[serial]
    fn test_vote_unsubscribe() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks);
        let session = create_session();
        let (subscriber, _id_receiver, _) = Subscriber::new_test("voteNotification");
        rpc.vote_subscribe(session, subscriber);

        let session = create_session();
        assert!(rpc
            .vote_unsubscribe(Some(session), SubscriptionId::Number(42))
            .is_err());

        let session = create_session();
        assert!(rpc
            .vote_unsubscribe(Some(session), SubscriptionId::Number(0))
            .is_ok());
    }
}
