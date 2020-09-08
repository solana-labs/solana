//! The `rpc_service` module implements the Solana JSON RPC service.

use crate::{
    bigtable_upload_service::BigTableUploadService,
    cluster_info::ClusterInfo,
    poh_recorder::PohRecorder,
    rpc::*,
    rpc_health::*,
    send_transaction_service::{LeaderInfo, SendTransactionService},
    validator::ValidatorExit,
};
use jsonrpc_core::MetaIoHandler;
use jsonrpc_http_server::{
    hyper, AccessControlAllowOrigin, CloseHandle, DomainsValidation, RequestMiddleware,
    RequestMiddlewareAction, ServerBuilder,
};
use regex::Regex;
use solana_ledger::blockstore::Blockstore;
use solana_runtime::{
    bank_forks::{BankForks, SnapshotConfig},
    commitment::BlockCommitmentCache,
    snapshot_utils,
};
use solana_sdk::{hash::Hash, native_token::lamports_to_sol, pubkey::Pubkey};
use std::{
    collections::HashSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    sync::{mpsc::channel, Arc, Mutex, RwLock},
    thread::{self, Builder, JoinHandle},
};
use tokio::runtime;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,

    #[cfg(test)]
    pub request_processor: JsonRpcRequestProcessor, // Used only by test_rpc_new()...

    close_handle: Option<CloseHandle>,
    runtime: runtime::Runtime,
}

struct RpcRequestMiddleware {
    ledger_path: PathBuf,
    snapshot_archive_path_regex: Regex,
    snapshot_config: Option<SnapshotConfig>,
    bank_forks: Arc<RwLock<BankForks>>,
    health: Arc<RpcHealth>,
}

impl RpcRequestMiddleware {
    pub fn new(
        ledger_path: PathBuf,
        snapshot_config: Option<SnapshotConfig>,
        bank_forks: Arc<RwLock<BankForks>>,
        health: Arc<RpcHealth>,
    ) -> Self {
        Self {
            ledger_path,
            snapshot_archive_path_regex: Regex::new(
                r"/snapshot-\d+-[[:alnum:]]+\.tar\.(bz2|zst|gz)$",
            )
            .unwrap(),
            snapshot_config,
            bank_forks,
            health,
        }
    }

    fn redirect(location: &str) -> hyper::Response<hyper::Body> {
        hyper::Response::builder()
            .status(hyper::StatusCode::SEE_OTHER)
            .header(hyper::header::LOCATION, location)
            .body(hyper::Body::from(String::from(location)))
            .unwrap()
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

    fn is_file_get_path(&self, path: &str) -> bool {
        match path {
            "/genesis.tar.bz2" => true,
            _ => {
                if self.snapshot_config.is_some() {
                    self.snapshot_archive_path_regex.is_match(path)
                } else {
                    false
                }
            }
        }
    }

    fn process_file_get(&self, path: &str) -> RequestMiddlewareAction {
        // Stuck on tokio 0.1 until the jsonrpc-http-server crate upgrades to tokio 0.2
        use tokio_01::prelude::*;

        let stem = path.split_at(1).1; // Drop leading '/' from path
        let filename = {
            match path {
                "/genesis.tar.bz2" => self.ledger_path.join(stem),
                _ => self
                    .snapshot_config
                    .as_ref()
                    .unwrap()
                    .snapshot_package_output_path
                    .join(stem),
            }
        };

        info!("get {} -> {:?}", path, filename);

        RequestMiddlewareAction::Respond {
            should_validate_hosts: true,
            response: Box::new(
                tokio_fs_01::file::File::open(filename)
                    .and_then(|file| {
                        let buf: Vec<u8> = Vec::new();
                        tokio_io_01::io::read_to_end(file, buf)
                            .and_then(|item| Ok(hyper::Response::new(item.1.into())))
                            .or_else(|_| Ok(RpcRequestMiddleware::internal_server_error()))
                    })
                    .or_else(|_| Ok(RpcRequestMiddleware::not_found())),
            ),
        }
    }

    fn health_check(&self) -> &'static str {
        let response = match self.health.check() {
            RpcHealthStatus::Ok => "ok",
            RpcHealthStatus::Behind => "behind",
        };
        info!("health check: {}", response);
        response
    }
}

