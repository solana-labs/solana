//! A client for subscribing to messages from the RPC server.
//!
//! The [`PubsubClient`] implements [Solana WebSocket event
//! subscriptions][spec].
//!
//! [spec]: https://docs.solana.com/developing/clients/jsonrpc-api#subscription-websocket
//!
//! This is a blocking API. For a non-blocking API use the asynchronous client
//! in [`crate::nonblocking::pubsub_client`].
//!
//! `PubsubClient` contains static methods to subscribe to events, like
//! [`PubsubClient::account_subscribe`]. These methods each return their own
//! subscription type, like [`AccountSubscription`], that are typedefs of
//! tuples, the first element being a handle to the subscription, like
//! [`AccountSubscription`], the second a [`Receiver`] of [`RpcResponse`] of
//! whichever type is appropriate for the subscription. The subscription handle
//! is a typedef of [`PubsubClientSubscription`], and it must remain live for
//! the receiver to continue receiving messages.
//!
//! Because this is a blocking API, with blocking receivers, a reasonable
//! pattern for using this API is to move each event receiver to its own thread
//! to block on messages, while holding all subscription handles on a single
//! primary thread.
//!
//! While `PubsubClientSubscription` contains methods for shutting down,
//! [`PubsubClientSubscription::send_unsubscribe`], and
//! [`PubsubClientSubscription::shutdown`], because its internal receivers block
//! on events from the server, these subscriptions cannot actually be shutdown
//! reliably. For a non-blocking, cancelable API, use the asynchronous client
//! in [`crate::nonblocking::pubsub_client`].
//!
//! By default the [`block_subscribe`] and [`vote_subscribe`] events are
//! disabled on RPC nodes. They can be enabled by passing
//! `--rpc-pubsub-enable-block-subscription` and
//! `--rpc-pubsub-enable-vote-subscription` to `solana-validator`. When these
//! methods are disabled, the RPC server will return a "Method not found" error
//! message.
//!
//! [`block_subscribe`]: https://docs.rs/solana-rpc/latest/solana_rpc/rpc_pubsub/trait.RpcSolPubSub.html#tymethod.block_subscribe
//! [`vote_subscribe`]: https://docs.rs/solana-rpc/latest/solana_rpc/rpc_pubsub/trait.RpcSolPubSub.html#tymethod.vote_subscribe
//!
//! # Examples
//!
//! This example subscribes to account events and then loops forever receiving
//! them.
//!
//! ```
//! use anyhow::Result;
//! use solana_sdk::commitment_config::CommitmentConfig;
//! use solana_pubsub_client::pubsub_client::PubsubClient;
//! use solana_rpc_client_api::config::RpcAccountInfoConfig;
//! use solana_sdk::pubkey::Pubkey;
//! use std::thread;
//!
//! fn get_account_updates(account_pubkey: Pubkey) -> Result<()> {
//!     let url = "wss://api.devnet.solana.com/";
//!
//!     let (mut account_subscription_client, account_subscription_receiver) =
//!         PubsubClient::account_subscribe(
//!             url,
//!             &account_pubkey,
//!             Some(RpcAccountInfoConfig {
//!                 encoding: None,
//!                 data_slice: None,
//!                 commitment: Some(CommitmentConfig::confirmed()),
//!                 min_context_slot: None,
//!             }),
//!         )?;
//!
//!     loop {
//!         match account_subscription_receiver.recv() {
//!             Ok(response) => {
//!                 println!("account subscription response: {:?}", response);
//!             }
//!             Err(e) => {
//!                 println!("account subscription error: {:?}", e);
//!                 break;
//!             }
//!         }
//!     }
//!
//!     Ok(())
//! }
//! #
//! # get_account_updates(solana_sdk::pubkey::new_rand());
//! # Ok::<(), anyhow::Error>(())
//! ```

