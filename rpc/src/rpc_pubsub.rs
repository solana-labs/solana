//! The `pubsub` module implements a threaded subscription service on client RPC request
#[cfg(test)]
use crate::{rpc_pubsub_service, rpc_subscriptions::RpcSubscriptions};
use {
    crate::{
        rpc::check_is_at_least_confirmed,
        rpc_pubsub_service::PubSubConfig,
        rpc_subscription_tracker::{
            AccountSubscriptionParams, BlockSubscriptionKind, BlockSubscriptionParams,
            LogsSubscriptionKind, LogsSubscriptionParams, ProgramSubscriptionParams,
            SignatureSubscriptionParams, SubscriptionControl, SubscriptionId, SubscriptionParams,
            SubscriptionToken,
        },
    },
    dashmap::DashMap,
    jsonrpc_core::{Error, ErrorCode, Result},
    jsonrpc_derive::rpc,
    jsonrpc_pubsub::{typed::Subscriber, SubscriptionId as PubSubSubscriptionId},
    solana_account_decoder::{UiAccount, UiAccountEncoding},
    solana_rpc_client_api::{
        config::{
            RpcAccountInfoConfig, RpcBlockSubscribeConfig, RpcBlockSubscribeFilter,
            RpcProgramAccountsConfig, RpcSignatureSubscribeConfig, RpcTransactionLogsConfig,
            RpcTransactionLogsFilter,
        },
        response::{
            Response as RpcResponse, RpcBlockUpdate, RpcKeyedAccount, RpcLogsResponse,
            RpcSignatureResult, RpcVersionInfo, RpcVote, SlotInfo, SlotUpdate,
        },
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature},
    solana_transaction_status::UiTransactionEncoding,
    std::{str::FromStr, sync::Arc},
};

// We have to keep both of the following traits to not break backwards compatibility.
// `RpcSolPubSubInternal` is actually used by the current PubSub API implementation.
// `RpcSolPubSub` and the corresponding `gen_client` module are preserved
// so the clients reliant on `gen_client::Client` do not break after this implementation is released.
//
// There are no compile-time checks that ensure coherence between traits
// so extra attention is required when adding a new method to the API.

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
    fn account_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
    ) -> Result<bool>;

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
    fn program_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
    ) -> Result<bool>;

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
    fn logs_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
    ) -> Result<bool>;

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
        id: PubSubSubscriptionId,
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
    fn slot_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
    ) -> Result<bool>;

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
        id: PubSubSubscriptionId,
    ) -> Result<bool>;

    // Subscribe to block data and content
    #[pubsub(subscription = "blockNotification", subscribe, name = "blockSubscribe")]
    fn block_subscribe(
        &self,
        meta: Self::Metadata,
        subscriber: Subscriber<Arc<RpcBlockUpdate>>,
        filter: RpcBlockSubscribeFilter,
        config: Option<RpcBlockSubscribeConfig>,
    );

    // Unsubscribe from block notification subscription.
    #[pubsub(
        subscription = "blockNotification",
        unsubscribe,
        name = "blockUnsubscribe"
    )]
    fn block_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
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
    fn vote_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
    ) -> Result<bool>;

    // Get notification when a new root is set
    #[pubsub(subscription = "rootNotification", subscribe, name = "rootSubscribe")]
    fn root_subscribe(&self, meta: Self::Metadata, subscriber: Subscriber<Slot>);

    // Unsubscribe from slot notification subscription.
    #[pubsub(
        subscription = "rootNotification",
        unsubscribe,
        name = "rootUnsubscribe"
    )]
    fn root_unsubscribe(
        &self,
        meta: Option<Self::Metadata>,
        id: PubSubSubscriptionId,
    ) -> Result<bool>;
}

pub use internal::RpcSolPubSubInternal;

// We have to use a separate module so the code generated by different `rpc` macro invocations do not interfere with each other.
mod internal {
    use super::*;

    #[rpc]
    pub trait RpcSolPubSubInternal {
        // Get notification every time account data is changed
        // Accepts pubkey parameter as base-58 encoded string
        #[rpc(name = "accountSubscribe")]
        fn account_subscribe(
            &self,
            pubkey_str: String,
            config: Option<RpcAccountInfoConfig>,
        ) -> Result<SubscriptionId>;

