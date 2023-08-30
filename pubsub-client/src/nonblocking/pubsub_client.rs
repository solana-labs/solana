//! A client for subscribing to messages from the RPC server.
//!
//! The [`PubsubClient`] implements [Solana WebSocket event
//! subscriptions][spec].
//!
//! [spec]: https://docs.solana.com/developing/clients/jsonrpc-api#subscription-websocket
//!
//! This is a nonblocking (async) API. For a blocking API use the synchronous
//! client in [`crate::pubsub_client`].
//!
//! A single `PubsubClient` client may be used to subscribe to many events via
//! subscription methods like [`PubsubClient::account_subscribe`]. These methods
//! return a [`PubsubClientResult`] of a pair, the first element being a
//! [`BoxStream`] of subscription-specific [`RpcResponse`]s, the second being an
//! unsubscribe closure, an asynchronous function that can be called and
//! `await`ed to unsubscribe.
//!
//! Note that `BoxStream` contains an immutable reference to the `PubsubClient`
//! that created it. This makes `BoxStream` not `Send`, forcing it to stay in
//! the same task as its `PubsubClient`. `PubsubClient` though is `Send` and
//! `Sync`, and can be shared between tasks by putting it in an `Arc`. Thus
//! one viable pattern to creating multiple subscriptions is:
//!
//! - create an `Arc<PubsubClient>`
//! - spawn one task for each subscription, sharing the `PubsubClient`.
//! - in each task:
//!   - create a subscription
//!   - send the `UnsubscribeFn` to another task to handle shutdown
//!   - loop while receiving messages from the subscription
//!
//! This pattern is illustrated in the example below.
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
//! Demo two async `PubsubClient` subscriptions with clean shutdown.
//!
//! This spawns a task for each subscription type, each of which subscribes and
//! sends back a ready message and an unsubscribe channel (closure), then loops
//! on printing messages. The main task then waits for user input before
//! unsubscribing and waiting on the tasks.
//!
//! ```
//! use anyhow::Result;
//! use futures_util::StreamExt;
//! use solana_pubsub_client::nonblocking::pubsub_client::PubsubClient;
//! use std::sync::Arc;
//! use tokio::io::AsyncReadExt;
//! use tokio::sync::mpsc::unbounded_channel;
//!
//! pub async fn watch_subscriptions(
//!     websocket_url: &str,
//! ) -> Result<()> {
//!
//!     // Subscription tasks will send a ready signal when they have subscribed.
//!     let (ready_sender, mut ready_receiver) = unbounded_channel::<()>();
//!
//!     // Channel to receive unsubscribe channels (actually closures).
//!     // These receive a pair of `(Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>), &'static str)`,
//!     // where the first is a closure to call to unsubscribe, the second is the subscription name.
//!     let (unsubscribe_sender, mut unsubscribe_receiver) = unbounded_channel::<(_, &'static str)>();
//!
//!     // The `PubsubClient` must be `Arc`ed to share it across tasks.
//!     let pubsub_client = Arc::new(PubsubClient::new(websocket_url).await?);
//!
//!     let mut join_handles = vec![];
//!
//!     join_handles.push(("slot", tokio::spawn({
//!         // Clone things we need before moving their clones into the `async move` block.
//!         //
//!         // The subscriptions have to be made from the tasks that will receive the subscription messages,
//!         // because the subscription streams hold a reference to the `PubsubClient`.
//!         // Otherwise we would just subscribe on the main task and send the receivers out to other tasks.
//!
//!         let ready_sender = ready_sender.clone();
//!         let unsubscribe_sender = unsubscribe_sender.clone();
//!         let pubsub_client = Arc::clone(&pubsub_client);
//!         async move {
//!             let (mut slot_notifications, slot_unsubscribe) =
//!                 pubsub_client.slot_subscribe().await?;
//!
//!             // With the subscription started,
//!             // send a signal back to the main task for synchronization.
//!             ready_sender.send(()).expect("channel");
//!
//!             // Send the unsubscribe closure back to the main task.
//!             unsubscribe_sender.send((slot_unsubscribe, "slot"))
//!                 .map_err(|e| format!("{}", e)).expect("channel");
//!
//!             // Drop senders so that the channels can close.
//!             // The main task will receive until channels are closed.
//!             drop((ready_sender, unsubscribe_sender));
//!
//!             // Do something with the subscribed messages.
//!             // This loop will end once the main task unsubscribes.
//!             while let Some(slot_info) = slot_notifications.next().await {
//!                 println!("------------------------------------------------------------");
//!                 println!("slot pubsub result: {:?}", slot_info);
//!             }
//!
//!             // This type hint is necessary to allow the `async move` block to use `?`.
//!             Ok::<_, anyhow::Error>(())
//!         }
//!     })));
//!
//!     join_handles.push(("root", tokio::spawn({
//!         let ready_sender = ready_sender.clone();
//!         let unsubscribe_sender = unsubscribe_sender.clone();
//!         let pubsub_client = Arc::clone(&pubsub_client);
//!         async move {
//!             let (mut root_notifications, root_unsubscribe) =
//!                 pubsub_client.root_subscribe().await?;
//!
//!             ready_sender.send(()).expect("channel");
//!             unsubscribe_sender.send((root_unsubscribe, "root"))
//!                 .map_err(|e| format!("{}", e)).expect("channel");
//!             drop((ready_sender, unsubscribe_sender));
//!
//!             while let Some(root) = root_notifications.next().await {
//!                 println!("------------------------------------------------------------");
//!                 println!("root pubsub result: {:?}", root);
//!             }
//!
//!             Ok::<_, anyhow::Error>(())
//!         }
//!     })));
//!
//!     // Drop these senders so that the channels can close
//!     // and their receivers return `None` below.
//!     drop(ready_sender);
//!     drop(unsubscribe_sender);
//!
//!     // Wait until all subscribers are ready before proceeding with application logic.
//!     while let Some(_) = ready_receiver.recv().await { }
//!
//!     // Do application logic here.
//!
//!     // Wait for input or some application-specific shutdown condition.
//!     tokio::io::stdin().read_u8().await?;
//!
//!     // Unsubscribe from everything, which will shutdown all the tasks.
//!     while let Some((unsubscribe, name)) = unsubscribe_receiver.recv().await {
//!         println!("unsubscribing from {}", name);
//!         unsubscribe().await
//!     }
//!
//!     // Wait for the tasks.
//!     for (name, handle) in join_handles {
//!         println!("waiting on task {}", name);
//!         if let Ok(Err(e)) = handle.await {
//!             println!("task {} failed: {}", name, e);
//!         }
//!     }
//!
//!     Ok(())
//! }
//! # Ok::<(), anyhow::Error>(())
//! ```