pub use crate::nonblocking::pubsub_client::PubsubClientError;
use {
    crossbeam_channel::{unbounded, Receiver, Sender},
    log::*,
    serde::de::DeserializeOwned,
    serde_json::{
        json,
        value::Value::{Number, Object},
        Map, Value,
    },
    solana_account_decoder::UiAccount,
    solana_rpc_client_api::{
        config::{
            RpcAccountInfoConfig, RpcBlockSubscribeConfig, RpcBlockSubscribeFilter,
            RpcProgramAccountsConfig, RpcSignatureSubscribeConfig, RpcTransactionLogsConfig,
            RpcTransactionLogsFilter,
        },
        filter,
        response::{
            Response as RpcResponse, RpcBlockUpdate, RpcKeyedAccount, RpcLogsResponse,
            RpcSignatureResult, RpcVote, SlotInfo, SlotUpdate,
        },
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature},
    std::{
        marker::PhantomData,
        net::TcpStream,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{sleep, JoinHandle},
        time::Duration,
    },
    tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket},
    url::Url,
};

/// A subscription.
///
/// The subscription is unsubscribed on drop, and note that unsubscription (and
/// thus drop) time is unbounded. See
/// [`PubsubClientSubscription::send_unsubscribe`].
pub struct PubsubClientSubscription<T>
where
    T: DeserializeOwned,
{
    message_type: PhantomData<T>,
    operation: &'static str,
    socket: Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
    subscription_id: u64,
    t_cleanup: Option<JoinHandle<()>>,
    exit: Arc<AtomicBool>,
}

impl<T> Drop for PubsubClientSubscription<T>
where
    T: DeserializeOwned,
{
    fn drop(&mut self) {
        self.send_unsubscribe()
            .unwrap_or_else(|_| warn!("unable to unsubscribe from websocket"));
        self.socket
            .write()
            .unwrap()
            .close(None)
            .unwrap_or_else(|_| warn!("unable to close websocket"));
    }
}

impl<T> PubsubClientSubscription<T>
where
    T: DeserializeOwned,
{
    fn send_subscribe(
        writable_socket: &Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
        body: String,
    ) -> Result<u64, PubsubClientError> {
        writable_socket.write().unwrap().send(Message::Text(body))?;
        let message = writable_socket.write().unwrap().read()?;
        Self::extract_subscription_id(message)
    }

    fn extract_subscription_id(message: Message) -> Result<u64, PubsubClientError> {
        let message_text = &message.into_text()?;

        if let Ok(json_msg) = serde_json::from_str::<Map<String, Value>>(message_text) {
            if let Some(Number(x)) = json_msg.get("result") {
                if let Some(x) = x.as_u64() {
                    return Ok(x);
                }
            }
        }

        Err(PubsubClientError::UnexpectedSubscriptionResponse(format!(
            "msg={message_text}"
        )))
    }

    /// Send an unsubscribe message to the server.
    ///
    /// Note that this will block as long as the internal subscription receiver
    /// is waiting on messages from the server, and this can take an unbounded
    /// amount of time if the server does not send any messages.
    ///
    /// If a pubsub client needs to shutdown reliably it should use
    /// the async client in [`crate::nonblocking::pubsub_client`].
    pub fn send_unsubscribe(&self) -> Result<(), PubsubClientError> {
        let method = format!("{}Unsubscribe", self.operation);
        self.socket
            .write()
            .unwrap()
            .send(Message::Text(
                json!({
                "jsonrpc":"2.0","id":1,"method":method,"params":[self.subscription_id]
                })
                .to_string(),
            ))
            .map_err(|err| err.into())
    }

    fn get_version(
        writable_socket: &Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
    ) -> Result<semver::Version, PubsubClientError> {
        writable_socket.write().unwrap().send(Message::Text(
            json!({
                "jsonrpc":"2.0","id":1,"method":"getVersion",
            })
            .to_string(),
        ))?;
        let message = writable_socket.write().unwrap().read()?;
        let message_text = &message.into_text()?;

        if let Ok(json_msg) = serde_json::from_str::<Map<String, Value>>(message_text) {
            if let Some(Object(version_map)) = json_msg.get("result") {
                if let Some(node_version) = version_map.get("solana-core") {
                    if let Some(node_version) = node_version.as_str() {
                        if let Ok(parsed) = semver::Version::parse(node_version) {
                            return Ok(parsed);
                        }
                    }
                }
            }
        }

        Err(PubsubClientError::UnexpectedGetVersionResponse(format!(
            "msg={message_text}"
        )))
    }

    fn read_message(
        writable_socket: &Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
    ) -> Result<Option<T>, PubsubClientError> {
        let message = writable_socket.write().unwrap().read()?;
        if message.is_ping() {
            return Ok(None);
        }
        let message_text = &message.into_text()?;
        if let Ok(json_msg) = serde_json::from_str::<Map<String, Value>>(message_text) {
            if let Some(Object(params)) = json_msg.get("params") {
                if let Some(result) = params.get("result") {
                    if let Ok(x) = serde_json::from_value::<T>(result.clone()) {
                        return Ok(Some(x));
                    }
                }
            }
        }

        Err(PubsubClientError::UnexpectedMessageError(format!(
            "msg={message_text}"
        )))
    }

    /// Shutdown the internel message receiver and wait for its thread to exit.
    ///
    /// Note that this will block as long as the subscription receiver is
    /// waiting on messages from the server, and this can take an unbounded
    /// amount of time if the server does not send any messages.
    ///
    /// If a pubsub client needs to shutdown reliably it should use
    /// the async client in [`crate::nonblocking::pubsub_client`].
    pub fn shutdown(&mut self) -> std::thread::Result<()> {
        if self.t_cleanup.is_some() {
            info!("websocket thread - shutting down");
            self.exit.store(true, Ordering::Relaxed);
            let x = self.t_cleanup.take().unwrap().join();
            info!("websocket thread - shut down.");
            x
        } else {
            warn!("websocket thread - already shut down.");
            Ok(())
        }
    }
}

