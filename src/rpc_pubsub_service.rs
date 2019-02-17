//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::bank::Bank;
use crate::rpc_pubsub::{RpcPubSubBank, RpcSolPubSub, RpcSolPubSubImpl};
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use jsonrpc_pubsub::{PubSubHandler, Session};
use jsonrpc_ws_server::{RequestContext, ServerBuilder};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct PubSubService {
    thread_hdl: JoinHandle<()>,
    exit: Arc<AtomicBool>,
    rpc_bank: Arc<RwLock<RpcPubSubBank>>,
    subscription: Arc<RpcSubscriptions>,
}

impl Service for PubSubService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

impl PubSubService {
    pub fn new(bank: &Arc<Bank>, pubsub_addr: SocketAddr) -> Self {
        info!("rpc_pubsub bound to {:?}", pubsub_addr);
        let rpc_bank = Arc::new(RwLock::new(RpcPubSubBank::new(bank.clone())));
        let rpc = RpcSolPubSubImpl::new(rpc_bank.clone());
        let subscription = rpc.subscription.clone();
        bank.set_subscriptions(subscription.clone());
        let exit = Arc::new(AtomicBool::new(false));
        let exit_ = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-pubsub".to_string())
            .spawn(move || {
                let mut io = PubSubHandler::default();
                io.extend_with(rpc.to_delegate());

                let server = ServerBuilder::with_meta_extractor(io, |context: &RequestContext| {
                        info!("New pubsub connection");
                        let session = Arc::new(Session::new(context.sender().clone()));
                        session.on_drop(|| {
                            info!("Pubsub connection dropped");
                        });
                        session
                })
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
        PubSubService {
            thread_hdl,
            exit,
            rpc_bank,
            subscription,
        }
    }

    pub fn set_bank(&self, bank: &Arc<Bank>) {
        self.rpc_bank.write().unwrap().bank = bank.clone();
        bank.set_subscriptions(self.subscription.clone());
    }

    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit();
        self.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::genesis_block::GenesisBlock;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_pubsub_new() {
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new(&genesis_block);
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let pubsub_service = PubSubService::new(&Arc::new(bank), pubsub_addr);
        let thread = pubsub_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-pubsub");
    }
}
