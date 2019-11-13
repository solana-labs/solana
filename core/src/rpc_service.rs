//! The `rpc_service` module implements the Solana JSON RPC service.

use crate::{
    cluster_info::ClusterInfo, commitment::BlockCommitmentCache, rpc::*, service::Service,
    storage_stage::StorageState, validator::ValidatorExit,
};
use jsonrpc_core::MetaIoHandler;
use jsonrpc_http_server::{
    hyper, AccessControlAllowOrigin, CloseHandle, DomainsValidation, RequestMiddleware,
    RequestMiddlewareAction, ServerBuilder,
};
use solana_ledger::{bank_forks::BankForks, blocktree::Blocktree};
use solana_sdk::hash::Hash;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, RwLock},
    thread::{self, Builder, JoinHandle},
};
use tokio::prelude::Future;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,

    #[cfg(test)]
    pub request_processor: Arc<RwLock<JsonRpcRequestProcessor>>, // Used only by test_rpc_new()...

    close_handle: Option<CloseHandle>,
}

#[derive(Default)]
struct RpcRequestMiddleware {
    ledger_path: PathBuf,
}
impl RpcRequestMiddleware {
    pub fn new(ledger_path: PathBuf) -> Self {
        Self { ledger_path }
    }

    fn not_found() -> hyper::Response<hyper::Body> {
        hyper::Response::builder()
            .status(hyper::StatusCode::NOT_FOUND)
            .body(hyper::Body::empty())
            .unwrap()
    }

    fn internal_server_error() -> hyper::Response<hyper::Body> {
        hyper::Response::builder()
            .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
            .body(hyper::Body::empty())
            .unwrap()
    }

    fn get(&self, filename: &str) -> RequestMiddlewareAction {
        info!("get {}", filename);
        let filename = self.ledger_path.join(filename);
        RequestMiddlewareAction::Respond {
            should_validate_hosts: true,
            response: Box::new(
                tokio_fs::file::File::open(filename)
                    .and_then(|file| {
                        let buf: Vec<u8> = Vec::new();
                        tokio_io::io::read_to_end(file, buf)
                            .and_then(|item| Ok(hyper::Response::new(item.1.into())))
                            .or_else(|_| Ok(RpcRequestMiddleware::internal_server_error()))
                    })
                    .or_else(|_| Ok(RpcRequestMiddleware::not_found())),
            ),
        }
    }
}

impl RequestMiddleware for RpcRequestMiddleware {
    fn on_request(&self, request: hyper::Request<hyper::Body>) -> RequestMiddlewareAction {
        trace!("request uri: {}", request.uri());
        match request.uri().path() {
            "/snapshot.tar.bz2" => self.get("snapshot.tar.bz2"),
            "/genesis.tar.bz2" => self.get("genesis.tar.bz2"),
            _ => RequestMiddlewareAction::Proceed {
                should_continue_on_invalid_cors: false,
                request,
            },
        }
    }
}

impl JsonRpcService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rpc_addr: SocketAddr,
        config: JsonRpcConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        blocktree: Arc<Blocktree>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        genesis_hash: Hash,
        ledger_path: &Path,
        storage_state: StorageState,
        validator_exit: &Arc<RwLock<Option<ValidatorExit>>>,
    ) -> Self {
        info!("rpc bound to {:?}", rpc_addr);
        info!("rpc configuration: {:?}", config);
        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            config,
            bank_forks,
            block_commitment_cache,
            blocktree,
            storage_state,
            validator_exit,
        )));
        let request_processor_ = request_processor.clone();

        let cluster_info = cluster_info.clone();
        let ledger_path = ledger_path.to_path_buf();

        let (close_handle_sender, close_handle_receiver) = channel();
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
                        genesis_hash
                    }).threads(4)
                        .cors(DomainsValidation::AllowOnly(vec![
                            AccessControlAllowOrigin::Any,
                        ]))
                        .request_middleware(RpcRequestMiddleware::new(ledger_path))
                        .start_http(&rpc_addr);
                if let Err(e) = server {
                    warn!("JSON RPC service unavailable error: {:?}. \nAlso, check that port {} is not already in use by another application", e, rpc_addr.port());
                    return;
                }

                let server = server.unwrap();
                close_handle_sender.send(server.close_handle()).unwrap();
                server.wait();
            })
            .unwrap();

        let close_handle = close_handle_receiver.recv().unwrap();
        let close_handle_ = close_handle.clone();
        let mut validator_exit_write = validator_exit.write().unwrap();
        validator_exit_write
            .as_mut()
            .unwrap()
            .register_exit(Box::new(move || close_handle_.close()));
        Self {
            thread_hdl,
            #[cfg(test)]
            request_processor,
            close_handle: Some(close_handle),
        }
    }

    pub fn exit(&mut self) {
        if let Some(c) = self.close_handle.take() {
            c.close()
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
    use crate::{
        contact_info::ContactInfo,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        rpc::tests::create_validator_exit,
    };
    use solana_ledger::blocktree::get_tmp_ledger_path;
    use solana_runtime::bank::Bank;
    use solana_sdk::signature::KeypairUtil;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_rpc_new() {
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            ..
        } = create_genesis_config(10_000);
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let bank = Bank::new(&genesis_config);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            ContactInfo::default(),
        )));
        let rpc_addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            solana_net_utils::find_available_port_in_range((10000, 65535)).unwrap(),
        );
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank.slot(), bank)));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let mut rpc_service = JsonRpcService::new(
            rpc_addr,
            JsonRpcConfig::default(),
            bank_forks,
            block_commitment_cache,
            Arc::new(blocktree),
            &cluster_info,
            Hash::default(),
            &PathBuf::from("farf"),
            StorageState::default(),
            &validator_exit,
        );
        let thread = rpc_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-jsonrpc");

        assert_eq!(
            10_000,
            rpc_service
                .request_processor
                .read()
                .unwrap()
                .get_balance(Ok(mint_keypair.pubkey()), None)
                .unwrap()
                .value
        );
        rpc_service.exit();
        rpc_service.join().unwrap();
    }
}
