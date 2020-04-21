//! The `rpc_service` module implements the Solana JSON RPC service.

use crate::{
    cluster_info::ClusterInfo, commitment::BlockCommitmentCache, rpc::*,
    storage_stage::StorageState, validator::ValidatorExit,
};
use jsonrpc_core::MetaIoHandler;
use jsonrpc_http_server::{
    hyper, AccessControlAllowOrigin, CloseHandle, DomainsValidation, RequestMiddleware,
    RequestMiddlewareAction, ServerBuilder,
};
use regex::Regex;
use solana_ledger::{
    bank_forks::{BankForks, SnapshotConfig},
    blockstore::Blockstore,
    snapshot_utils,
};
use solana_sdk::{hash::Hash, pubkey::Pubkey};
use std::{
    collections::HashSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, RwLock},
    thread::{self, Builder, JoinHandle},
};
use tokio::prelude::Future;

// If trusted validators are specified, consider this validator healthy if its latest account hash
// is no further behind than this distance from the latest trusted validator account hash
const HEALTH_CHECK_SLOT_DISTANCE: u64 = 150;

pub struct JsonRpcService {
    thread_hdl: JoinHandle<()>,

    #[cfg(test)]
    pub request_processor: Arc<RwLock<JsonRpcRequestProcessor>>, // Used only by test_rpc_new()...

    close_handle: Option<CloseHandle>,
}

struct RpcRequestMiddleware {
    ledger_path: PathBuf,
    snapshot_archive_path_regex: Regex,
    snapshot_config: Option<SnapshotConfig>,
    cluster_info: Arc<ClusterInfo>,
    trusted_validators: Option<HashSet<Pubkey>>,
}