use {
    futures_util::{
        future::{ready, BoxFuture, FutureExt},
        sink::SinkExt,
        stream::{BoxStream, StreamExt},
    },
    log::*,
    serde::de::DeserializeOwned,
    serde_json::{json, Map, Value},
    solana_account_decoder::UiAccount,
    solana_rpc_client_api::{
        config::{
            RpcAccountInfoConfig, RpcBlockSubscribeConfig, RpcBlockSubscribeFilter,
            RpcProgramAccountsConfig, RpcSignatureSubscribeConfig, RpcTransactionLogsConfig,
            RpcTransactionLogsFilter,
        },
        error_object::RpcErrorObject,
        filter::maybe_map_filters,
        response::{
            Response as RpcResponse, RpcBlockUpdate, RpcKeyedAccount, RpcLogsResponse,
            RpcSignatureResult, RpcVersionInfo, RpcVote, SlotInfo, SlotUpdate,
        },
    },
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature},
    std::collections::BTreeMap,
    thiserror::Error,
    tokio::{
        net::TcpStream,
        sync::{mpsc, oneshot, RwLock},
        task::JoinHandle,
        time::{sleep, Duration},
    },
    tokio_stream::wrappers::UnboundedReceiverStream,
    tokio_tungstenite::{
        connect_async,
        tungstenite::{
            protocol::frame::{coding::CloseCode, CloseFrame},
            Message,
        },
        MaybeTlsStream, WebSocketStream,
    },
    url::Url,
};

