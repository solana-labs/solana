//! The `pubsub` module implements a threaded subscription service on client RPC request

use crate::{
    rpc_pubsub::{RpcSolPubSub, RpcSolPubSubImpl},
    rpc_subscriptions::RpcSubscriptions,
    validator::ValidatorExit,
};
use jsonrpc_pubsub::{PubSubHandler, Session};
use jsonrpc_ws_server::{CloseHandle, RequestContext, ServerBuilder};
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};

#[derive(Default)]
pub struct PubSubService {
    close_handle: Option<CloseHandle>,
}

impl PubSubService {
    pub fn new(
        pubsub_addr: SocketAddr,
        subscriptions: Arc<RpcSubscriptions>,
        validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
    ) -> Self {
        info!("rpc_pubsub bound to {:?}", pubsub_addr);
        let rpc = RpcSolPubSubImpl::new(subscriptions);
        let mut io = PubSubHandler::default();
        io.extend_with(rpc.to_delegate());

        // Start pub sub server in new thread
        let server = ServerBuilder::with_meta_extractor(io, |context: &RequestContext| {
            info!("New pubsub connection");
            let session = Arc::new(Session::new(context.sender()));
            session.on_drop(|| {
                info!("Pubsub connection dropped");
            });
            session
        })
        .start(&pubsub_addr);

        match server {
            Ok(server) => {
                let close_handle = server.close_handle();
                let close_handle_ = close_handle.clone();

                let mut validator_exit_write = validator_exit.write().unwrap();
                validator_exit_write
                    .as_mut()
                    .unwrap()
                    .register_exit(Box::new(move || close_handle_.close()));

                Self {
                    close_handle: Some(close_handle),
                }
            }
            Err(err) => {
                warn!("Pubsub service unavailable error: {:?}. \nAlso, check that port {} is not already in use by another application", err, pubsub_addr.port());
                Self::default()
            }
        }
    }

    pub fn exit(mut self) {
        if let Some(c) = self.close_handle.take() {
            c.close()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::tests::create_validator_exit;
    use std::{
        net::{IpAddr, Ipv4Addr},
        sync::atomic::AtomicBool,
    };

    #[test]
    fn test_pubsub_new() {
        let pubsub_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let subscriptions = Arc::new(RpcSubscriptions::new(&exit));
        let pubsub_service = PubSubService::new(pubsub_addr, subscriptions, validator_exit);
        pubsub_service.exit();
    }
}
