use {
    crate::{
        http_sender::RpcErrorObject,
        rpc_config::{
            RpcAccountInfoConfig, RpcBlockSubscribeConfig, RpcBlockSubscribeFilter,
            RpcProgramAccountsConfig, RpcSignatureSubscribeConfig, RpcTransactionLogsConfig,
            RpcTransactionLogsFilter,
        },
        rpc_response::{
            Response as RpcResponse, RpcBlockUpdate, RpcKeyedAccount, RpcLogsResponse,
            RpcSignatureResult, RpcVote, SlotInfo, SlotUpdate,
        },
    },
    futures_util::{
        future::{ready, BoxFuture, FutureExt},
        sink::SinkExt,
        stream::{BoxStream, StreamExt},
    },
    log::*,
    serde::de::DeserializeOwned,
    serde_json::{json, Map, Value},
    solana_account_decoder::UiAccount,
    solana_sdk::{clock::Slot, pubkey::Pubkey, signature::Signature},
    std::collections::BTreeMap,
    thiserror::Error,
    tokio::{
        net::TcpStream,
        sync::{mpsc, oneshot},
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
}

type UnsubscribeFn = Box<dyn FnOnce() -> BoxFuture<'static, ()> + Send>;
type SubscribeResponseMsg =
    Result<(mpsc::UnboundedReceiver<Value>, UnsubscribeFn), PubsubClientError>;
type SubscribeRequestMsg = (String, Value, oneshot::Sender<SubscribeResponseMsg>);
type SubscribeResult<'a, T> = PubsubClientResult<(BoxStream<'a, T>, UnsubscribeFn)>;

#[derive(Debug)]
pub struct PubsubClient {
    subscribe_tx: mpsc::UnboundedSender<SubscribeRequestMsg>,
    shutdown_tx: oneshot::Sender<()>,
    ws: JoinHandle<PubsubClientResult>,
}

impl PubsubClient {
    pub async fn new(url: &str) -> PubsubClientResult<Self> {
        let url = Url::parse(url)?;
        let (ws, _response) = connect_async(url)
            .await
            .map_err(PubsubClientError::ConnectionError)?;

        let (subscribe_tx, subscribe_rx) = mpsc::unbounded_channel();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        Ok(Self {
            subscribe_tx,
            shutdown_tx,
            ws: tokio::spawn(PubsubClient::run_ws(ws, subscribe_rx, shutdown_rx)),
        })
    }

    pub async fn shutdown(self) -> PubsubClientResult {
        let _ = self.shutdown_tx.send(());
        self.ws.await.unwrap() // WS future should not be cancelled or panicked
    }

    async fn subscribe<'a, T>(&self, operation: &str, params: Value) -> SubscribeResult<'a, T>
    where
        T: DeserializeOwned + Send + 'a,
    {
        let (response_tx, response_rx) = oneshot::channel();
        self.subscribe_tx
            .send((operation.to_string(), params, response_tx))
            .map_err(|err| PubsubClientError::ConnectionClosed(err.to_string()))?;

        let (notifications, unsubscribe) = response_rx
            .await
            .map_err(|err| PubsubClientError::ConnectionClosed(err.to_string()))??;
        Ok((
            UnboundedReceiverStream::new(notifications)
                .filter_map(|value| ready(serde_json::from_value::<T>(value).ok()))
                .boxed(),
            unsubscribe,
        ))
    }

