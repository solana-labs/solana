//! The `pubsub` module implements a threaded subscription service on client RPC request

use {
    crate::{
        rpc_pubsub::{RpcSolPubSub, RpcSolPubSubImpl, MAX_ACTIVE_SUBSCRIPTIONS},
        rpc_subscriptions::RpcSubscriptions,
    },
    jsonrpc_pubsub::{PubSubHandler, Session},
    jsonrpc_ws_server::{RequestContext, ServerBuilder},
    std::{
        net::SocketAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::Duration,
    },
};

#[derive(Debug, Clone)]
pub struct PubSubConfig {
    pub enable_vote_subscription: bool,

    // See the corresponding fields in
    // https://github.com/paritytech/ws-rs/blob/be4d47575bae55c60d9f51b47480d355492a94fc/src/lib.rs#L131
    // for a complete description of each field in this struct
    pub max_connections: usize,
    pub max_fragment_size: usize,
    pub max_in_buffer_capacity: usize,
    pub max_out_buffer_capacity: usize,
    pub max_active_subscriptions: usize,
}

impl Default for PubSubConfig {
    fn default() -> Self {
        Self {
            enable_vote_subscription: false,
            max_connections: 1000, // Arbitrary, default of 100 is too low
            max_fragment_size: 50 * 1024, // 50KB
            max_in_buffer_capacity: 50 * 1024, // 50KB
            max_out_buffer_capacity: 15 * 1024 * 1024, // max account size (10MB), then 5MB extra for base64 encoding overhead/etc
            max_active_subscriptions: MAX_ACTIVE_SUBSCRIPTIONS,
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
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!("rpc_pubsub bound to {:?}", pubsub_addr);
        let rpc = RpcSolPubSubImpl::new(
            subscriptions.clone(),
            pubsub_config.max_active_subscriptions,
        );
        let exit_ = exit.clone();

        let thread_hdl = Builder::new()
            .name("solana-pubsub".to_string())
            .spawn(move || {
                let mut io = PubSubHandler::default();
                io.extend_with(rpc.to_delegate());

                let server = ServerBuilder::with_meta_extractor(io, |context: &RequestContext| {
                    info!("New pubsub connection");
                    let session = Arc::new(Session::new(context.sender()));
                    session.on_drop(|| {
                        info!("Pubsub connection dropped");
                    });
                    session
                })
                .max_connections(pubsub_config.max_connections)
                .max_payload(pubsub_config.max_fragment_size)
                .max_in_buffer_capacity(pubsub_config.max_in_buffer_capacity)
                .max_out_buffer_capacity(pubsub_config.max_out_buffer_capacity)
                .start(&pubsub_addr);

                if let Err(e) = server {
                    warn!(
                        "Pubsub service unavailable error: {:?}. \n\
                           Also, check that port {} is not already in use by another application",
                        e,
                        pubsub_addr.port()
                    );
                    return;
                }
                while !exit_.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100));
                }
                server.unwrap().close();
            })
            .unwrap();
        Self { thread_hdl }
    }

    pub fn close(self) -> thread::Result<()> {
        self.join()
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
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
            sync::RwLock,
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
        let pubsub_service =
            PubSubService::new(PubSubConfig::default(), &subscriptions, pubsub_addr, &exit);
        let thread = pubsub_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-pubsub");
    }
}