impl RequestMiddleware for RpcRequestMiddleware {
    fn on_request(&self, request: hyper::Request<hyper::Body>) -> RequestMiddlewareAction {
        trace!("request uri: {}", request.uri());

        if let Some(ref snapshot_config) = self.snapshot_config {
            if request.uri().path() == "/snapshot.tar.bz2" {
                // Convenience redirect to the latest snapshot
                return RequestMiddlewareAction::Respond {
                    should_validate_hosts: true,
                    response: Box::new(jsonrpc_core::futures::future::ok(
                        if let Some((snapshot_archive, _)) =
                            snapshot_utils::get_highest_snapshot_archive_path(
                                &snapshot_config.snapshot_package_output_path,
                            )
                        {
                            RpcRequestMiddleware::redirect(&format!(
                                "/{}",
                                snapshot_archive
                                    .file_name()
                                    .unwrap_or_else(|| std::ffi::OsStr::new(""))
                                    .to_str()
                                    .unwrap_or(&"")
                            ))
                        } else {
                            RpcRequestMiddleware::not_found()
                        },
                    )),
                };
            }
        }

        if let Some(result) = process_rest(&self.bank_forks, request.uri().path()) {
            RequestMiddlewareAction::Respond {
                should_validate_hosts: true,
                response: Box::new(jsonrpc_core::futures::future::ok(
                    hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .body(hyper::Body::from(result))
                        .unwrap(),
                )),
            }
        } else if self.is_file_get_path(request.uri().path()) {
            self.process_file_get(request.uri().path())
        } else if request.uri().path() == "/health" {
            RequestMiddlewareAction::Respond {
                should_validate_hosts: true,
                response: Box::new(jsonrpc_core::futures::future::ok(
                    hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .body(hyper::Body::from(self.health_check()))
                        .unwrap(),
                )),
            }
        } else {
            RequestMiddlewareAction::Proceed {
                should_continue_on_invalid_cors: false,
                request,
            }
        }
    }
}

fn process_rest(bank_forks: &Arc<RwLock<BankForks>>, path: &str) -> Option<String> {
    match path {
        "/v0/circulating-supply" => {
            let r_bank_forks = bank_forks.read().unwrap();
            let bank = r_bank_forks.root_bank();
            let total_supply = bank.capitalization();
            let non_circulating_supply =
                crate::non_circulating_supply::calculate_non_circulating_supply(&bank).lamports;
            Some(format!(
                "{}",
                lamports_to_sol(total_supply - non_circulating_supply)
            ))
        }
        "/v0/total-supply" => {
            let r_bank_forks = bank_forks.read().unwrap();
            let bank = r_bank_forks.root_bank();
            let total_supply = bank.capitalization();
            Some(format!("{}", lamports_to_sol(total_supply)))
        }
        _ => None,
    }
}