pub type PubsubClientResult<T = ()> = Result<T, PubsubClientError>;

#[derive(Debug, Error)]
pub enum PubsubClientError {
    #[error("url parse error")]
    UrlParseError(#[from] url::ParseError),

    #[error("unable to connect to server")]
    ConnectionError(tokio_tungstenite::tungstenite::Error),

    #[error("websocket error")]
    WsError(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("connection closed (({0})")]
    ConnectionClosed(String),

    #[error("json parse error")]
    JsonParseError(#[from] serde_json::error::Error),

    #[error("subscribe failed: {reason}")]
    SubscribeFailed { reason: String, message: String },

    #[error("unexpected message format: {0}")]
    UnexpectedMessageError(String),

    #[error("request failed: {reason}")]
    RequestFailed { reason: String, message: String },

    #[error("request error: {0}")]
    RequestError(String),

    #[error("could not find subscription id: {0}")]
    UnexpectedSubscriptionResponse(String),

    #[error("could not find node version: {0}")]
    UnexpectedGetVersionResponse(String),
}

type UnsubscribeFn = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;
type SubscribeResponseMsg =
    Result<(mpsc::UnboundedReceiver<Value>, UnsubscribeFn), PubsubClientError>;
type SubscribeRequestMsg = (String, Value, oneshot::Sender<SubscribeResponseMsg>);
type SubscribeResult<'a, T> = PubsubClientResult<(BoxStream<'a, T>, UnsubscribeFn)>;
type RequestMsg = (
    String,
    Value,
    oneshot::Sender<Result<Value, PubsubClientError>>,
);

/// A client for subscribing to messages from the RPC server.
///
/// See the [module documentation][self].
#[derive(Debug)]
pub struct PubsubClient {
    subscribe_sender: mpsc::UnboundedSender<SubscribeRequestMsg>,
    request_sender: mpsc::UnboundedSender<RequestMsg>,
    shutdown_sender: oneshot::Sender<()>,
    node_version: RwLock<Option<semver::Version>>,
    ws: JoinHandle<PubsubClientResult>,
}

impl PubsubClient {
    pub async fn new(url: &str) -> PubsubClientResult<Self> {
        let url = Url::parse(url)?;
        let (ws, _response) = connect_async(url)
            .await
            .map_err(PubsubClientError::ConnectionError)?;

        let (subscribe_sender, subscribe_receiver) = mpsc::unbounded_channel();
        let (request_sender, request_receiver) = mpsc::unbounded_channel();
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();

        Ok(Self {
            subscribe_sender,
            request_sender,
            shutdown_sender,
            node_version: RwLock::new(None),
            ws: tokio::spawn(PubsubClient::run_ws(
                ws,
                subscribe_receiver,
                request_receiver,
                shutdown_receiver,
            )),
        })
    }

    pub async fn shutdown(self) -> PubsubClientResult {
        let _ = self.shutdown_sender.send(());
        self.ws.await.unwrap() // WS future should not be cancelled or panicked
    }

    pub async fn set_node_version(&self, version: semver::Version) -> Result<(), ()> {
        let mut w_node_version = self.node_version.write().await;
        *w_node_version = Some(version);
        Ok(())
    }

