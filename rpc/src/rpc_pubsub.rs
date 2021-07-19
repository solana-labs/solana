//! The `pubsub` module implements a threaded subscription service on client RPC request

#[cfg(test)]
use solana_runtime::bank_forks::BankForks;
#[cfg(test)]
use std::sync::RwLock;
use {
    crate::{
        rpc_pubsub_service::PubSubConfig,
        rpc_subscription_tracker::{
            AccountSubscriptionParams, LogsSubscriptionKind, LogsSubscriptionParams,
            ProgramSubscriptionParams, SignatureSubscriptionParams, SubscriptionId,
            SubscriptionParams, SubscriptionToken,
        },
        rpc_subscriptions::RpcSubscriptions,
    },
    dashmap::DashMap,
    jsonrpc_core::{Error, ErrorCode, Result},
    jsonrpc_derive::rpc,
    solana_account_decoder::UiAccountEncoding,
    solana_client::rpc_config::{
        RpcAccountInfoConfig, RpcProgramAccountsConfig, RpcSignatureSubscribeConfig,
        RpcTransactionLogsConfig, RpcTransactionLogsFilter,
    },
    solana_sdk::{pubkey::Pubkey, signature::Signature},
    std::{str::FromStr, sync::Arc},
};

#[rpc]
pub trait RpcSolPubSub {
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
}

pub struct RpcSolPubSubImpl {
    config: PubSubConfig,
    subscriptions: Arc<RpcSubscriptions>,
    current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>,
}

impl RpcSolPubSubImpl {
    pub fn new(
        config: PubSubConfig,
        subscriptions: Arc<RpcSubscriptions>,
        current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>,
    ) -> Self {
        Self {
            config,
            subscriptions,
            current_subscriptions,
        }
    }

    fn check_subscription_count(&self) -> Result<()> {
        let num_subscriptions = self.subscriptions.total();
        debug!("Total existing subscriptions: {}", num_subscriptions);
        if num_subscriptions >= self.config.max_active_subscriptions {
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

    fn subscribe(&self, params: SubscriptionParams) -> SubscriptionId {
        let token = self.subscriptions.subscribe(params);
        let id = token.id();
        self.current_subscriptions.insert(id, token);
        id
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
}

fn param<T: FromStr>(param_str: &str, thing: &str) -> Result<T> {
    param_str.parse::<T>().map_err(|_e| Error {
        code: ErrorCode::InvalidParams,
        message: format!("Invalid Request: Invalid {} provided", thing),
        data: None,
    })
}

impl RpcSolPubSub for RpcSolPubSubImpl {
    fn account_subscribe(
        &self,
        pubkey_str: String,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
        let config = config.unwrap_or_default();
        let params = AccountSubscriptionParams {
            pubkey: param::<Pubkey>(&pubkey_str, "pubkey")?,
            commitment: config.commitment.unwrap_or_default(),
            data_slice: config.data_slice,
            encoding: config.encoding.unwrap_or(UiAccountEncoding::Binary),
        };
        Ok(self.subscribe(SubscriptionParams::Account(params)))
    }

    fn account_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn program_subscribe(
        &self,
        pubkey_str: String,
        config: Option<RpcProgramAccountsConfig>,
    ) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
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
        Ok(self.subscribe(SubscriptionParams::Program(params)))
    }

    fn program_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn logs_subscribe(
        &self,
        filter: RpcTransactionLogsFilter,
        config: Option<RpcTransactionLogsConfig>,
    ) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
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
        Ok(self.subscribe(SubscriptionParams::Logs(params)))
    }

    fn logs_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn signature_subscribe(
        &self,
        signature_str: String,
        config: Option<RpcSignatureSubscribeConfig>,
    ) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
        let config = config.unwrap_or_default();
        let params = SignatureSubscriptionParams {
            signature: param::<Signature>(&signature_str, "signature")?,
            commitment: config.commitment.unwrap_or_default(),
            enable_received_notification: config.enable_received_notification.unwrap_or_default(),
        };
        Ok(self.subscribe(SubscriptionParams::Signature(params)))
    }

    fn signature_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn slot_subscribe(&self) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
        Ok(self.subscribe(SubscriptionParams::Slot))
    }

    fn slot_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn slots_updates_subscribe(&self) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
        Ok(self.subscribe(SubscriptionParams::SlotsUpdates))
    }

    fn slots_updates_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
    }

    fn vote_subscribe(&self) -> Result<SubscriptionId> {
        if !self.config.enable_vote_subscription {
            return Err(Error::new(jsonrpc_core::ErrorCode::MethodNotFound));
        }
        self.check_subscription_count()?;
        Ok(self.subscribe(SubscriptionParams::Vote))
    }

    fn vote_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        if !self.config.enable_vote_subscription {
            return Err(Error::new(jsonrpc_core::ErrorCode::MethodNotFound));
        }
        self.unsubscribe(id)
    }

    fn root_subscribe(&self) -> Result<SubscriptionId> {
        self.check_subscription_count()?;
        Ok(self.subscribe(SubscriptionParams::Root))
    }

    fn root_unsubscribe(&self, id: SubscriptionId) -> Result<bool> {
        self.unsubscribe(id)
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
        let encoding = UiAccountEncoding::Base64;
        let (subscriber, _id_receiver, receiver) = Subscriber::new_test("accountNotification");
        rpc.account_subscribe(
            session,
            subscriber,
            stake_account.pubkey().to_string(),
            Some(RpcAccountInfoConfig {
                commitment: Some(CommitmentConfig::processed()),
                encoding: Some(encoding),
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
                       "data": [base64::encode(expected_data), encoding],
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