impl JsonRpcService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        rpc_addr: SocketAddr,
        config: JsonRpcConfig,
        snapshot_config: Option<SnapshotConfig>,
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        blockstore: Arc<Blockstore>,
        cluster_info: Arc<ClusterInfo>,
        poh_recorder: Option<Arc<Mutex<PohRecorder>>>,
        genesis_hash: Hash,
        ledger_path: &Path,
        validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
        trusted_validators: Option<HashSet<Pubkey>>,
        override_health_check: Arc<AtomicBool>,
    ) -> Self {
        info!("rpc bound to {:?}", rpc_addr);
        info!("rpc configuration: {:?}", config);

        let health = Arc::new(RpcHealth::new(
            cluster_info.clone(),
            trusted_validators,
            config.health_check_slot_distance,
            override_health_check,
        ));

        let tpu_address = cluster_info.my_contact_info().tpu;
        let mut runtime = runtime::Builder::new()
            .threaded_scheduler()
            .thread_name("rpc-runtime")
            .enable_all()
            .build()
            .expect("Runtime");

        let exit_bigtable_ledger_upload_service = Arc::new(AtomicBool::new(false));

        let (bigtable_ledger_storage, _bigtable_ledger_upload_service) =
            if config.enable_bigtable_ledger_storage || config.enable_bigtable_ledger_upload {
                runtime
                    .block_on(solana_storage_bigtable::LedgerStorage::new(
                        !config.enable_bigtable_ledger_upload,
                    ))
                    .map(|bigtable_ledger_storage| {
                        info!("BigTable ledger storage initialized");

                        let bigtable_ledger_upload_service = Arc::new(BigTableUploadService::new(
                            runtime.handle().clone(),
                            bigtable_ledger_storage.clone(),
                            blockstore.clone(),
                            block_commitment_cache.clone(),
                            exit_bigtable_ledger_upload_service.clone(),
                        ));

                        (
                            Some(bigtable_ledger_storage),
                            Some(bigtable_ledger_upload_service),
                        )
                    })
                    .unwrap_or_else(|err| {
                        error!("Failed to initialize BigTable ledger storage: {:?}", err);
                        (None, None)
                    })
            } else {
                (None, None)
            };

        let (request_processor, receiver) = JsonRpcRequestProcessor::new(
            config,
            bank_forks.clone(),
            block_commitment_cache,
            blockstore,
            validator_exit.clone(),
            health.clone(),
            cluster_info.clone(),
            genesis_hash,
            &runtime,
            bigtable_ledger_storage,
        );

        let exit_send_transaction_service = Arc::new(AtomicBool::new(false));
        let leader_info =
            poh_recorder.map(|recorder| LeaderInfo::new(cluster_info.clone(), recorder));
        let _send_transaction_service = Arc::new(SendTransactionService::new(
            tpu_address,
            &bank_forks,
            leader_info,
            &exit_send_transaction_service,
            receiver,
        ));

        #[cfg(test)]
        let test_request_processor = request_processor.clone();

        let ledger_path = ledger_path.to_path_buf();

        let (close_handle_sender, close_handle_receiver) = channel();
        let thread_hdl = Builder::new()
            .name("solana-jsonrpc".to_string())
            .spawn(move || {
                let mut io = MetaIoHandler::default();
                let rpc = RpcSolImpl;
                io.extend_with(rpc.to_delegate());

                let request_middleware = RpcRequestMiddleware::new(
                    ledger_path,
                    snapshot_config,
                    bank_forks.clone(),
                    health.clone(),
                );
                let server = ServerBuilder::with_meta_extractor(
                    io,
                    move |_req: &hyper::Request<hyper::Body>| request_processor.clone(),
                )
                .threads(num_cpus::get())
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .cors_max_age(86400)
                .request_middleware(request_middleware)
                .start_http(&rpc_addr);

                if let Err(e) = server {
                    warn!(
                        "JSON RPC service unavailable error: {:?}. \n\
                           Also, check that port {} is not already in use by another application",
                        e,
                        rpc_addr.port()
                    );
                    return;
                }

                let server = server.unwrap();
                close_handle_sender.send(server.close_handle()).unwrap();
                server.wait();
                exit_send_transaction_service.store(true, Ordering::Relaxed);
                exit_bigtable_ledger_upload_service.store(true, Ordering::Relaxed);
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
            runtime,
            #[cfg(test)]
            request_processor: test_request_processor,
            close_handle: Some(close_handle),
        }
    }

    pub fn exit(&mut self) {
        if let Some(c) = self.close_handle.take() {
            c.close()
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.runtime.shutdown_background();
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        crds_value::{CrdsData, CrdsValue, SnapshotHash},
        rpc::create_validator_exit,
    };
    use solana_ledger::{
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
    };
    use solana_runtime::{
        bank::Bank, bank_forks::CompressionType, snapshot_utils::SnapshotVersion,
    };
    use solana_sdk::{genesis_config::ClusterType, signature::Signer};
    use std::net::{IpAddr, Ipv4Addr};

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
        let cluster_info = Arc::new(ClusterInfo::default());
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let rpc_addr = SocketAddr::new(
            ip_addr,
            solana_net_utils::find_available_port_in_range(ip_addr, (10000, 65535)).unwrap(),
        );
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank)));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let mut rpc_service = JsonRpcService::new(
            rpc_addr,
            JsonRpcConfig::default(),
            None,
            bank_forks,
            block_commitment_cache,
            blockstore,
            cluster_info,
            None,
            Hash::default(),
            &PathBuf::from("farf"),
            validator_exit,
            None,
            Arc::new(AtomicBool::new(false)),
        );
        let thread = rpc_service.thread_hdl.thread();
        assert_eq!(thread.name().unwrap(), "solana-jsonrpc");

        assert_eq!(
            10_000,
            rpc_service
                .request_processor
                .get_balance(&mint_keypair.pubkey(), None)
                .value
        );
        rpc_service.exit();
        rpc_service.join().unwrap();
    }

    fn create_bank_forks() -> Arc<RwLock<BankForks>> {
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(10_000);
        genesis_config.cluster_type = ClusterType::MainnetBeta;
        let bank = Bank::new(&genesis_config);
        Arc::new(RwLock::new(BankForks::new(bank)))
    }

    #[test]
    fn test_process_rest_api() {
        let bank_forks = create_bank_forks();

        assert_eq!(None, process_rest(&bank_forks, "not-a-supported-rest-api"));
        assert_eq!(
            Some("0.000010127".to_string()),
            process_rest(&bank_forks, "/v0/circulating-supply")
        );
        assert_eq!(
            Some("0.000010127".to_string()),
            process_rest(&bank_forks, "/v0/total-supply")
        );
    }

    #[test]
    fn test_is_file_get_path() {
        let bank_forks = create_bank_forks();
        let rrm = RpcRequestMiddleware::new(
            PathBuf::from("/"),
            None,
            bank_forks.clone(),
            RpcHealth::stub(),
        );
        let rrm_with_snapshot_config = RpcRequestMiddleware::new(
            PathBuf::from("/"),
            Some(SnapshotConfig {
                snapshot_interval_slots: 0,
                snapshot_package_output_path: PathBuf::from("/"),
                snapshot_path: PathBuf::from("/"),
                compression: CompressionType::Bzip2,
                snapshot_version: SnapshotVersion::default(),
            }),
            bank_forks,
            RpcHealth::stub(),
        );

        assert!(rrm.is_file_get_path("/genesis.tar.bz2"));
        assert!(!rrm.is_file_get_path("genesis.tar.bz2"));

        assert!(!rrm.is_file_get_path("/snapshot.tar.bz2")); // This is a redirect

        assert!(!rrm.is_file_get_path(
            "/snapshot-100-AvFf9oS8A8U78HdjT9YG2sTTThLHJZmhaMn2g8vkWYnr.tar.bz2"
        ));
        assert!(rrm_with_snapshot_config.is_file_get_path(
            "/snapshot-100-AvFf9oS8A8U78HdjT9YG2sTTThLHJZmhaMn2g8vkWYnr.tar.bz2"
        ));

        assert!(!rrm.is_file_get_path(
            "/snapshot-notaslotnumber-AvFf9oS8A8U78HdjT9YG2sTTThLHJZmhaMn2g8vkWYnr.tar.bz2"
        ));

        assert!(!rrm.is_file_get_path("/"));
        assert!(!rrm.is_file_get_path(".."));
        assert!(!rrm.is_file_get_path("🎣"));
    }

    #[test]
    fn test_health_check_with_no_trusted_validators() {
        let rm = RpcRequestMiddleware::new(
            PathBuf::from("/"),
            None,
            create_bank_forks(),
            RpcHealth::stub(),
        );
        assert_eq!(rm.health_check(), "ok");
    }

    #[test]
    fn test_health_check_with_trusted_validators() {
        let cluster_info = Arc::new(ClusterInfo::default());
        let health_check_slot_distance = 123;
        let override_health_check = Arc::new(AtomicBool::new(false));
        let trusted_validators = vec![Pubkey::new_rand(), Pubkey::new_rand(), Pubkey::new_rand()];

        let health = Arc::new(RpcHealth::new(
            cluster_info.clone(),
            Some(trusted_validators.clone().into_iter().collect()),
            health_check_slot_distance,
            override_health_check.clone(),
        ));

        let rm = RpcRequestMiddleware::new(PathBuf::from("/"), None, create_bank_forks(), health);

        // No account hashes for this node or any trusted validators == "behind"
        assert_eq!(rm.health_check(), "behind");

        // No account hashes for any trusted validators == "behind"
        cluster_info.push_accounts_hashes(vec![(1000, Hash::default()), (900, Hash::default())]);
        assert_eq!(rm.health_check(), "behind");
        override_health_check.store(true, Ordering::Relaxed);
        assert_eq!(rm.health_check(), "ok");
        override_health_check.store(false, Ordering::Relaxed);

        // This node is ahead of the trusted validators == "ok"
        cluster_info
            .gossip
            .write()
            .unwrap()
            .crds
            .insert(
                CrdsValue::new_unsigned(CrdsData::AccountsHashes(SnapshotHash::new(
                    trusted_validators[0],
                    vec![
                        (1, Hash::default()),
                        (1001, Hash::default()),
                        (2, Hash::default()),
                    ],
                ))),
                1,
            )
            .unwrap();
        assert_eq!(rm.health_check(), "ok");

        // Node is slightly behind the trusted validators == "ok"
        cluster_info
            .gossip
            .write()
            .unwrap()
            .crds
            .insert(
                CrdsValue::new_unsigned(CrdsData::AccountsHashes(SnapshotHash::new(
                    trusted_validators[1],
                    vec![(1000 + health_check_slot_distance - 1, Hash::default())],
                ))),
                1,
            )
            .unwrap();
        assert_eq!(rm.health_check(), "ok");

        // Node is far behind the trusted validators == "behind"
        cluster_info
            .gossip
            .write()
            .unwrap()
            .crds
            .insert(
                CrdsValue::new_unsigned(CrdsData::AccountsHashes(SnapshotHash::new(
                    trusted_validators[2],
                    vec![(1000 + health_check_slot_distance, Hash::default())],
                ))),
                1,
            )
            .unwrap();
        assert_eq!(rm.health_check(), "behind");
    }
}
