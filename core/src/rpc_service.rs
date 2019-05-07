//! The `rpc_service` module implements the Solana JSON RPC service.

use crate::bank_forks::BankForks;
use crate::cluster_info::ClusterInfo;
use crate::rpc::*;
use crate::service::Service;
use crate::storage_stage::StorageState;
use jsonrpc_core::MetaIoHandler;
use jsonrpc_http_server::{hyper, AccessControlAllowOrigin, DomainsValidation, ServerBuilder};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,

    #[cfg(test)]
    pub request_processor: Arc<RwLock<JsonRpcRequestProcessor>>, // Used only by test_rpc_new()...
}

impl JsonRpcService {
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        rpc_addr: SocketAddr,
        storage_state: StorageState,
        config: JsonRpcConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        info!("rpc bound to {:?}", rpc_addr);
        info!("rpc configuration: {:?}", config);
        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            storage_state,
            config,
            bank_forks,
            exit,
        )));
        let request_processor_ = request_processor.clone();

        let cluster_info = cluster_info.clone();
        let exit_ = exit.clone();

        let thread_hdl = Builder::new()
            .name("solana-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let server =
                    ServerBuilder::with_meta_extractor(io, move |_req: &hyper::Request<hyper::Body>| Meta {
                        request_processor: request_processor_.clone(),
                        cluster_info: cluster_info.clone(),
                    }).threads(4)
                        .cors(DomainsValidation::AllowOnly(vec![
                            AccessControlAllowOrigin::Any,
                        ]))
                        .start_http(&rpc_addr);
                if let Err(e) = server {
                    warn!("JSON RPC service unavailable error: {:?}. \nAlso, check that port {} is not already in use by another application", e, rpc_addr.port());
                    return;
                }
                while !exit_.load(Ordering::Relaxed) {
                    sleep(Duration::from_millis(100));
                }
                server.unwrap().close();
            })
            .unwrap();
        Self {
            thread_hdl,
            #[cfg(test)]
            request_processor,
        }
    }
}

impl Service for JsonRpcService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_info::ContactInfo;
    use crate::genesis_utils::create_genesis_block;
    use solana_runtime::bank::Bank;
    use solana_sdk::signature::KeypairUtil;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn test_rpc_new() {
        let (genesis_block, alice) = create_genesis_block(10_000);
        let exit = Arc::new(AtomicBool::new(false));
        let bank = Bank::new(&genesis_block);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            ContactInfo::default(),
        )));
        let rpc_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            solana_netutil::find_available_port_in_range((10000, 65535)).unwrap(),
        );
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank.slot(), bank)));
        let rpc_service = JsonRpcService::new(
            &cluster_info,
            rpc_addr,
            StorageState::default(),
            JsonRpcConfig::default(),
            bank_forks,
            &exit,
        );
        let thread = rpc_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-jsonrpc");

        assert_eq!(
            10_000,
            rpc_service
                .request_processor
                .read()
                .unwrap()
                .get_balance(&alice.pubkey())
        );
        exit.store(true, Ordering::Relaxed);
        rpc_service.join().unwrap();
    }
}