        // Unsubscribe from account notification subscription.
        #[rpc(name = "accountUnsubscribe")]
        fn account_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get notification every time account data owned by a particular program is changed
        // Accepts pubkey parameter as base-58 encoded string
        #[rpc(name = "programSubscribe")]
        fn program_subscribe(
            &self,
            pubkey_str: String,
            config: Option<RpcProgramAccountsConfig>,
        ) -> Result<SubscriptionId>;

        // Unsubscribe from account notification subscription.
        #[rpc(name = "programUnsubscribe")]
        fn program_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get logs for all transactions that reference the specified address
        #[rpc(name = "logsSubscribe")]
        fn logs_subscribe(
            &self,
            filter: RpcTransactionLogsFilter,
            config: Option<RpcTransactionLogsConfig>,
        ) -> Result<SubscriptionId>;

        // Unsubscribe from logs notification subscription.
        #[rpc(name = "logsUnsubscribe")]
        fn logs_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get notification when signature is verified
        // Accepts signature parameter as base-58 encoded string
        #[rpc(name = "signatureSubscribe")]
        fn signature_subscribe(
            &self,
            signature_str: String,
            config: Option<RpcSignatureSubscribeConfig>,
        ) -> Result<SubscriptionId>;

        // Unsubscribe from signature notification subscription.
        #[rpc(name = "signatureUnsubscribe")]
        fn signature_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get notification when slot is encountered
        #[rpc(name = "slotSubscribe")]
        fn slot_subscribe(&self) -> Result<SubscriptionId>;

        // Unsubscribe from slot notification subscription.
        #[rpc(name = "slotUnsubscribe")]
        fn slot_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get series of updates for all slots
        #[rpc(name = "slotsUpdatesSubscribe")]
        fn slots_updates_subscribe(&self) -> Result<SubscriptionId>;

        // Unsubscribe from slots updates notification subscription.
        #[rpc(name = "slotsUpdatesUnsubscribe")]
        fn slots_updates_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Subscribe to block data and content
        #[rpc(name = "blockSubscribe")]
        fn block_subscribe(
            &self,
            filter: RpcBlockSubscribeFilter,
            config: Option<RpcBlockSubscribeConfig>,
        ) -> Result<SubscriptionId>;

        // Unsubscribe from block notification subscription.
        #[rpc(name = "blockUnsubscribe")]
        fn block_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get notification when vote is encountered
        #[rpc(name = "voteSubscribe")]
        fn vote_subscribe(&self) -> Result<SubscriptionId>;

        // Unsubscribe from vote notification subscription.
        #[rpc(name = "voteUnsubscribe")]
        fn vote_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get notification when a new root is set
        #[rpc(name = "rootSubscribe")]
        fn root_subscribe(&self) -> Result<SubscriptionId>;

        // Unsubscribe from slot notification subscription.
        #[rpc(name = "rootUnsubscribe")]
        fn root_unsubscribe(&self, id: SubscriptionId) -> Result<bool>;

        // Get the current solana version running on the node
        #[rpc(name = "getVersion")]
        fn get_version(&self) -> Result<RpcVersionInfo>;
    }
}

pub struct RpcSolPubSubImpl {
    config: PubSubConfig,
    subscription_control: SubscriptionControl,
    current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>,
}

impl RpcSolPubSubImpl {
    pub fn new(
        config: PubSubConfig,
        subscription_control: SubscriptionControl,
        current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>,
    ) -> Self {
        Self {
            config,
            subscription_control,
            current_subscriptions,
        }
    }

    fn subscribe(&self, params: SubscriptionParams) -> Result<SubscriptionId> {
        let token = self
            .subscription_control
            .subscribe(params)
            .map_err(|_| Error {
                code: ErrorCode::InternalError,
                message: "Internal Error: Subscription refused. Node subscription limit reached"
                    .into(),
                data: None,
            })?;
        let id = token.id();
        self.current_subscriptions.insert(id, token);
        Ok(id)
    }