    pub async fn account_subscribe(
        &self,
        pubkey: &Pubkey,
        config: Option<RpcAccountInfoConfig>,
    ) -> SubscribeResult<'_, RpcResponse<UiAccount>> {
        let params = json!([pubkey.to_string(), config]);
        self.subscribe("account", params).await
    }

    pub async fn block_subscribe(
        &self,
        filter: RpcBlockSubscribeFilter,
        config: Option<RpcBlockSubscribeConfig>,
    ) -> SubscribeResult<'_, RpcResponse<RpcBlockUpdate>> {
        self.subscribe("block", json!([filter, config])).await
    }

    pub async fn logs_subscribe(
        &self,
        filter: RpcTransactionLogsFilter,
        config: RpcTransactionLogsConfig,
    ) -> SubscribeResult<'_, RpcResponse<RpcLogsResponse>> {
        self.subscribe("logs", json!([filter, config])).await
    }

    pub async fn program_subscribe(
        &self,
        pubkey: &Pubkey,
        config: Option<RpcProgramAccountsConfig>,
    ) -> SubscribeResult<'_, RpcResponse<RpcKeyedAccount>> {
        let params = json!([pubkey.to_string(), config]);
        self.subscribe("program", params).await
    }

    pub async fn vote_subscribe(&self) -> SubscribeResult<'_, RpcVote> {
        self.subscribe("vote", json!([])).await
    }

    pub async fn root_subscribe(&self) -> SubscribeResult<'_, Slot> {
        self.subscribe("root", json!([])).await
    }

    pub async fn signature_subscribe(
        &self,
        signature: &Signature,
        config: Option<RpcSignatureSubscribeConfig>,
    ) -> SubscribeResult<'_, RpcResponse<RpcSignatureResult>> {
        let params = json!([signature.to_string(), config]);
        self.subscribe("signature", params).await
    }

    pub async fn slot_subscribe(&self) -> SubscribeResult<'_, SlotInfo> {
        self.subscribe("slot", json!([])).await
    }

    pub async fn slot_updates_subscribe(&self) -> SubscribeResult<'_, SlotUpdate> {
        self.subscribe("slotsUpdates", json!([])).await
    }

    async fn run_ws(
        mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
        mut subscribe_rx: mpsc::UnboundedReceiver<SubscribeRequestMsg>,
        mut shutdown_rx: oneshot::Receiver<()>,
    ) -> PubsubClientResult {
        let mut request_id: u64 = 0;

        let mut requests_subscribe = BTreeMap::new();
        let mut requests_unsubscribe = BTreeMap::<u64, oneshot::Sender<()>>::new();
        let mut subscriptions = BTreeMap::new();
        let (unsubscribe_tx, mut unsubscribe_rx) = mpsc::unbounded_channel();

        loop {
            tokio::select! {
                // Send close on shutdown signal
                _ = (&mut shutdown_rx) => {
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
                Some((operation, params, response_tx)) = subscribe_rx.recv() => {
                    request_id += 1;
                    let method = format!("{}Subscribe", operation);
                    let text = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":params}).to_string();
                    ws.send(Message::Text(text)).await?;
                    requests_subscribe.insert(request_id, (operation, response_tx));
                },
                // Read message for unsubscribe
                Some((operation, sid, response_tx)) = unsubscribe_rx.recv() => {
                    subscriptions.remove(&sid);
                    request_id += 1;
                    let method = format!("{}Unsubscribe", operation);
                    let text = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":[sid]}).to_string();
                    ws.send(Message::Text(text)).await?;
                    requests_unsubscribe.insert(request_id, response_tx);
                },
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

                        if let Some(response_tx) = requests_unsubscribe.remove(&id) {
                            let _ = response_tx.send(()); // do not care if receiver is closed
                        } else if let Some((operation, response_tx)) = requests_subscribe.remove(&id) {
                            match err {
                                Some(reason) => {
                                    let _ = response_tx.send(Err(PubsubClientError::SubscribeFailed { reason, message: text.clone()}));
                                },
                                None => {
                                    // Subscribe Id
                                    let sid = json.get("result").and_then(Value::as_u64).ok_or_else(|| {
                                        PubsubClientError::SubscribeFailed { reason: "invalid `result` field".into(), message: text.clone() }
                                    })?;

                                    // Create notifications channel and unsubscribe function
                                    let (notifications_tx, notifications_rx) = mpsc::unbounded_channel();
                                    let unsubscribe_tx = unsubscribe_tx.clone();
                                    let unsubscribe = Box::new(move || async move {
                                        let (response_tx, response_rx) = oneshot::channel();
                                        // do nothing if ws already closed
                                        if unsubscribe_tx.send((operation, sid, response_tx)).is_ok() {
                                            let _ = response_rx.await; // channel can be closed only if ws is closed
                                        }
                                    }.boxed());

                                    if response_tx.send(Ok((notifications_rx, unsubscribe))).is_err() {
                                        break;
                                    }
                                    subscriptions.insert(sid, notifications_tx);
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

                            if let Some(notifications_tx) = subscriptions.get(&sid) {
                                if let Some(result) = params.remove("result") {
                                    if notifications_tx.send(result).is_err() {
                                        unsubscribe_required = true;
                                    }
                                }
                            } else {
                                unsubscribe_required = true;
                            }

                            if unsubscribe_required {
                                if let Some(Value::String(method)) = json.remove("method") {
                                    if let Some(operation) = method.strip_suffix("Notification") {
                                        let (response_tx, _response_rx) = oneshot::channel();
                                        let _ = unsubscribe_tx.send((operation.to_string(), sid, response_tx));
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