    async fn get_node_version(&self) -> PubsubClientResult<semver::Version> {
        let r_node_version = self.node_version.read().await;
        if let Some(version) = &*r_node_version {
            Ok(version.clone())
        } else {
            drop(r_node_version);
            let mut w_node_version = self.node_version.write().await;
            let node_version = self.get_version().await?;
            *w_node_version = Some(node_version.clone());
            Ok(node_version)
        }
    }

    async fn get_version(&self) -> PubsubClientResult<semver::Version> {
        let (response_sender, response_receiver) = oneshot::channel();
        self.request_sender
            .send(("getVersion".to_string(), Value::Null, response_sender))
            .map_err(|err| PubsubClientError::ConnectionClosed(err.to_string()))?;
        let result = response_receiver
            .await
            .map_err(|err| PubsubClientError::ConnectionClosed(err.to_string()))??;
        let node_version: RpcVersionInfo = serde_json::from_value(result)?;
        let node_version = semver::Version::parse(&node_version.solana_core).map_err(|e| {
            PubsubClientError::RequestFailed {
                reason: format!("failed to parse cluster version: {e}"),
                message: "getVersion".to_string(),
            }
        })?;
        Ok(node_version)
    }

    async fn subscribe<'a, T>(&self, operation: &str, params: Value) -> SubscribeResult<'a, T>
    where
        T: DeserializeOwned + Send + 'a,
    {
        let (response_sender, response_receiver) = oneshot::channel();
        self.subscribe_sender
            .send((operation.to_string(), params, response_sender))
            .map_err(|err| PubsubClientError::ConnectionClosed(err.to_string()))?;

        let (notifications, unsubscribe) = response_receiver
            .await
            .map_err(|err| PubsubClientError::ConnectionClosed(err.to_string()))??;
        Ok((
            UnboundedReceiverStream::new(notifications)
                .filter_map(|value| ready(serde_json::from_value::<T>(value).ok()))
                .boxed(),
            unsubscribe,
        ))
    }

    /// Subscribe to account events.
    ///
    /// Receives messages of type [`UiAccount`] when an account's lamports or data changes.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`accountSubscribe`] RPC method.
    ///
    /// [`accountSubscribe`]: https://docs.solana.com/developing/clients/jsonrpc-api#accountsubscribe
    pub async fn account_subscribe(
        &self,
        pubkey: &Pubkey,
        config: Option<RpcAccountInfoConfig>,
    ) -> SubscribeResult<'_, RpcResponse<UiAccount>> {
        let params = json!([pubkey.to_string(), config]);
        self.subscribe("account", params).await
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
    pub async fn block_subscribe(
        &self,
        filter: RpcBlockSubscribeFilter,
        config: Option<RpcBlockSubscribeConfig>,
    ) -> SubscribeResult<'_, RpcResponse<RpcBlockUpdate>> {
        self.subscribe("block", json!([filter, config])).await
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
    pub async fn logs_subscribe(
        &self,
        filter: RpcTransactionLogsFilter,
        config: RpcTransactionLogsConfig,
    ) -> SubscribeResult<'_, RpcResponse<RpcLogsResponse>> {
        self.subscribe("logs", json!([filter, config])).await
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
    pub async fn program_subscribe(
        &self,
        pubkey: &Pubkey,
        mut config: Option<RpcProgramAccountsConfig>,
    ) -> SubscribeResult<'_, RpcResponse<RpcKeyedAccount>> {
        if let Some(ref mut config) = config {
            if let Some(ref mut filters) = config.filters {
                let node_version = self.get_node_version().await.ok();
                // If node does not support the pubsub `getVersion` method, assume version is old
                // and filters should be mapped (node_version.is_none()).
                maybe_map_filters(node_version, filters).map_err(|e| {
                    PubsubClientError::RequestFailed {
                        reason: e,
                        message: "maybe_map_filters".to_string(),
                    }
                })?;
            }
        }