    fn unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        if self.current_subscriptions.remove(&id).is_some() {
            Ok(true)
        } else {
            Err(Error {
                code: ErrorCode::InvalidParams,
                message: "Invalid subscription id.".into(),
                data: None,
            })
        }
    }

    #[cfg(test)]
    pub fn block_until_processed(&self, rpc_subscriptions: &Arc<RpcSubscriptions>) {
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(rpc_subscriptions);
        rpc.slot_subscribe().unwrap();
        rpc_subscriptions.notify_slot(1, 0, 0);
        receiver.recv();
    }
}

fn param<T: FromStr>(param_str: &str, thing: &str) -> Result<T> {
    param_str.parse::<T>().map_err(|_e| Error {
        code: ErrorCode::InvalidParams,
        message: format!("Invalid Request: Invalid {thing} provided"),
        data: None,
    })
}

impl RpcSolPubSubInternal for RpcSolPubSubImpl {
    fn account_subscribe(
        &self,
        pubkey_str: String,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<SubscriptionId> {
        let RpcAccountInfoConfig {
            encoding,
            data_slice,
            commitment,
            min_context_slot: _, // ignored
        } = config.unwrap_or_default();
        let params = AccountSubscriptionParams {
            pubkey: param::<Pubkey>(&pubkey_str, "pubkey")?,
            commitment: commitment.unwrap_or_default(),
            data_slice,
            encoding: encoding.unwrap_or(UiAccountEncoding::Binary),
        };
        self.subscribe(SubscriptionParams::Account(params))
    }

    fn account_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn program_subscribe(
        &self,
        pubkey_str: String,
        config: Option<RpcProgramAccountsConfig>,
    ) -> Result<SubscriptionId> {
        let config = config.unwrap_or_default();
        let params = ProgramSubscriptionParams {
            pubkey: param::<Pubkey>(&pubkey_str, "pubkey")?,
            filters: config.filters.unwrap_or_default(),
            encoding: config
                .account_config
                .encoding
                .unwrap_or(UiAccountEncoding::Binary),
            data_slice: config.account_config.data_slice,
            commitment: config.account_config.commitment.unwrap_or_default(),
            with_context: config.with_context.unwrap_or_default(),
        };
        self.subscribe(SubscriptionParams::Program(params))
    }

    fn program_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn logs_subscribe(
        &self,
        filter: RpcTransactionLogsFilter,
        config: Option<RpcTransactionLogsConfig>,
    ) -> Result<SubscriptionId> {
        let params = LogsSubscriptionParams {
            kind: match filter {
                RpcTransactionLogsFilter::All => LogsSubscriptionKind::All,
                RpcTransactionLogsFilter::AllWithVotes => LogsSubscriptionKind::AllWithVotes,
                RpcTransactionLogsFilter::Mentions(keys) => {
                    if keys.len() != 1 {
                        return Err(Error {
                            code: ErrorCode::InvalidParams,
                            message: "Invalid Request: Only 1 address supported".into(),
                            data: None,
                        });
                    }
                    LogsSubscriptionKind::Single(param::<Pubkey>(&keys[0], "mentions")?)
                }
            },
            commitment: config.and_then(|c| c.commitment).unwrap_or_default(),
        };
        self.subscribe(SubscriptionParams::Logs(params))
    }

    fn logs_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn signature_subscribe(
        &self,
        signature_str: String,
        config: Option<RpcSignatureSubscribeConfig>,
    ) -> Result<SubscriptionId> {
        let config = config.unwrap_or_default();
        let params = SignatureSubscriptionParams {
            signature: param::<Signature>(&signature_str, "signature")?,
            commitment: config.commitment.unwrap_or_default(),
            enable_received_notification: config.enable_received_notification.unwrap_or_default(),
        };
        self.subscribe(SubscriptionParams::Signature(params))
    }

    fn signature_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn slot_subscribe(&self) -> Result<SubscriptionId> {
        self.subscribe(SubscriptionParams::Slot)
    }

    fn slot_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn slots_updates_subscribe(&self) -> Result<SubscriptionId> {
        self.subscribe(SubscriptionParams::SlotsUpdates)
    }

    fn slots_updates_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn block_subscribe(
        &self,
        filter: RpcBlockSubscribeFilter,
        config: Option<RpcBlockSubscribeConfig>,
    ) -> Result<SubscriptionId> {
        if !self.config.enable_block_subscription {
            return Err(Error::new(jsonrpc_core::ErrorCode::MethodNotFound));
        }
        let config = config.unwrap_or_default();
        let commitment = config.commitment.unwrap_or_default();
        check_is_at_least_confirmed(commitment)?;
        let params = BlockSubscriptionParams {
            commitment: config.commitment.unwrap_or_default(),
            encoding: config.encoding.unwrap_or(UiTransactionEncoding::Base64),
            kind: match filter {
                RpcBlockSubscribeFilter::All => BlockSubscriptionKind::All,
                RpcBlockSubscribeFilter::MentionsAccountOrProgram(key) => {
                    BlockSubscriptionKind::MentionsAccountOrProgram(param::<Pubkey>(
                        &key,
                        "mentions_account_or_program",
                    )?)
                }
            },
            transaction_details: config.transaction_details.unwrap_or_default(),
            show_rewards: config.show_rewards.unwrap_or_default(),
            max_supported_transaction_version: config.max_supported_transaction_version,
        };
        self.subscribe(SubscriptionParams::Block(params))
    }

    fn block_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        if !self.config.enable_block_subscription {
            return Err(Error::new(jsonrpc_core::ErrorCode::MethodNotFound));
        }
        self.unsubscribe(id)
    }

