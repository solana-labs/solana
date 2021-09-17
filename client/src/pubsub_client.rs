use {
    crate::{
        rpc_config::{
            RpcSignatureSubscribeConfig, RpcTransactionLogsConfig, RpcTransactionLogsFilter,
        },
        rpc_response::{
            Response as RpcResponse, RpcLogsResponse, RpcSignatureResult, SlotInfo, SlotUpdate,
        },
    },
    log::*,
    serde::de::DeserializeOwned,
    serde_json::{
        json,
        value::Value::{Number, Object},
        Map, Value,
    },
    solana_sdk::signature::Signature,
    std::{
        marker::PhantomData,
        net::TcpStream,
        sync::{
            atomic::{AtomicBool, Ordering},
            mpsc::{channel, Receiver},
            Arc, RwLock,
        },
        thread::{sleep, JoinHandle},
        time::Duration,
    },
    thiserror::Error,
    tungstenite::{connect, stream::MaybeTlsStream, Message, WebSocket},
    url::{ParseError, Url},
};

#[derive(Debug, Error)]
pub enum PubsubClientError {
    #[error("url parse error")]
    UrlParseError(#[from] ParseError),

    #[error("unable to connect to server")]
    ConnectionError(#[from] tungstenite::Error),

    #[error("json parse error")]
    JsonParseError(#[from] serde_json::error::Error),

    #[error("unexpected message format: {0}")]
    UnexpectedMessageError(String),
}

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
        writable_socket
            .write()
            .unwrap()
            .write_message(Message::Text(body))?;
        let message = writable_socket.write().unwrap().read_message()?;
        Self::extract_subscription_id(message)
    }

    fn extract_subscription_id(message: Message) -> Result<u64, PubsubClientError> {
        let message_text = &message.into_text()?;
        let json_msg: Map<String, Value> = serde_json::from_str(message_text)?;

        if let Some(Number(x)) = json_msg.get("result") {
            if let Some(x) = x.as_u64() {
                return Ok(x);
            }
        }
        // TODO: Add proper JSON RPC response/error handling...
        Err(PubsubClientError::UnexpectedMessageError(format!(
            "{:?}",
            json_msg
        )))
    }

    pub fn send_unsubscribe(&self) -> Result<(), PubsubClientError> {
        let method = format!("{}Unsubscribe", self.operation);
        self.socket
            .write()
            .unwrap()
            .write_message(Message::Text(
                json!({
                "jsonrpc":"2.0","id":1,"method":method,"params":[self.subscription_id]
                })
                .to_string(),
            ))
            .map_err(|err| err.into())
    }

    fn read_message(
        writable_socket: &Arc<RwLock<WebSocket<MaybeTlsStream<TcpStream>>>>,
    ) -> Result<T, PubsubClientError> {
        let message = writable_socket.write().unwrap().read_message()?;
        let message_text = &message.into_text().unwrap();
        let json_msg: Map<String, Value> = serde_json::from_str(message_text)?;

        if let Some(Object(params)) = json_msg.get("params") {
            if let Some(result) = params.get("result") {
                let x: T = serde_json::from_value::<T>(result.clone()).unwrap();
                return Ok(x);
            }
        }

        // TODO: Add proper JSON RPC response/error handling...
        Err(PubsubClientError::UnexpectedMessageError(format!(
            "{:?}",
            json_msg
        )))
    }

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

pub type LogsSubscription = (
    PubsubClientSubscription<RpcResponse<RpcLogsResponse>>,
    Receiver<RpcResponse<RpcLogsResponse>>,
);
pub type SlotsSubscription = (PubsubClientSubscription<SlotInfo>, Receiver<SlotInfo>);
pub type SignatureSubscription = (
    PubsubClientSubscription<RpcResponse<RpcSignatureResult>>,
    Receiver<RpcResponse<RpcSignatureResult>>,
);

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
    pub fn logs_subscribe(
        url: &str,
        filter: RpcTransactionLogsFilter,
        config: RpcTransactionLogsConfig,
    ) -> Result<LogsSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = channel();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();

        let subscription_id =
            PubsubClientSubscription::<RpcResponse<RpcLogsResponse>>::send_subscribe(
                &socket_clone,
                json!({
                    "jsonrpc":"2.0","id":1,"method":"logsSubscribe","params":[filter, config]
                })
                .to_string(),
            )?;

        let t_cleanup = std::thread::spawn(move || {
            loop {
                if exit_clone.load(Ordering::Relaxed) {
                    break;
                }

                match PubsubClientSubscription::read_message(&socket_clone) {
                    Ok(message) => match sender.send(message) {
                        Ok(_) => (),
                        Err(err) => {
                            info!("receive error: {:?}", err);
                            break;
                        }
                    },
                    Err(err) => {
                        info!("receive error: {:?}", err);
                        break;
                    }
                }
            }

            info!("websocket - exited receive loop");
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

    pub fn slot_subscribe(url: &str) -> Result<SlotsSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = channel::<SlotInfo>();

        let socket = Arc::new(RwLock::new(socket));
        let socket_clone = socket.clone();
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let subscription_id = PubsubClientSubscription::<SlotInfo>::send_subscribe(
            &socket_clone,
            json!({
                "jsonrpc":"2.0","id":1,"method":"slotSubscribe","params":[]
            })
            .to_string(),
        )?;

        let t_cleanup = std::thread::spawn(move || {
            loop {
                if exit_clone.load(Ordering::Relaxed) {
                    break;
                }
                match PubsubClientSubscription::read_message(&socket_clone) {
                    Ok(message) => match sender.send(message) {
                        Ok(_) => (),
                        Err(err) => {
                            info!("receive error: {:?}", err);
                            break;
                        }
                    },
                    Err(err) => {
                        info!("receive error: {:?}", err);
                        break;
                    }
                }
            }

            info!("websocket - exited receive loop");
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

    pub fn signature_subscribe(
        url: &str,
        signature: &Signature,
        config: Option<RpcSignatureSubscribeConfig>,
    ) -> Result<SignatureSubscription, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;
        let (sender, receiver) = channel();

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
            PubsubClientSubscription::<RpcResponse<RpcSignatureResult>>::send_subscribe(
                &socket_clone,
                body,
            )?;

        let t_cleanup = std::thread::spawn(move || {
            loop {
                if exit_clone.load(Ordering::Relaxed) {
                    break;
                }

                let message: Result<RpcResponse<RpcSignatureResult>, PubsubClientError> =
                    PubsubClientSubscription::read_message(&socket_clone);

                if let Ok(msg) = message {
                    match sender.send(msg.clone()) {
                        Ok(_) => (),
                        Err(err) => {
                            info!("receive error: {:?}", err);
                            break;
                        }
                    }
                } else {
                    info!("receive error: {:?}", message);
                    break;
                }
            }

            info!("websocket - exited receive loop");
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

    pub fn slot_updates_subscribe(
        url: &str,
        handler: impl Fn(SlotUpdate) + Send + 'static,
    ) -> Result<PubsubClientSubscription<SlotUpdate>, PubsubClientError> {
        let url = Url::parse(url)?;
        let socket = connect_with_retry(url)?;

        let socket = Arc::new(RwLock::new(socket));
        let exit = Arc::new(AtomicBool::new(false));
        let exit_clone = exit.clone();
        let subscription_id = PubsubClientSubscription::<SlotUpdate>::send_subscribe(
            &socket,
            json!({
                "jsonrpc":"2.0","id":1,"method":"slotsUpdatesSubscribe","params":[]
            })
            .to_string(),
        )?;

        let t_cleanup = {
            let socket = socket.clone();
            std::thread::spawn(move || {
                loop {
                    if exit_clone.load(Ordering::Relaxed) {
                        break;
                    }
                    match PubsubClientSubscription::read_message(&socket) {
                        Ok(message) => handler(message),
                        Err(err) => {
                            info!("receive error: {:?}", err);
                            break;
                        }
                    }
                }

                info!("websocket - exited receive loop");
            })
        };

        Ok(PubsubClientSubscription {
            message_type: PhantomData,
            operation: "slotsUpdates",
            socket,
            subscription_id,
            t_cleanup: Some(t_cleanup),
            exit,
        })
    }
}

#[cfg(test)]
mod tests {
    // see core/tests/client.rs#test_slot_subscription()
}