pub type PubsubLogsClientSubscription = PubsubClientSubscription<RpcResponse<RpcLogsResponse>>;
pub type LogsSubscription = (
    PubsubLogsClientSubscription,
    Receiver<RpcResponse<RpcLogsResponse>>,
);

pub type PubsubSlotClientSubscription = PubsubClientSubscription<SlotInfo>;
pub type SlotsSubscription = (PubsubSlotClientSubscription, Receiver<SlotInfo>);

pub type PubsubSignatureClientSubscription =
    PubsubClientSubscription<RpcResponse<RpcSignatureResult>>;
pub type SignatureSubscription = (
    PubsubSignatureClientSubscription,
    Receiver<RpcResponse<RpcSignatureResult>>,
);

pub type PubsubBlockClientSubscription = PubsubClientSubscription<RpcResponse<RpcBlockUpdate>>;
pub type BlockSubscription = (
    PubsubBlockClientSubscription,
    Receiver<RpcResponse<RpcBlockUpdate>>,
);

pub type PubsubProgramClientSubscription = PubsubClientSubscription<RpcResponse<RpcKeyedAccount>>;
pub type ProgramSubscription = (
    PubsubProgramClientSubscription,
    Receiver<RpcResponse<RpcKeyedAccount>>,
);

pub type PubsubAccountClientSubscription = PubsubClientSubscription<RpcResponse<UiAccount>>;
pub type AccountSubscription = (
    PubsubAccountClientSubscription,
    Receiver<RpcResponse<UiAccount>>,
);

pub type PubsubVoteClientSubscription = PubsubClientSubscription<RpcVote>;
pub type VoteSubscription = (PubsubVoteClientSubscription, Receiver<RpcVote>);

pub type PubsubRootClientSubscription = PubsubClientSubscription<Slot>;
pub type RootSubscription = (PubsubRootClientSubscription, Receiver<Slot>);

/// A client for subscribing to messages from the RPC server.
///
/// See the [module documentation][self].
pub struct PubsubClient {}

