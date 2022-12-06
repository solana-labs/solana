//! The `pubsub` module implements a threaded subscription service on client RPC request

use {
    crate::{
        rpc_pubsub::{RpcSolPubSubImpl, RpcSolPubSubInternal},
        rpc_subscription_tracker::{
            SubscriptionControl, SubscriptionId, SubscriptionParams, SubscriptionToken,
        },
        rpc_subscriptions::{RpcNotification, RpcSubscriptions},
    },
    dashmap::{mapref::entry::Entry, DashMap},
    jsonrpc_core::IoHandler,
    soketto::handshake::{server, Server},
    solana_metrics::TokenCounter,
    std::{
        io,
        net::SocketAddr,
        str,
        sync::Arc,
        thread::{self, Builder, JoinHandle},
    },
    stream_cancel::{Trigger, Tripwire},
    thiserror::Error,
    tokio::{net::TcpStream, pin, select, sync::broadcast},
    tokio_util::compat::TokioAsyncReadCompatExt,
};

pub const MAX_ACTIVE_SUBSCRIPTIONS: usize = 1_000_000;
pub const DEFAULT_QUEUE_CAPACITY_ITEMS: usize = 10_000_000;
pub const DEFAULT_TEST_QUEUE_CAPACITY_ITEMS: usize = 100;
pub const DEFAULT_QUEUE_CAPACITY_BYTES: usize = 256 * 1024 * 1024;
pub const DEFAULT_WORKER_THREADS: usize = 1;

#[derive(Debug, Clone)]
pub struct PubSubConfig {
    pub enable_block_subscription: bool,
    pub enable_vote_subscription: bool,
    pub max_active_subscriptions: usize,
    pub queue_capacity_items: usize,
    pub queue_capacity_bytes: usize,
    pub worker_threads: usize,
    pub notification_threads: Option<usize>,
}

impl Default for PubSubConfig {
    fn default() -> Self {
        Self {
            enable_block_subscription: false,
            enable_vote_subscription: false,
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
            queue_capacity_items: DEFAULT_QUEUE_CAPACITY_ITEMS,
            queue_capacity_bytes: DEFAULT_QUEUE_CAPACITY_BYTES,
            worker_threads: DEFAULT_WORKER_THREADS,
            notification_threads: None,
        }
    }
}

