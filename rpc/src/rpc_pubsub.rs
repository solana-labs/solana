//! The `pubsub` module implements a threaded subscription service on client RPC request

#[cfg(test)]
use solana_runtime::bank_forks::BankForks;
#[cfg(test)]
use std::sync::RwLock;
use {
    crate::rpc_subscriptions::{RpcSubscriptions, RpcVote},
    jsonrpc_core::{Error, ErrorCode, Result},
    jsonrpc_derive::rpc,
    jsonrpc_pubsub::{typed::Subscriber, Session, SubscriptionId},
    solana_account_decoder::UiAccount,
    solana_client::{
        rpc_config::{
            RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSignatureSubscribeConfig,
            RpcTransactionLogsConfig, RpcTransactionLogsFilter,
        },
        rpc_response::{
            Response as RpcResponse, RpcKeyedAccount, RpcLogsResponse, RpcSignatureResult,
            SlotInfo, SlotUpdate,
        },
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature},
    std::{
        str::FromStr,
        sync::{atomic, Arc},
    },
};

pub const MAX_ACTIVE_SUBSCRIPTIONS: usize = 100_000;

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
        config: Option<RpcTransactionLogsConfig>,
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

    // Get series of updates for all slots
    #[pubsub(
        subscription = "slotsUpdatesNotification",
        subscribe,
        name = "slotsUpdatesSubscribe"
    )]
    fn slots_updates_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<Arc<SlotUpdate>>,
    );

    // Unsubscribe from slots updates notification subscription.
    #[pubsub(
        subscription = "slotsUpdatesNotification",
        unsubscribe,
        name = "slotsUpdatesUnsubscribe"
    )]
    fn slots_updates_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: SubscriptionId,
    ) -> Result<bool>;

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
    max_active_subscriptions: usize,
}

impl RpcSolPubSubImpl {
    pub fn new(subscriptions: Arc<RpcSubscriptions>, max_active_subscriptions: usize) -> Self {
        let uid = Arc::new(atomic::AtomicUsize::default());
        Self {
            uid,
            subscriptions,
            max_active_subscriptions,
        }
    }

