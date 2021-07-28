//! The `pubsub` module implements a threaded subscription service on client RPC request

use {
    crate::{
        rpc_pubsub::{RpcSolPubSub, RpcSolPubSubImpl},
        rpc_subscription_tracker::{SubscriptionId, SubscriptionToken},
        rpc_subscriptions::{RpcNotification, RpcSubscriptions},
    },
    dashmap::DashMap,
    jsonrpc_core::IoHandler,
    soketto::handshake::{server, Server},
    std::{
        net::SocketAddr,
        str,
        sync::Arc,
        thread::{self, Builder, JoinHandle},
    },
    stream_cancel::{Trigger, Tripwire},
    tokio::{net::TcpStream, pin, select},
    tokio_util::compat::TokioAsyncReadCompatExt,
};

pub const MAX_ACTIVE_SUBSCRIPTIONS: usize = 1_000_000;
pub const DEFAULT_BROADCAST_CHANNEL_CAPACITY: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct PubSubConfig {
    pub enable_vote_subscription: bool,
    pub max_active_subscriptions: usize,
    pub queue_capacity: usize,
}

impl Default for PubSubConfig {
    fn default() -> Self {
        Self {
            enable_vote_subscription: false,
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
            queue_capacity: DEFAULT_BROADCAST_CHANNEL_CAPACITY,
        }
    }
}

pub struct PubSubService {
    thread_hdl: JoinHandle<()>,
}

impl PubSubService {
    pub fn new(
        pubsub_config: PubSubConfig,
        subscriptions: &Arc<RpcSubscriptions>,
        pubsub_addr: SocketAddr,
    ) -> (Trigger, Self) {
        info!("rpc_pubsub bound to {:?}", pubsub_addr);
        let subscriptions = subscriptions.clone();
        let (trigger, tripwire) = Tripwire::new();
        let thread_hdl = Builder::new()
            .name("solana-pubsub".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("runtime creation failed");
                if let Err(err) =
                    runtime.block_on(listen(pubsub_addr, pubsub_config, subscriptions, tripwire))
                {
                    error!("pubsub service failed: {}", err);
                };
            })
            .expect("thread spawn failed");