impl PubSubConfig {
    pub fn default_for_tests() -> Self {
        Self {
            enable_block_subscription: false,
            enable_vote_subscription: false,
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
            queue_capacity_items: DEFAULT_TEST_QUEUE_CAPACITY_ITEMS,
            queue_capacity_bytes: DEFAULT_QUEUE_CAPACITY_BYTES,
            worker_threads: DEFAULT_WORKER_THREADS,
            notification_threads: Some(2),
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
        let subscription_control = subscriptions.control().clone();
        info!("rpc_pubsub bound to {:?}", pubsub_addr);

        let (trigger, tripwire) = Tripwire::new();
        let thread_hdl = Builder::new()
            .name("solRpcPubSub".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(pubsub_config.worker_threads)
                    .enable_all()
                    .build()
                    .expect("runtime creation failed");
                if let Err(err) = runtime.block_on(listen(
                    pubsub_addr,
                    pubsub_config,
                    subscription_control,
                    tripwire,
                )) {
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

struct BroadcastHandler {
    current_subscriptions: Arc<DashMap<SubscriptionId, SubscriptionToken>>,
}

fn count_final(params: &SubscriptionParams) {
    match params {
        SubscriptionParams::Account(_) => {
            inc_new_counter_info!("rpc-pubsub-final-accounts", 1);
        }
        SubscriptionParams::Logs(_) => {
            inc_new_counter_info!("rpc-pubsub-final-logs", 1);
        }
        SubscriptionParams::Program(_) => {
            inc_new_counter_info!("rpc-pubsub-final-programs", 1);
        }
        SubscriptionParams::Signature(_) => {
            inc_new_counter_info!("rpc-pubsub-final-signatures", 1);
        }
        SubscriptionParams::Slot => {
            inc_new_counter_info!("rpc-pubsub-final-slots", 1);
        }
        SubscriptionParams::SlotsUpdates => {
            inc_new_counter_info!("rpc-pubsub-final-slots-updates", 1);
        }
        SubscriptionParams::Root => {
            inc_new_counter_info!("rpc-pubsub-final-roots", 1);
        }
        SubscriptionParams::Vote => {
            inc_new_counter_info!("rpc-pubsub-final-votes", 1);
        }
        SubscriptionParams::Block(_) => {
            inc_new_counter_info!("rpc-pubsub-final-slot-txs", 1);
        }
    }
}

impl BroadcastHandler {
    fn handle(&self, notification: RpcNotification) -> Result<Option<Arc<String>>, Error> {
        if let Entry::Occupied(entry) = self
            .current_subscriptions
            .entry(notification.subscription_id)
        {
            count_final(entry.get().params());

            let time_since_created = notification.created_at.elapsed();

            datapoint_info!(
                "pubsub_notifications",
                (
                    "created_to_queue_time_us",
                    time_since_created.as_micros() as i64,
                    i64
                ),
            );

            if notification.is_final {
                entry.remove();
            }
            notification
                .json
                .upgrade()
                .ok_or(Error::NotificationIsGone)
                .map(Some)
        } else {
            Ok(None)
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
        match self.recv_timeout(std::time::Duration::from_secs(10)) {
            Err(err) => panic!("broadcast receiver error: {err}"),
            Ok(str) => str,
        }
    }

    pub fn recv_timeout(&mut self, timeout: std::time::Duration) -> Result<String, String> {
        use {std::thread::sleep, tokio::sync::broadcast::error::TryRecvError};

        let started = std::time::Instant::now();

        loop {
            match self.inner.try_recv() {
                Ok(notification) => {
                    debug!(
                        "TestBroadcastReceiver: {:?}ms elapsed",
                        started.elapsed().as_millis()
                    );
                    if let Some(json) = self.handler.handle(notification).expect("handler failed") {
                        return Ok(json.to_string());
                    }
                }
                Err(TryRecvError::Empty) => {
                    if started.elapsed() > timeout {
                        return Err("TestBroadcastReceiver: no data, timeout reached".into());
                    }
                    sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

#[cfg(test)]
pub fn test_connection(
    subscriptions: &Arc<RpcSubscriptions>,
) -> (RpcSolPubSubImpl, TestBroadcastReceiver) {
    let current_subscriptions = Arc::new(DashMap::new());

    let rpc_impl = RpcSolPubSubImpl::new(
        PubSubConfig {
            enable_block_subscription: true,
            enable_vote_subscription: true,
            queue_capacity_items: 100,
            ..PubSubConfig::default()
        },
        subscriptions.control().clone(),
        Arc::clone(&current_subscriptions),
    );
    let broadcast_handler = BroadcastHandler {
        current_subscriptions,
    };
    let receiver = TestBroadcastReceiver {
        inner: subscriptions.control().broadcast_receiver(),
        handler: broadcast_handler,
    };
    (rpc_impl, receiver)
}

#[derive(Debug, Error)]
enum Error {
    #[error("handshake error: {0}")]
    Handshake(#[from] soketto::handshake::Error),
    #[error("connection error: {0}")]
    Connection(#[from] soketto::connection::Error),
    #[error("broadcast queue error: {0}")]
    Broadcast(#[from] broadcast::error::RecvError),
    #[error("client has lagged behind (notification is gone)")]
    NotificationIsGone,
}

async fn handle_connection(
    socket: TcpStream,
    subscription_control: SubscriptionControl,
    config: PubSubConfig,
    mut tripwire: Tripwire,
) -> Result<(), Error> {
    let mut server = Server::new(socket.compat());
    let request = server.receive_request().await?;
    let accept = server::Response::Accept {
        key: request.key(),
        protocol: None,
    };
    server.send_response(&accept).await?;
    let (mut sender, mut receiver) = server.into_builder().finish();

    let mut broadcast_receiver = subscription_control.broadcast_receiver();
    let mut data = Vec::new();
    let current_subscriptions = Arc::new(DashMap::new());

    let mut json_rpc_handler = IoHandler::new();
    let rpc_impl = RpcSolPubSubImpl::new(
        config,
        subscription_control,
        Arc::clone(&current_subscriptions),
    );
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
                    result = broadcast_receiver.recv() => {

                        // In both possible error cases (closed or lagged) we disconnect the client.
                        if let Some(json) = broadcast_handler.handle(result?)? {
                            sender.send_text(&*json).await?;
                        }
                    },
                    _ = &mut tripwire => {
                        warn!("disconnecting websocket client: shutting down");
                        return Ok(())
                    },

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
    subscription_control: SubscriptionControl,
    mut tripwire: Tripwire,
) -> io::Result<()> {
    let listener = tokio::net::TcpListener::bind(&listen_address).await?;
    let counter = TokenCounter::new("rpc_pubsub_connections");
    loop {
        select! {
            result = listener.accept() => match result {
                Ok((socket, addr)) => {
                    debug!("new client ({:?})", addr);
                    let subscription_control = subscription_control.clone();
                    let config = config.clone();
                    let tripwire = tripwire.clone();
                    let counter_token = counter.create_token();
                    tokio::spawn(async move {
                        let handle = handle_connection(
                            socket, subscription_control, config, tripwire
                        );
                        match handle.await {
                            Ok(()) => debug!("connection closed ({:?})", addr),
                            Err(err) => warn!("connection handler error ({:?}): {}", addr, err),
                        }
                        drop(counter_token); // Force moving token into the task.
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
            sync::{
                atomic::{AtomicBool, AtomicU64},
                RwLock,
            },
        },
    };

    #[test]
    fn test_pubsub_new() {
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let exit = Arc::new(AtomicBool::new(false));
        let max_complete_transaction_status_slot = Arc::new(AtomicU64::default());
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config(10_000);
        let bank = Bank::new_for_tests(&genesis_config);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let optimistically_confirmed_bank =
            OptimisticallyConfirmedBank::locked_from_bank_forks_root(&bank_forks);
        let subscriptions = Arc::new(RpcSubscriptions::new_for_tests(
            &exit,
            max_complete_transaction_status_slot,
            bank_forks,
            Arc::new(RwLock::new(BlockCommitmentCache::new_for_tests())),
            optimistically_confirmed_bank,
        ));
        let (_trigger, pubsub_service) =
            PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr);
        let thread = pubsub_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solRpcPubSub");
    }
}