fn connect_with_retry(
    url: Url,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, tungstenite::Error> {
    let mut connection_retries = 5;
    loop {
        let result = connect(url.clone()).map(|(socket, _)| socket);
        if let Err(tungstenite::Error::Http(response)) = &result {
            if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && connection_retries > 0
            {
                let mut duration = Duration::from_millis(500);
                if let Some(retry_after) = response.headers().get(reqwest::header::RETRY_AFTER) {
                    if let Ok(retry_after) = retry_after.to_str() {
                        if let Ok(retry_after) = retry_after.parse::<u64>() {
                            if retry_after < 120 {
                                duration = Duration::from_secs(retry_after);
                            }
                        }
                    }
                }

                connection_retries -= 1;
                debug!(
                    "Too many requests: server responded with {:?}, {} retries left, pausing for {:?}",
                    response, connection_retries, duration
                );

                sleep(duration);
                continue;
            }
        }
        return result;
    }
}

impl PubsubClient {
    /// Subscribe to account events.
    ///
    /// Receives messages of type [`UiAccount`] when an account's lamports or data changes.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`accountSubscribe`] RPC method.
    ///
    /// [`accountSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#accountsubscribe
    pub fn account_subscribe(
        url: &str,
        pubkey: &Pubkey,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<AccountSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"accountSubscribe",
            "params":[
                pubkey.to_string(),
                config
            ]
        })
        .to_string();
        let subscription_id = PubsubAccountClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "account",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to block events.
    ///
    /// Receives messages of type [`RpcBlockUpdate`] when a block is confirmed or finalized.
    ///
    /// This method is disabled by default. It can be enabled by passing
    /// `--rpc-pubsub-enable-block-subscription` to `solana-validator`.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`blockSubscribe`] RPC method.
    ///
    /// [`blockSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#blocksubscribe---unstable-disabled-by-default
    pub fn block_subscribe(
        url: &str,
        filter: RpcBlockSubscribeFilter,
        config: Option<RpcBlockSubscribeConfig>,
    ) -> Result<BlockSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"blockSubscribe",
            "params":[filter, config]
        })
        .to_string();

        let subscription_id = PubsubBlockClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "block",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to transaction log events.
    ///
    /// Receives messages of type [`RpcLogsResponse`] when a transaction is committed.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`logsSubscribe`] RPC method.
    ///
    /// [`logsSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#logssubscribe
    pub fn logs_subscribe(
        url: &str,
        filter: RpcTransactionLogsFilter,
        config: RpcTransactionLogsConfig,
    ) -> Result<LogsSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"logsSubscribe",
            "params":[filter, config]
        })
        .to_string();

        let subscription_id = PubsubLogsClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "logs",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to program account events.
    ///
    /// Receives messages of type [`RpcKeyedAccount`] when an account owned
    /// by the given program changes.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`programSubscribe`] RPC method.
    ///
    /// [`programSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#programsubscribe
    pub fn program_subscribe(
        url: &str,
        pubkey: &Pubkey,
        mut config: Option<RpcProgramAccountsConfig>,
    ) -> Result<ProgramSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();

        if let Some(ref mut config) = config {
            if let Some(ref mut filters) = config.filters {
                let node_version = PubsubProgramClientSubscription::get_version(&socket_clone).ok();
                // If node does not support the pubsub `getVersion` method, assume version is old
                // and filters should be mapped (node_version.is_none()).
                filter::maybe_map_filters(node_version, filters)
                    .map_err(PubsubClientError::RequestError)?;
            }
        }

        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"programSubscribe",
            "params":[
                pubkey.to_string(),
                config
            ]
        })
        .to_string();
        let subscription_id = PubsubProgramClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "program",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to vote events.
    ///
    /// Receives messages of type [`RpcVote`] when a new vote is observed. These
    /// votes are observed prior to confirmation and may never be confirmed.
    ///
    /// This method is disabled by default. It can be enabled by passing
    /// `--rpc-pubsub-enable-vote-subscription` to `solana-validator`.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`voteSubscribe`] RPC method.
    ///
    /// [`voteSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#votesubscribe---unstable-disabled-by-default
    pub fn vote_subscribe(url: &str) -> Result<VoteSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"voteSubscribe",
        })
        .to_string();
        let subscription_id = PubsubVoteClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "vote",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to root events.
    ///
    /// Receives messages of type [`Slot`] when a new [root] is set by the
    /// validator.
    ///
    /// [root]: https://docs.solana.com/terminology#root
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`rootSubscribe`] RPC method.
    ///
    /// [`rootSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#rootsubscribe
    pub fn root_subscribe(url: &str) -> Result<RootSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"rootSubscribe",
        })
        .to_string();
        let subscription_id = PubsubRootClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "root",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to transaction confirmation events.
    ///
    /// Receives messages of type [`RpcSignatureResult`] when a transaction
    /// with the given signature is committed.
    ///
    /// This is a subscription to a single notification. It is automatically
    /// cancelled by the server once the notification is sent.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`signatureSubscribe`] RPC method.
    ///
    /// [`signatureSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#signaturesubscribe
    pub fn signature_subscribe(
        url: &str,
        signature: &Signature,
        config: Option<RpcSignatureSubscribeConfig>,
    ) -> Result<SignatureSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"signatureSubscribe",
            "params":[
                signature.to_string(),
                config
            ]
        })
        .to_string();
        let subscription_id =
            PubsubSignatureClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "signature",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to slot events.
    ///
    /// Receives messages of type [`SlotInfo`] when a slot is processed.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`slotSubscribe`] RPC method.
    ///
    /// [`slotSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#slotsubscribe
    pub fn slot_subscribe(url: &str) -> Result<SlotsSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = unbounded::<SlotInfo>();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"slotSubscribe",
            "params":[]
        })
        .to_string();
        let subscription_id = PubsubSlotClientSubscription::send_subscribe(&socket_clone, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_sender(exit_clone, &socket_clone, sender)
        });

        let result = PubsubClientSubscription {
            message_type: PhantomData,
            operation: "slot",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        };

        Ok((result, receiver))
    }

    /// Subscribe to slot update events.
    ///
    /// Receives messages of type [`SlotUpdate`] when various updates to a slot occur.
    ///
    /// Note that this method operates differently than other subscriptions:
    /// instead of sending the message to a reciever on a channel, it accepts a
    /// `handler` callback that processes the message directly. This processing
    /// occurs on another thread.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`slotUpdatesSubscribe`] RPC method.
    ///
    /// [`slotUpdatesSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#slotsupdatessubscribe---unstable
    pub fn slot_updates_subscribe(
        url: &str,
        handler: impl Fn(SlotUpdate) + Send + 'static,
    ) -> Result<PubsubClientSubscription<SlotUpdate>, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let body = json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"slotsUpdatesSubscribe",
            "params":[]
        })
        .to_string();
        let subscription_id = PubsubSlotClientSubscription::send_subscribe(&socket, body)?;

        let t_cleanup = std::thread::spawn(move || {
            Self::cleanup_with_handler(exit_clone, &socket_clone, handler)
        });

        Ok(PubsubClientSubscription {
            message_type: PhantomData,
            operation: "slotsUpdates",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        })
    }

    fn cleanup_with_sender<T>(
        exit: Arc<AtomicBool>,
        socket: &Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
        sender: Sender<T>,
    ) where
        T: DeserializeOwned + Send + 'static,
    {
        let handler = move |message| match sender.send(message) {
            Ok(_) => (),
            Err(err) => {
                info!("receive error: {:?}", err);
            }
        };
        Self::cleanup_with_handler(exit, socket, handler);
    }

    fn cleanup_with_handler<T, F>(
        exit: Arc<AtomicBool>,
        socket: &Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
        handler: F,
    ) where
        T: DeserializeOwned,
        F: Fn(T) + Send + 'static,
    {
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            match PubsubClientSubscription::read_message(socket) {
                Ok(Some(message)) => handler(message),
                Ok(None) => {
                    // Nothing useful, means we received a ping message
                }
                Err(err) => {
                    info!("receive error: {:?}", err);
                    break;
                }
            }
        }

        info!("websocket - exited receive loop");
    }
}

#[cfg(test)]
mod tests {
    // see client-test/test/client.rs
}