    #[cfg(test)]
    fn default_with_bank_forks(bank_forks: Arc<RwLock<BankForks>>) -> Self {
        let uid = Arc::new(atomic::AtomicUsize::default());
        let subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(bank_forks));
        let max_active_subscriptions = MAX_ACTIVE_SUBSCRIPTIONS;
        Self {
            uid,
            subscriptions,
            max_active_subscriptions,
        }
    }

    fn check_subscription_count(&self) -> Result<()> {
        let num_subscriptions = self.subscriptions.total();
        debug!("Total existing subscriptions: {}", num_subscriptions);
        if num_subscriptions >= self.max_active_subscriptions {
            info!("Node subscription limit reached");
            datapoint_info!("rpc-subscription", ("total", num_subscriptions, i64));
            inc_new_counter_info!("rpc-subscription-refused-limit-reached", 1);
            Err(Error {
                code: ErrorCode::InternalError,
                message: "Internal Error: Subscription refused. Node subscription limit reached"
                    .into(),
                data: None,
            })
        } else {
            datapoint_info!("rpc-subscription", ("total", num_subscriptions + 1, i64));
            Ok(())
        }
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
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
        match param::<Pubkey>(&pubkey_str, "pubkey") {
            Ok(pubkey) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!("account_subscribe: account={:?} id={:?}", pubkey, sub_id);
                self.subscriptions
                    .add_account_subscription(pubkey, config, sub_id, subscriber)
            }
            Err(e) => subscriber.reject(e).unwrap_or_default(),
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
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
        match param::<Pubkey>(&pubkey_str, "pubkey") {
            Ok(pubkey) => {
                let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
                let sub_id = SubscriptionId::Number(id as u64);
                info!("program_subscribe: account={:?} id={:?}", pubkey, sub_id);
                self.subscriptions
                    .add_program_subscription(pubkey, config, sub_id, subscriber)
            }
            Err(e) => subscriber.reject(e).unwrap_or_default(),
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
        config: Option<RpcTransactionLogsConfig>,
    ) {
        info!("logs_subscribe");
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }

        let (address, include_votes) = match filter {
            RpcTransactionLogsFilter::All => (None, false),
            RpcTransactionLogsFilter::AllWithVotes => (None, true),
            RpcTransactionLogsFilter::Mentions(addresses) => {
                match addresses.len() {
                    1 => match param::<Pubkey>(&addresses[0], "mentions") {
                        Ok(address) => (Some(address), false),
                        Err(e) => {
                            subscriber.reject(e).unwrap_or_default();
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
                            .unwrap_or_default();
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
            config.and_then(|config| config.commitment),
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
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
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
            Err(e) => subscriber.reject(e).unwrap_or_default(),
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
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
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

    fn slots_updates_subscribe(
        &self,
        _meta: Self::Metadata,
        subscriber: Subscriber<Arc<SlotUpdate>>,
    ) {
        info!("slots_updates_subscribe");
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
        let id = self.uid.fetch_add(1, atomic::Ordering::Relaxed);
        let sub_id = SubscriptionId::Number(id as u64);
        info!("slots_updates_subscribe: id={:?}", sub_id);
        self.subscriptions
            .add_slots_updates_subscription(sub_id, subscriber);
    }

    fn slots_updates_unsubscribe(
        &self,
        _meta: Option<Self::Metadata>,
        id: SubscriptionId,
    ) -> Result<bool> {
        info!("slots_updates_unsubscribe");
        if self.subscriptions.remove_slots_updates_subscription(&id) {
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
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
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
        if let Err(err) = self.check_subscription_count() {
            subscriber.reject(err).unwrap_or_default();
            return;
        }
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
    use {
        super::*,
        crate::{
            optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
            rpc_subscriptions::tests::robust_poll_or_panic,
        },
        jsonrpc_core::{futures::channel::mpsc, Response},
        jsonrpc_pubsub::{PubSubHandler, Session},
        serial_test::serial,
        solana_account_decoder::{parse_account_data::parse_account_data, UiAccountEncoding},
        solana_client::rpc_response::{ProcessedSignatureResult, ReceivedSignatureResult},
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            commitment::{BlockCommitmentCache, CommitmentSlots},
            genesis_utils::{
                create_genesis_config, create_genesis_config_with_vote_accounts, GenesisConfigInfo,
                ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{
            account::ReadableAccount,
            commitment_config::CommitmentConfig,
            hash::Hash,
            message::Message,
            pubkey::Pubkey,
            signature::{Keypair, Signer},
            stake::{
                self, instruction as stake_instruction,
                state::{Authorized, Lockup, StakeAuthorize},
            },
            system_instruction, system_program, system_transaction,
            transaction::{self, Transaction},
        },
        solana_stake_program::stake_state,
        solana_vote_program::vote_state::Vote,
        std::{
            sync::{atomic::AtomicBool, RwLock},
            thread::sleep,
            time::Duration,
        },
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
        let commitment_slots = CommitmentSlots {
            slot: current_slot,
            ..CommitmentSlots::default()
        };
        subscriptions.notify_subscribers(commitment_slots);
        Ok(())
    }

    fn create_session() -> Arc<Session> {
        Arc::new(Session::new(mpsc::unbounded().0))
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
        let bank = Bank::new_for_tests(&genesis_config);
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
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
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
                commitment: Some(CommitmentConfig::finalized()),
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
                commitment: Some(CommitmentConfig::finalized()),
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

        // Test "received" for gossip subscription
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("signatureNotification");
        rpc.signature_subscribe(
            session,
            subscriber,
            tx.signatures[0].to_string(),
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                enable_received_notification: Some(true),
            }),
        );
        let received_slot = 2;
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
               "subscription": 2,
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
        let bank = Bank::new_for_tests(&genesis_config);
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
        let res = io.handle_request_sync(req, session.clone());

        let expected = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);

        // Test bad parameter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[1]}"#;
        let res = io.handle_request_sync(req, session);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid subscription id."},"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

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
        let stake_program_id = stake::program::id();
        let bank = Bank::new_for_tests(&genesis_config);
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
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
        };
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            stake_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::processed()),
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
        let account = bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .get_account(&stake_account.pubkey())
            .unwrap();
        let expected_data = account.data();
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
            None,
        );
        let message = Message::new(&[ix], Some(&stake_authority.pubkey()));
        let tx = Transaction::new(&[&stake_authority], message, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc.subscriptions, 1).unwrap();
        sleep(Duration::from_millis(200));

        let bank = bank_forks.read().unwrap()[1].clone();
        let account = bank.get_account(&stake_account.pubkey()).unwrap();
        assert_eq!(
            stake_state::authorized_from(&account).unwrap().staker,
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
        let bank = Bank::new_for_tests(&genesis_config);
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
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
        };
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            nonce_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::processed()),
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
        let account = bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .get_account(&nonce_account.pubkey())
            .unwrap();
        let expected_data = account.data();
        let expected_data = parse_account_data(
            &nonce_account.pubkey(),
            &system_program::id(),
            expected_data,
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
        let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::new_for_tests(
            &genesis_config,
        ))));

        let mut io = PubSubHandler::default();
        let rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks);

        io.extend_with(rpc.to_delegate());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{}"]}}"#,
            bob_pubkey.to_string()
        );
        let _res = io.handle_request_sync(&req, session.clone());

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[0]}"#;
        let res = io.handle_request_sync(req, session.clone());

        let expected = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(expected, result);

        // Test bad parameter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[1]}"#;
        let res = io.handle_request_sync(req, session);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid subscription id."},"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

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
        let bank = Bank::new_for_tests(&genesis_config);
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
                commitment: Some(CommitmentConfig::finalized()),
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
        let bank = Bank::new_for_tests(&genesis_config);
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
                commitment: Some(CommitmentConfig::finalized()),
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
        let commitment_slots = CommitmentSlots {
            slot: 1,
            ..CommitmentSlots::default()
        };
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
        let bank = Bank::new_for_tests(&genesis_config);
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
        let bank = Bank::new_for_tests(&genesis_config);
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
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));

        // Setup RPC
        let mut rpc = RpcSolPubSubImpl::default_with_bank_forks(bank_forks.clone());
        let session = create_session();
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("voteNotification");

        // Setup Subscriptions
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = Arc::new(RpcSubscriptions::new_with_vote_subscription(
            &exit,
            bank_forks,
            block_commitment_cache,
            optimistically_confirmed_bank,
            true,
        ));
        rpc.subscriptions = subscriptions.clone();
        rpc.vote_subscribe(session, subscriber);

        let vote = Vote {
            slots: vec![1, 2],
            hash: Hash::default(),
            timestamp: None,
        };
        subscriptions.notify_vote(&vote);

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
        let bank = Bank::new_for_tests(&genesis_config);
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