        (trigger, Self { thread_hdl })
    }

    pub fn close(self) -> thread::Result<()> {
        self.join()
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

type StdResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct BroadcastHandler {
    current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>,
}

impl BroadcastHandler {
    fn handle(&self, notification: RpcNotification) -> Option<Arc<str>> {
        if self
            .current_subscriptions
            .contains_key(&notification.subscription_id)
        {
            if notification.is_final {
                self.current_subscriptions
                    .remove(&notification.subscription_id);
            }
            Some(notification.json)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub struct TestBroadcastReceiver {
    handler: BroadcastHandler,
    inner: tokio::sync::broadcast::Receiver<RpcNotification>,
}

#[cfg(test)]
impl TestBroadcastReceiver {
    pub fn recv(&mut self) -> String {
        use std::thread::sleep;
        use std::time::{Duration, Instant};
        use tokio::sync::broadcast::error::TryRecvError;

        let timeout = Duration::from_millis(100);
        let started = Instant::now();

        loop {
            match self.inner.try_recv() {
                Ok(notification) => {
                    if let Some(json) = self.handler.handle(notification) {
                        return json.to_string();
                    }
                }
                Err(TryRecvError::Empty) => {
                    if started.elapsed() > timeout {
                        panic!("TestBroadcastReceiver: no data, timeout reached");
                    }
                    sleep(Duration::from_millis(50));
                }
                Err(err) => panic!("broadcast receiver error: {}", err),
            }
        }
    }
}

#[cfg(test)]
pub fn test_connection(
    subscriptions: &Arc<RpcSubscriptions>,
) -> (RpcSolPubSubImpl, TestBroadcastReceiver) {
    let current_subscriptions = Arc::new(DashMap::new());
    let broadcast_receiver = subscriptions.broadcast_receiver();
    let rpc_impl = RpcSolPubSubImpl::new(
        PubSubConfig {
            enable_vote_subscription: true,
            ..PubSubConfig::default()
        },
        Arc::clone(subscriptions),
        Arc::clone(&current_subscriptions),
    );
    let broadcast_handler = BroadcastHandler {
        current_subscriptions,
    };
    let receiver = TestBroadcastReceiver {
        inner: broadcast_receiver,
        handler: broadcast_handler,
    };
    (rpc_impl, receiver)
}

async fn handle_connection(
    socket: TcpStream,
    subscriptions: Arc<RpcSubscriptions>,
    config: PubSubConfig,
    mut tripwire: Tripwire,
) -> StdResult<()> {
    let mut server = Server::new(socket.compat());
    let request = server.receive_request().await?;
    let accept = server::Response::Accept {
        key: request.key(),
        protocol: None,
    };
    server.send_response(&accept).await?;
    let (mut sender, mut receiver) = server.into_builder().finish();

    let mut broadcast_receiver = subscriptions.broadcast_receiver();
    let mut data = Vec::new();
    let current_subscriptions = Arc::new(DashMap::new());

    let mut json_rpc_handler = IoHandler::new();
    let rpc_impl = RpcSolPubSubImpl::new(config, subscriptions, Arc::clone(&current_subscriptions));
    json_rpc_handler.extend_with(rpc_impl.to_delegate());
    let broadcast_handler = BroadcastHandler {
        current_subscriptions,
    };
    loop {
        // Extra block for dropping `receive_future`.
        {
            // soketto is not cancel safe, so we have to introduce an inner loop to poll
            // `receive_data` to completion.
            let receive_future = receiver.receive_data(&mut data);
            pin!(receive_future);
            loop {
                select! {
                    result = &mut receive_future => match result {
                        Ok(_) => break,
                        Err(soketto::connection::Error::Closed) => return Ok(()),
                        Err(err) => return Err(err.into()),
                    },
                    result = broadcast_receiver.recv() => match result {
                        Ok(notification) => {
                            if let Some(json) = broadcast_handler.handle(notification) {
                                sender.send_text(&json).await?;

                            }
                        }
                        // In both possible error cases (closed or lagged) we disconnect the client.
                        Err(_) => return Ok(()),
                    },
                    _ = &mut tripwire => return Ok(()),
                }
            }
        }
        let data_str = match str::from_utf8(&data) {
            Ok(str) => str,
            Err(_) => {
                // Old implementation just closes the connection, so we preserve that behavior
                // for now. It would be more correct to respond with an error.
                break;
            }
        };

        if let Some(response) = json_rpc_handler.handle_request(data_str).await {
            sender.send_text(&response).await?;
        }
        data.clear();
    }

    Ok(())
}

async fn listen(
    listen_address: SocketAddr,
    config: PubSubConfig,
    subscriptions: Arc<RpcSubscriptions>,
    mut tripwire: Tripwire,
) -> StdResult<()> {
    let listener = tokio::net::TcpListener::bind(&listen_address).await?;
    loop {
        select! {
            result = listener.accept() => match result {
                Ok((socket, addr)) => {
                    debug!("new client: {:?}", addr);
                    let subscriptions = subscriptions.clone();
                    let config = config.clone();
                    let tripwire = tripwire.clone();
                    tokio::spawn(async move {
                        if let Err(err) =
                            handle_connection(socket, subscriptions, config, tripwire).await
                        {
                            debug!("connection handler error: {}", err);
                        }
                    });
                }
                Err(e) => error!("couldn't accept connection: {:?}", e),
            },
            _ = &mut tripwire => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::optimistically_confirmed_bank_tracker::OptimisticallyConfirmedBank,
        solana_runtime::{
            bank::Bank,
            bank_forks::BankForks,
            commitment::BlockCommitmentCache,
            genesis_utils::{create_genesis_config, GenesisConfigInfo},
        },
        std::{
            net::{IpAddr, Ipv4Addr},
            sync::{atomic::AtomicBool, RwLock},
        },
    };

    #[test]
    fn test_pubsub_new() {
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let exit = Arc::new(AtomicBool::new(false));
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = Arc::new(RpcSubscriptions::new(
            &exit,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            optimistically_confirmed_bank,
        ));
        let (_trigger, pubsub_service) =
            PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);
        let thread = pubsub_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-pubsub");
    }
}