    fn vote_subscribe(&self) -> Result<SubscriptionId> {
        if !self.config.enable_vote_subscription {
            return Err(Error::new(jsonrpc_core::ErrorCode::MethodNotFound));
        }
        self.subscribe(SubscriptionParams::Vote)
    }

    fn vote_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        if !self.config.enable_vote_subscription {
            return Err(Error::new(jsonrpc_core::ErrorCode::MethodNotFound));
        }
        self.unsubscribe(id)
    }

    fn root_subscribe(&self) -> Result<SubscriptionId> {
        self.subscribe(SubscriptionParams::Root)
    }

    fn root_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn get_version(&self) -> Result<RpcVersionInfo> {
        let version = solana_version::Version::default();
        Ok(RpcVersionInfo {
            solana_core: version.to_string(),
            feature_set: Some(version.feature_set),
        })
    }
}

#[cfg(test)]
mod tests {
    use {
        super::{RpcSolPubSubInternal, *},
        crate::{
            optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank, rpc_pubsub_service,
            rpc_subscriptions::RpcSubscriptions,
        },
        base64::{prelude::BASE64_STANDARD, Engine},
        jsonrpc_core::{IoHandler, Response},
        serial_test::serial,
        solana_account_decoder::{parse_account_data::parse_account_data, UiAccountEncoding},
        solana_rpc_client_api::response::{
            ProcessedSignatureResult, ReceivedSignatureResult, RpcSignatureResult, SlotInfo,
        },
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            commitment::{BlockCommitmentCache, CommitmentSlots},
            genesis_utils::{
                activate_all_features, create_genesis_config,
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{
            account::ReadableAccount,
            clock::Slot,
            commitment_config::CommitmentConfig,
            hash::Hash,
            message::Message,
            pubkey::Pubkey,
            rent::Rent,
            signature::{Keypair, Signer},
            stake::{
                self, instruction as stake_instruction,
                state::{Authorized, Lockup, StakeAuthorize, StakeStateV2},
            },
            system_instruction, system_program, system_transaction,
            transaction::{self, Transaction},
        },
        solana_stake_program::stake_state,
        solana_vote::vote_transaction::VoteTransaction,
        solana_vote_program::vote_state::Vote,
        std::{
            sync::{
                atomic::{AtomicBool, AtomicU64},
                RwLock,
            },
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
            .read()
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
        let bank_forks = BankForks::new_rw_arc(bank);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            Arc::new(AtomicBool::new(false)),
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        ));

        // Test signature subscriptions
        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);

        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);

        rpc.signature_subscribe(
            tx.signatures[0].to_string(),
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::finalized()),
                ..RpcSignatureSubscribeConfig::default()
            }),
        )
        .unwrap();

        process_transaction_and_notify(&bank_forks, &tx, &rpc_subscriptions, 0).unwrap();

        // Test signature confirmation notification
        let response = receiver.recv();
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

        assert_eq!(
            expected,
            serde_json::from_str::<serde_json::Value>(&response).unwrap(),
        );

        // Test "received"
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);

        rpc.signature_subscribe(
            tx.signatures[0].to_string(),
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::finalized()),
                enable_received_notification: Some(true),
            }),
        )
        .unwrap();
        let received_slot = 1;
        rpc_subscriptions.notify_signatures_received((received_slot, vec![tx.signatures[0]]));

        // Test signature confirmation notification
        let response = receiver.recv();
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
        assert_eq!(
            expected,
            serde_json::from_str::<serde_json::Value>(&response).unwrap(),
        );

        // Test "received" for gossip subscription
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);

        rpc.signature_subscribe(
            tx.signatures[0].to_string(),
            Some(RpcSignatureSubscribeConfig {
                commitment: Some(CommitmentConfig::confirmed()),
                enable_received_notification: Some(true),
            }),
        )
        .unwrap();
        let received_slot = 2;
        rpc_subscriptions.notify_signatures_received((received_slot, vec![tx.signatures[0]]));

        // Test signature confirmation notification
        let response = receiver.recv();
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
        assert_eq!(
            expected,
            serde_json::from_str::<serde_json::Value>(&response).unwrap(),
        );
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
        let bank_forks = BankForks::new_rw_arc(bank);

        let mut io = IoHandler::<()>::default();
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
        ));
        let (rpc, _receiver) = rpc_pubsub_service::test_connection(&subscriptions);

        io.extend_with(rpc.to_delegate());

        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"signatureSubscribe","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let _res = io.handle_request_sync(&req);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[0]}"#;
        let res = io.handle_request_sync(req);

        let expected = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(result, expected);

        // Test bad parameter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"signatureUnsubscribe","params":[1]}"#;
        let res = io.handle_request_sync(req);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid subscription id."},"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(result, expected);
    }

    #[test]
    #[serial]
    fn test_account_subscribe() {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair: alice,
            ..
        } = create_genesis_config(10_000_000_000);
        genesis_config.rent = Rent::default();
        activate_all_features(&mut genesis_config);

        let new_stake_authority = solana_sdk::pubkey::new_rand();
        let stake_authority = Keypair::new();
        let from = Keypair::new();
        let stake_account = Keypair::new();
        let stake_program_id = stake::program::id();
        let bank = Bank::new_for_tests(&genesis_config);
        let blockhash = bank.last_blockhash();
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();
        let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            Arc::new(AtomicBool::new(false)),
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        ));

        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);

        let encoding = UiAccountEncoding::Base64;

        rpc.account_subscribe(
            stake_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::processed()),
                encoding: Some(encoding),
                data_slice: None,
                min_context_slot: None,
            }),
        )
        .unwrap();
        rpc.block_until_processed(&rpc_subscriptions);

        let balance = {
            let bank = bank_forks.read().unwrap().working_bank();
            let rent = &bank.rent_collector().rent;
            rent.minimum_balance(StakeStateV2::size_of())
        };

        let tx = system_transaction::transfer(&alice, &from.pubkey(), balance, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc_subscriptions, 1).unwrap();
        let authorized = Authorized::auto(&stake_authority.pubkey());
        let ixs = stake_instruction::create_account(
            &from.pubkey(),
            &stake_account.pubkey(),
            &authorized,
            &Lockup::default(),
            balance,
        );
        let message = Message::new(&ixs, Some(&from.pubkey()));
        let tx = Transaction::new(&[&from, &stake_account], message, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc_subscriptions, 1).unwrap();

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
                       "lamports": balance,
                       "data": [BASE64_STANDARD.encode(expected_data), encoding],
                       "executable": false,
                       "rentEpoch": u64::MAX,
                       "space": expected_data.len(),
                   },
               },
               "subscription": 0,
           }
        });

        let response = receiver.recv();
        assert_eq!(
            expected,
            serde_json::from_str::<serde_json::Value>(&response).unwrap(),
        );

        let balance = {
            let bank = bank_forks.read().unwrap().working_bank();
            let rent = &bank.rent_collector().rent;
            rent.minimum_balance(0)
        };
        let tx =
            system_transaction::transfer(&alice, &stake_authority.pubkey(), balance, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc_subscriptions, 1).unwrap();
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
        process_transaction_and_notify(&bank_forks, &tx, &rpc_subscriptions, 1).unwrap();
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
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();
        let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            Arc::new(AtomicBool::new(false)),
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests_with_slots(
                1, 1,
            ))),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        ));

        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);

        rpc.account_subscribe(
            nonce_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::processed()),
                encoding: Some(UiAccountEncoding::JsonParsed),
                data_slice: None,
                min_context_slot: None,
            }),
        )
        .unwrap();
        rpc.block_until_processed(&rpc_subscriptions);

        let ixs = system_instruction::create_nonce_account(
            &alice.pubkey(),
            &nonce_account.pubkey(),
            &alice.pubkey(),
            100,
        );
        let message = Message::new(&ixs, Some(&alice.pubkey()));
        let tx = Transaction::new(&[&alice, &nonce_account], message, blockhash);
        process_transaction_and_notify(&bank_forks, &tx, &rpc_subscriptions, 1).unwrap();

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
                       "rentEpoch": u64::MAX,
                       "space": account.data().len(),
                   },
               },
               "subscription": 0,
           }
        });

        let response = receiver.recv();
        assert_eq!(
            expected,
            serde_json::from_str::<serde_json::Value>(&response).unwrap(),
        );
    }

    #[test]
    #[serial]
    fn test_account_unsubscribe() {
        let bob_pubkey = solana_sdk::pubkey::new_rand();

        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank_forks = BankForks::new_rw_arc(Bank::new_for_tests(&genesis_config));

        let mut io = IoHandler::<()>::default();
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
        ));
        let (rpc, _receiver) = rpc_pubsub_service::test_connection(&subscriptions);

        io.extend_with(rpc.to_delegate());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"accountSubscribe","params":["{bob_pubkey}"]}}"#
        );
        let _res = io.handle_request_sync(&req);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[0]}"#;
        let res = io.handle_request_sync(req);

        let expected = r#"{"jsonrpc":"2.0","result":true,"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(result, expected);

        // Test bad parameter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"accountUnsubscribe","params":[1]}"#;
        let res = io.handle_request_sync(req);
        let expected = r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid subscription id."},"id":1}"#;
        let expected: Response = serde_json::from_str(expected).unwrap();

        let result: Response = serde_json::from_str(&res.unwrap()).unwrap();
        assert_eq!(result, expected);
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
        let bank_forks = BankForks::new_rw_arc(bank);
        let bob = Keypair::new();

        let exit = Arc::new(AtomicBool::new(false));
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            exit,
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        ));

        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);

        rpc.account_subscribe(
            bob.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::finalized()),
                encoding: None,
                data_slice: None,
                min_context_slot: None,
            }),
        )
        .unwrap();

        let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
        bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        rpc_subscriptions.notify_subscribers(CommitmentSlots::default());

        // allow 200ms for notification thread to wake
        std::thread::sleep(Duration::from_millis(200));
        let _panic = receiver.recv();
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
        let bank_forks = BankForks::new_rw_arc(bank);
        let bank0 = bank_forks.read().unwrap().get(0).unwrap();
        let bank1 = Bank::new_from_parent(bank0, &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        let bob = Keypair::new();

        let exit = Arc::new(AtomicBool::new(false));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests()));
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            exit,
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks.clone(),
            block_commitment_cache,
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks),
        ));
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&subscriptions);

        rpc.account_subscribe(
            bob.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::finalized()),
                encoding: None,
                data_slice: None,
                min_context_slot: None,
            }),
        )
        .unwrap();

        let tx = system_transaction::transfer(&alice, &bob.pubkey(), 100, blockhash);
        bank_forks
            .read()
            .unwrap()
            .get(1)
            .unwrap()
            .process_transaction(&tx)
            .unwrap();
        let commitment_slots = CommitmentSlots {
            slot: 1,
            ..CommitmentSlots::default()
        };
        subscriptions.notify_subscribers(commitment_slots);

        let commitment_slots = CommitmentSlots {
            slot: 2,
            root: 1,
            highest_confirmed_slot: 1,
            highest_super_majority_root: 1,
        };
        subscriptions.notify_subscribers(commitment_slots);
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
                       "rentEpoch": u64::MAX,
                       "space": 0,
                   },
               },
               "subscription": 0,
           }
        });
        let response = receiver.recv();
        assert_eq!(
            expected,
            serde_json::from_str::<serde_json::Value>(&response).unwrap(),
        );
    }

    #[test]
    #[serial]
    fn test_slot_subscribe() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
        ));
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);
        rpc.slot_subscribe().unwrap();

        rpc_subscriptions.notify_slot(0, 0, 0);

        // Test slot confirmation notification
        let response = receiver.recv();
        let expected_res = SlotInfo {
            parent: 0,
            slot: 0,
            root: 0,
        };
        let expected_res_str = serde_json::to_string(&expected_res).unwrap();

        let expected = format!(
            r#"{{"jsonrpc":"2.0","method":"slotNotification","params":{{"result":{expected_res_str},"subscription":0}}}}"#
        );
        assert_eq!(expected, response);
    }

    #[test]
    #[serial]
    fn test_slot_unsubscribe() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
        ));
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);
        let sub_id = rpc.slot_subscribe().unwrap();

        rpc_subscriptions.notify_slot(0, 0, 0);
        let response = receiver.recv();
        let expected_res = SlotInfo {
            parent: 0,
            slot: 0,
            root: 0,
        };
        let expected_res_str = serde_json::to_string(&expected_res).unwrap();

        let expected = format!(
            r#"{{"jsonrpc":"2.0","method":"slotNotification","params":{{"result":{expected_res_str},"subscription":0}}}}"#
        );
        assert_eq!(expected, response);

        assert!(rpc.slot_unsubscribe(42.into()).is_err());
        assert!(rpc.slot_unsubscribe(sub_id).is_ok());
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
        let bank_forks = BankForks::new_rw_arc(bank);

        // Setup Subscriptions
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            exit,
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
            block_commitment_cache,
            optimistically_confirmed_bank,
        ));
        // Setup RPC
        let (rpc, mut receiver) = rpc_pubsub_service::test_connection(&subscriptions);
        rpc.vote_subscribe().unwrap();

        let vote = Vote {
            slots: vec![1, 2],
            hash: Hash::default(),
            timestamp: None,
        };
        subscriptions.notify_vote(
            Pubkey::default(),
            VoteTransaction::from(vote),
            Signature::default(),
        );

        let response = receiver.recv();
        assert_eq!(
            response,
            r#"{"jsonrpc":"2.0","method":"voteNotification","params":{"result":{"votePubkey":"11111111111111111111111111111111","slots":[1,2],"hash":"11111111111111111111111111111111","timestamp":null,"signature":"1111111111111111111111111111111111111111111111111111111111111111"},"subscription":0}}"#
        );
    }

    #[test]
    #[serial]
    fn test_vote_unsubscribe() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
        ));
        let (rpc, _receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);
        let sub_id = rpc.vote_subscribe().unwrap();

        assert!(rpc.vote_unsubscribe(42.into()).is_err());
        assert!(rpc.vote_unsubscribe(sub_id).is_ok());
    }

    #[test]
    fn test_get_version() {
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = BankForks::new_rw_arc(bank);
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let max_complete_rewards_slot = Arc::new(AtomicU64::default());
        let rpc_subscriptions = Arc::new(RpcSubscriptions::default_with_bank_forks(
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            bank_forks,
        ));
        let (rpc, _receiver) = rpc_pubsub_service::test_connection(&rpc_subscriptions);
        let version = rpc.get_version().unwrap();
        let expected_version = solana_version::Version::default();
        assert_eq!(version.to_string(), expected_version.to_string());
        assert_eq!(version.feature_set.unwrap(), expected_version.feature_set);
    }
}