impl RpcRequestMiddleware {
    pub fn new(
        ledger_path: PathBuf,
        snapshot_config: Option<SnapshotConfig>,
        cluster_info: Arc<ClusterInfo>,
        trusted_validators: Option<HashSet<Pubkey>>,
    ) -> Self {
        Self {
            ledger_path,
            snapshot_archive_path_regex: Regex::new(
                r"/snapshot-\d+-[[:alnum:]]+\.tar\.(bz2|zst|gz)$",
            )
            .unwrap(),
            snapshot_config,
            cluster_info,
            trusted_validators,
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

    fn is_get_path(&self, path: &str) -> bool {
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

    fn get(&self, path: &str) -> RequestMiddlewareAction {
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

    fn health_check(&self) -> &'static str {
        let response = if let Some(trusted_validators) = &self.trusted_validators {
            let (latest_account_hash_slot, latest_trusted_validator_account_hash_slot) = {
                (
                    self.cluster_info
                        .get_accounts_hash_for_node(&self.cluster_info.id(), |hashes| {
                            hashes
                                .iter()
                                .max_by(|a, b| a.0.cmp(&b.0))
                                .map(|slot_hash| slot_hash.0)
                        })
                        .flatten()
                        .unwrap_or(0),
                    trusted_validators
                        .iter()
                        .map(|trusted_validator| {
                            self.cluster_info
                                .get_accounts_hash_for_node(&trusted_validator, |hashes| {
                                    hashes
                                        .iter()
                                        .max_by(|a, b| a.0.cmp(&b.0))
                                        .map(|slot_hash| slot_hash.0)
                                })
                                .flatten()
                                .unwrap_or(0)
                        })
                        .max()
                        .unwrap_or(0),
                )
            };

            // This validator is considered healthy if its latest account hash slot is within
            // `HEALTH_CHECK_SLOT_DISTANCE` of the latest trusted validator's account hash slot
            if latest_account_hash_slot > 0
                && latest_trusted_validator_account_hash_slot > 0
                && latest_account_hash_slot
                    > latest_trusted_validator_account_hash_slot
                        .saturating_sub(HEALTH_CHECK_SLOT_DISTANCE)
            {
                "ok"
            } else {
                warn!(
                    "health check: me={}, latest trusted_validator={}",
                    latest_account_hash_slot, latest_trusted_validator_account_hash_slot
                );
                "behind"
            }
        } else {
            // No trusted validator point of reference available, so this validator is healthy
            // because it's running
            "ok"
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
        if self.is_get_path(request.uri().path()) {
            self.get(request.uri().path())
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
        genesis_hash: Hash,
        ledger_path: &Path,
        storage_state: StorageState,
        validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
        trusted_validators: Option<HashSet<Pubkey>>,
    ) -> Self {
        info!("rpc bound to {:?}", rpc_addr);
        info!("rpc configuration: {:?}", config);
        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            config,
            bank_forks,
            block_commitment_cache,
            blockstore,
            storage_state,
            validator_exit.clone(),
        )));

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
                    cluster_info.clone(),
                    trusted_validators,
                );
                let server = ServerBuilder::with_meta_extractor(
                    io,
                    move |_req: &hyper::Request<hyper::Body>| Meta {
                        request_processor: request_processor.clone(),
                        cluster_info: cluster_info.clone(),
                        genesis_hash,
                    },
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
        self.thread_hdl.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contact_info::ContactInfo,
        crds_value::{CrdsData, CrdsValue, SnapshotHash},
        rpc::tests::create_validator_exit,
    };
    use solana_ledger::{
        bank_forks::CompressionType,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
    };
    use solana_runtime::bank::Bank;
    use solana_sdk::signature::Signer;
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
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default()));
        let ip_addr = IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0));
        let rpc_addr = SocketAddr::new(
            ip_addr,
            solana_net_utils::find_available_port_in_range(ip_addr, (10000, 65535)).unwrap(),
        );
        let bank_forks = Arc::new(RwLock::new(BankForks::new(bank.slot(), bank)));
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let mut rpc_service = JsonRpcService::new(
            rpc_addr,
            JsonRpcConfig::default(),
            None,
            bank_forks,
            block_commitment_cache,
            Arc::new(blockstore),
            cluster_info,
            Hash::default(),
            &PathBuf::from("farf"),
            StorageState::default(),
            validator_exit,
            None,
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

    #[test]
    fn test_is_get_path() {
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default()));

        let rrm = RpcRequestMiddleware::new(PathBuf::from("/"), None, cluster_info.clone(), None);
        let rrm_with_snapshot_config = RpcRequestMiddleware::new(
            PathBuf::from("/"),
            Some(SnapshotConfig {
                snapshot_interval_slots: 0,
                snapshot_package_output_path: PathBuf::from("/"),
                snapshot_path: PathBuf::from("/"),
                compression: CompressionType::Bzip2,
            }),
            cluster_info,
            None,
        );

        assert!(rrm.is_get_path("/genesis.tar.bz2"));
        assert!(!rrm.is_get_path("genesis.tar.bz2"));

        assert!(!rrm.is_get_path("/snapshot.tar.bz2")); // This is a redirect

        assert!(
            !rrm.is_get_path("/snapshot-100-AvFf9oS8A8U78HdjT9YG2sTTThLHJZmhaMn2g8vkWYnr.tar.bz2")
        );
        assert!(rrm_with_snapshot_config
            .is_get_path("/snapshot-100-AvFf9oS8A8U78HdjT9YG2sTTThLHJZmhaMn2g8vkWYnr.tar.bz2"));

        assert!(!rrm.is_get_path(
            "/snapshot-notaslotnumber-AvFf9oS8A8U78HdjT9YG2sTTThLHJZmhaMn2g8vkWYnr.tar.bz2"
        ));

        assert!(!rrm.is_get_path("/"));
        assert!(!rrm.is_get_path(".."));
        assert!(!rrm.is_get_path("🎣"));
    }

    #[test]
    fn test_health_check_with_no_trusted_validators() {
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default()));

        let rm = RpcRequestMiddleware::new(PathBuf::from("/"), None, cluster_info.clone(), None);
        assert_eq!(rm.health_check(), "ok");
    }

    #[test]
    fn test_health_check_with_trusted_validators() {
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default()));

        let trusted_validators = vec![Pubkey::new_rand(), Pubkey::new_rand(), Pubkey::new_rand()];
        let rm = RpcRequestMiddleware::new(
            PathBuf::from("/"),
            None,
            cluster_info.clone(),
            Some(trusted_validators.clone().into_iter().collect()),
        );

        // No account hashes for this node or any trusted validators == "behind"
        assert_eq!(rm.health_check(), "behind");

        // No account hashes for any trusted validators == "behind"
        cluster_info.push_accounts_hashes(vec![(1000, Hash::default()), (900, Hash::default())]);
        assert_eq!(rm.health_check(), "behind");

        // This node is ahead of the trusted validators == "ok"
        cluster_info
            .gossip
            .write()
            .unwrap()
            .crds
            .insert(
                CrdsValue::new_unsigned(CrdsData::AccountsHashes(SnapshotHash::new(
                    trusted_validators[0].clone(),
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
                    trusted_validators[1].clone(),
                    vec![(1000 + HEALTH_CHECK_SLOT_DISTANCE - 1, Hash::default())],
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
                    trusted_validators[2].clone(),
                    vec![(1000 + HEALTH_CHECK_SLOT_DISTANCE, Hash::default())],
                ))),
                1,
            )
            .unwrap();
        assert_eq!(rm.health_check(), "behind");
    }
}