        let params = json!([pubkey.to_string(), config]);
        self.subscribe("program", params).await
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
    pub async fn vote_subscribe(&self) -> SubscribeResult<'_, RpcVote> {
        self.subscribe("vote", json!([])).await
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
    pub async fn root_subscribe(&self) -> SubscribeResult<'_, Slot> {
        self.subscribe("root", json!([])).await
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
    pub async fn signature_subscribe(
        &self,
        signature: &Signature,
        config: Option<RpcSignatureSubscribeConfig>,
    ) -> SubscribeResult<'_, RpcResponse<RpcSignatureResult>> {
        let params = json!([signature.to_string(), config]);
        self.subscribe("signature", params).await
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
    pub async fn slot_subscribe(&self) -> SubscribeResult<'_, SlotInfo> {
        self.subscribe("slot", json!([])).await
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
    pub async fn slot_updates_subscribe(&self) -> SubscribeResult<'_, SlotUpdate> {
        self.subscribe("slotsUpdates", json!([])).await
    }

    async fn run_ws(
        mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
        mut subscribe_receiver: mpsc::UnboundedReceiver<SubscribeRequestMsg>,
        mut request_receiver: mpsc::UnboundedReceiver<RequestMsg>,
        mut shutdown_receiver: oneshot::Receiver<()>,
    ) -> PubsubClientResult {
        let mut request_id: u64 = 0;

        let mut requests_subscribe = BTreeMap::new();
        let mut requests_unsubscribe = BTreeMap::<u64, oneshot::Sender<()>>::new();
        let mut other_requests = BTreeMap::new();
        let mut subscriptions = BTreeMap::new();
        let (unsubscribe_sender, mut unsubscribe_receiver) = mpsc::unbounded_channel();

        loop {
            tokio::select! {
                // Send close on shutdown signal
                _ = (&mut shutdown_receiver) => {
                    let frame = CloseFrame { code: CloseCode::Normal, reason: "".into() };
                    ws.send(Message::Close(Some(frame))).await?;
                    ws.flush().await?;
                    break;
                },
                // Send `Message::Ping` each 10s if no any other communication
                () = sleep(Duration::from_secs(10)) => {
                    ws.send(Message::Ping(Vec::new())).await?;
                },
                // Read message for subscribe
                Some((operation, params, response_sender)) = subscribe_receiver.recv() => {
                    request_id += 1;
                    let method = format!("{operation}Subscribe");
                    let text = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":params}).to_string();
                    ws.send(Message::Text(text)).await?;
                    requests_subscribe.insert(request_id, (operation, response_sender));
                },
                // Read message for unsubscribe
                Some((operation, sid, response_sender)) = unsubscribe_receiver.recv() => {
                    subscriptions.remove(&sid);
                    request_id += 1;
                    let method = format!("{operation}Unsubscribe");
                    let text = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":[sid]}).to_string();
                    ws.send(Message::Text(text)).await?;
                    requests_unsubscribe.insert(request_id, response_sender);
                },
                // Read message for other requests
                Some((method, params, response_sender)) = request_receiver.recv() => {
                    request_id += 1;
                    let text = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":params}).to_string();
                    ws.send(Message::Text(text)).await?;
                    other_requests.insert(request_id, response_sender);
                }
                // Read incoming WebSocket message
                next_msg = ws.next() => {
                    let msg = match next_msg {
                        Some(msg) => msg?,
                        None => break,
                    };
                    trace!("ws.next(): {:?}", &msg);

                    // Get text from the message
                    let text = match msg {
                        Message::Text(text) => text,
                        Message::Binary(_data) => continue, // Ignore
                        Message::Ping(data) => {
                            ws.send(Message::Pong(data)).await?;
                            continue
                        },
                        Message::Pong(_data) => continue,
                        Message::Close(_frame) => break,
                        Message::Frame(_frame) => continue,
                    };


                    let mut json: Map<String, Value> = serde_json::from_str(&text)?;

                    // Subscribe/Unsubscribe response, example:
                    // `{"jsonrpc":"2.0","result":5308752,"id":1}`
                    if let Some(id) = json.get("id") {
                        let id = id.as_u64().ok_or_else(|| {
                            PubsubClientError::SubscribeFailed { reason: "invalid `id` field".into(), message: text.clone() }
                        })?;

                        let err = json.get("error").map(|error_object| {
                            match serde_json::from_value::<RpcErrorObject>(error_object.clone()) {
                                Ok(rpc_error_object) => {
                                    format!("{} ({})",  rpc_error_object.message, rpc_error_object.code)
                                }
                                Err(err) => format!(
                                    "Failed to deserialize RPC error response: {} [{}]",
                                    serde_json::to_string(error_object).unwrap(),
                                    err
                                )
                            }
                        });

                        if let Some(response_sender) = other_requests.remove(&id) {
                            match err {
                                Some(reason) => {
                                    let _ = response_sender.send(Err(PubsubClientError::RequestFailed { reason, message: text.clone()}));
                                },
                                None => {
                                    let json_result = json.get("result").ok_or_else(|| {
                                        PubsubClientError::RequestFailed { reason: "missing `result` field".into(), message: text.clone() }
                                    })?;
                                    if response_sender.send(Ok(json_result.clone())).is_err() {
                                        break;
                                    }
                                }
                            }
                        } else if let Some(response_sender) = requests_unsubscribe.remove(&id) {
                            let _ = response_sender.send(()); // do not care if receiver is closed
                        } else if let Some((operation, response_sender)) = requests_subscribe.remove(&id) {
                            match err {
                                Some(reason) => {
                                    let _ = response_sender.send(Err(PubsubClientError::SubscribeFailed { reason, message: text.clone()}));
                                },
                                None => {
                                    // Subscribe Id
                                    let sid = json.get("result").and_then(Value::as_u64).ok_or_else(|| {
                                        PubsubClientError::SubscribeFailed { reason: "invalid `result` field".into(), message: text.clone() }
                                    })?;

                                    // Create notifications channel and unsubscribe function
                                    let (notifications_sender, notifications_receiver) = mpsc::unbounded_channel();
                                    let unsubscribe_sender = unsubscribe_sender.clone();
                                    let unsubscribe = Box::new(move || async move {
                                        let (response_sender, response_receiver) = oneshot::channel();
                                        // do nothing if ws already closed
                                        if unsubscribe_sender.send((operation, sid, response_sender)).is_ok() {
                                            let _ = response_receiver.await; // channel can be closed only if ws is closed
                                        }
                                    }.boxed());

                                    if response_sender.send(Ok((notifications_receiver, unsubscribe))).is_err() {
                                        break;
                                    }
                                    subscriptions.insert(sid, notifications_sender);
                                }
                            }
                        } else {
                            error!("Unknown request id: {}", id);
                            break;
                        }
                        continue;
                    }

                    // Notification, example:
                    // `{"jsonrpc":"2.0","method":"logsNotification","params":{"result":{...},"subscription":3114862}}`
                    if let Some(Value::Object(params)) = json.get_mut("params") {
                        if let Some(sid) = params.get("subscription").and_then(Value::as_u64) {
                            let mut unsubscribe_required = false;

                            if let Some(notifications_sender) = subscriptions.get(&sid) {
                                if let Some(result) = params.remove("result") {
                                    if notifications_sender.send(result).is_err() {
                                        unsubscribe_required = true;
                                    }
                                }
                            } else {
                                unsubscribe_required = true;
                            }

                            if unsubscribe_required {
                                if let Some(Value::String(method)) = json.remove("method") {
                                    if let Some(operation) = method.strip_suffix("Notification") {
                                        let (response_sender, _response_receiver) = oneshot::channel();
                                        let _ = unsubscribe_sender.send((operation.to_string(), sid, response_sender));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // see client-test/test/client.rs
}
