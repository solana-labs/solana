//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::{
    rpc_pubsub::{RpcSolPubSub, RpcSolPubSubImpl},
    rpc_subscriptions::RpcSubscriptions,
};
use jsonrpc_pubsub::{PubSubHandler, Session};
use jsonrpc_ws_server::{RequestContext, ServerBuilder};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, sleep, Builder, JoinHandle},
    time::Duration,
};

pub struct PubSubService {
    thread_hdl: JoinHandle<()>,
}

impl PubSubService {
    pub fn new(
        subscriptions: &Arc<RpcSubscriptions>,
        pubsub_addr: SocketAddr,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!("rpc_pubsub bound to {:?}", pubsub_addr);
        let rpc = RpcSolPubSubImpl::new(subscriptions.clone());
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
                .max_connections(1000) // Arbitrary, default of 100 is too low
                .max_payload(10 * 1024 * 1024 + 1024) // max account size (10MB) + extra (1K)
                .start(&pubsub_addr);

                if let Err(e) = server {
                    warn!("Pubsub service unavailable error: {:?}. \nAlso, check that port {} is not already in use by another application", e, pubsub_addr.port());
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
    use super::*;
    use crate::commitment::BlockCommitmentCache;
    use solana_ledger::{blockstore::Blockstore, get_tmp_ledger_path};
    use std::{
        net::{IpAddr, Ipv4Addr},
        sync::RwLock,
    };

    #[test]
    fn test_pubsub_new() {
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let exit = Arc::new(AtomicBool::new(false));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let subscriptions = Arc::new(RpcSubscriptions::new(
            &exit,
            Arc::new(RwLock::new(
                BlockCommitmentCache::new_for_tests_with_blockstore(blockstore),
            )),
        ));
        let pubsub_service = PubSubService::new(&subscriptions, pubsub_addr, &exit);
        let thread = pubsub_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-pubsub");
    }
}
