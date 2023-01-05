use {
    jsonrpc_core::{MetaIoHandler, Metadata, Result},
    jsonrpc_core_client::{transports::ipc, RpcError},
    jsonrpc_derive::rpc,
    jsonrpc_ipc_server::{RequestContext, ServerBuilder},
    jsonrpc_server_utils::tokio,
    log::*,
    serde::{de::Deserializer, Deserialize, Serialize},
    solana_core::{
        consensus::Tower, tower_storage::TowerStorage, validator::ValidatorStartProgress,
    },
    solana_gossip::{cluster_info::ClusterInfo, contact_info::ContactInfo},
    solana_rpc::rpc::verify_pubkey,
    solana_rpc_client_api::{config::RpcAccountIndex, custom_error::RpcCustomError},
    solana_runtime::{accounts_index::AccountIndex, bank_forks::BankForks},
    solana_sdk::{
        exit::Exit,
        pubkey::Pubkey,
        signature::{read_keypair_file, Keypair, Signer},
    },
    std::{
        collections::{HashMap, HashSet},
        error,
        fmt::{self, Display},
        net::SocketAddr,
        path::{Path, PathBuf},
        sync::{Arc, RwLock},
        thread::{self, Builder},
        time::{Duration, SystemTime},
    },
};

#[derive(Clone)]
pub struct AdminRpcRequestMetadataPostInit {
    pub cluster_info: Arc<ClusterInfo>,
    pub bank_forks: Arc<RwLock<BankForks>>,
    pub vote_account: Pubkey,
    pub repair_whitelist: Arc<RwLock<HashSet<Pubkey>>>,
}

#[derive(Clone)]
pub struct AdminRpcRequestMetadata {
    pub rpc_addr: Option<SocketAddr>,
    pub start_time: SystemTime,
    pub start_progress: Arc<RwLock<ValidatorStartProgress>>,
    pub validator_exit: Arc<RwLock<Exit>>,
    pub authorized_voter_keypairs: Arc<RwLock<Vec<Arc<Keypair>>>>,
    pub tower_storage: Arc<dyn TowerStorage>,
    pub staked_nodes_overrides: Arc<RwLock<HashMap<Pubkey, u64>>>,
    pub post_init: Arc<RwLock<Option<AdminRpcRequestMetadataPostInit>>>,
}
impl Metadata for AdminRpcRequestMetadata {}

impl AdminRpcRequestMetadata {
    fn with_post_init<F, R>(&self, func: F) -> Result<R>
    where
        F: FnOnce(&AdminRpcRequestMetadataPostInit) -> Result<R>,
    {
        if let Some(post_init) = self.post_init.read().unwrap().as_ref() {
            func(post_init)
        } else {
            Err(jsonrpc_core::error::Error::invalid_params(
                "Retry once validator start up is complete",
            ))
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdminRpcContactInfo {
    pub id: String,
    pub gossip: SocketAddr,
    pub tvu: SocketAddr,
    pub tvu_forwards: SocketAddr,
    pub repair: SocketAddr,
    pub tpu: SocketAddr,
    pub tpu_forwards: SocketAddr,
    pub tpu_vote: SocketAddr,
    pub rpc: SocketAddr,
    pub rpc_pubsub: SocketAddr,
    pub serve_repair: SocketAddr,
    pub last_updated_timestamp: u64,
    pub shred_version: u16,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AdminRpcRepairWhitelist {
    pub whitelist: Vec<Pubkey>,
}

impl From<ContactInfo> for AdminRpcContactInfo {
    fn from(contact_info: ContactInfo) -> Self {
        let ContactInfo {
            id,
            gossip,
            tvu,
            tvu_forwards,
            repair,
            tpu,
            tpu_forwards,
            tpu_vote,
            rpc,
            rpc_pubsub,
            serve_repair,
            wallclock,
            shred_version,
        } = contact_info;
        Self {
            id: id.to_string(),
            last_updated_timestamp: wallclock,
            gossip,
            tvu,
            tvu_forwards,
            repair,
            tpu,
            tpu_forwards,
            tpu_vote,
            rpc,
            rpc_pubsub,
            serve_repair,
            shred_version,
        }
    }
}

impl Display for AdminRpcContactInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Identity: {}", self.id)?;
        writeln!(f, "Gossip: {}", self.gossip)?;
        writeln!(f, "TVU: {}", self.tvu)?;
        writeln!(f, "TVU Forwards: {}", self.tvu_forwards)?;
        writeln!(f, "Repair: {}", self.repair)?;
        writeln!(f, "TPU: {}", self.tpu)?;
        writeln!(f, "TPU Forwards: {}", self.tpu_forwards)?;
        writeln!(f, "TPU Votes: {}", self.tpu_vote)?;
        writeln!(f, "RPC: {}", self.rpc)?;
        writeln!(f, "RPC Pubsub: {}", self.rpc_pubsub)?;
        writeln!(f, "Serve Repair: {}", self.serve_repair)?;
        writeln!(f, "Last Updated Timestamp: {}", self.last_updated_timestamp)?;
        writeln!(f, "Shred Version: {}", self.shred_version)
    }
}

impl Display for AdminRpcRepairWhitelist {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Repair whitelist: {:?}", &self.whitelist)
    }
}

#[rpc]
pub trait AdminRpc {
    type Metadata;

    #[rpc(meta, name = "exit")]
    fn exit(&self, meta: Self::Metadata) -> Result<()>;

    #[rpc(meta, name = "rpcAddress")]
    fn rpc_addr(&self, meta: Self::Metadata) -> Result<Option<SocketAddr>>;

    #[rpc(name = "setLogFilter")]
    fn set_log_filter(&self, filter: String) -> Result<()>;

    #[rpc(meta, name = "startTime")]
    fn start_time(&self, meta: Self::Metadata) -> Result<SystemTime>;

    #[rpc(meta, name = "startProgress")]
    fn start_progress(&self, meta: Self::Metadata) -> Result<ValidatorStartProgress>;

    #[rpc(meta, name = "addAuthorizedVoter")]
    fn add_authorized_voter(&self, meta: Self::Metadata, keypair_file: String) -> Result<()>;

    #[rpc(meta, name = "addAuthorizedVoterFromBytes")]
    fn add_authorized_voter_from_bytes(&self, meta: Self::Metadata, keypair: Vec<u8>)
        -> Result<()>;

    #[rpc(meta, name = "removeAllAuthorizedVoters")]
    fn remove_all_authorized_voters(&self, meta: Self::Metadata) -> Result<()>;

    #[rpc(meta, name = "setIdentity")]
    fn set_identity(
        &self,
        meta: Self::Metadata,
        keypair_file: String,
        require_tower: bool,
    ) -> Result<()>;

    #[rpc(meta, name = "setIdentityFromBytes")]
    fn set_identity_from_bytes(
        &self,
        meta: Self::Metadata,
        identity_keypair: Vec<u8>,
        require_tower: bool,
    ) -> Result<()>;

    #[rpc(meta, name = "setStakedNodesOverrides")]
    fn set_staked_nodes_overrides(&self, meta: Self::Metadata, path: String) -> Result<()>;

    #[rpc(meta, name = "contactInfo")]
    fn contact_info(&self, meta: Self::Metadata) -> Result<AdminRpcContactInfo>;

    #[rpc(meta, name = "repairWhitelist")]
    fn repair_whitelist(&self, meta: Self::Metadata) -> Result<AdminRpcRepairWhitelist>;

    #[rpc(meta, name = "setRepairWhitelist")]
    fn set_repair_whitelist(&self, meta: Self::Metadata, whitelist: Vec<Pubkey>) -> Result<()>;

    #[rpc(meta, name = "getSecondaryIndexKeySize")]
    fn get_secondary_index_key_size(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
    ) -> Result<HashMap<RpcAccountIndex, usize>>;
}

pub struct AdminRpcImpl;
impl AdminRpc for AdminRpcImpl {
    type Metadata = AdminRpcRequestMetadata;

    fn exit(&self, meta: Self::Metadata) -> Result<()> {
        debug!("exit admin rpc request received");

        thread::Builder::new()
            .name("solProcessExit".into())
            .spawn(move || {
                // Delay exit signal until this RPC request completes, otherwise the caller of `exit` might
                // receive a confusing error as the validator shuts down before a response is sent back.
                thread::sleep(Duration::from_millis(100));

                warn!("validator exit requested");
                meta.validator_exit.write().unwrap().exit();

                // TODO: Debug why Exit doesn't always cause the validator to fully exit
                // (rocksdb background processing or some other stuck thread perhaps?).
                //
                // If the process is still alive after five seconds, exit harder
                thread::sleep(Duration::from_secs(5));
                warn!("validator exit timeout");
                std::process::exit(0);
            })
            .unwrap();
        Ok(())
    }

    fn rpc_addr(&self, meta: Self::Metadata) -> Result<Option<SocketAddr>> {
        debug!("rpc_addr admin rpc request received");
        Ok(meta.rpc_addr)
    }

    fn set_log_filter(&self, filter: String) -> Result<()> {
        debug!("set_log_filter admin rpc request received");
        solana_logger::setup_with(&filter);
        Ok(())
    }

    fn start_time(&self, meta: Self::Metadata) -> Result<SystemTime> {
        debug!("start_time admin rpc request received");
        Ok(meta.start_time)
    }

    fn start_progress(&self, meta: Self::Metadata) -> Result<ValidatorStartProgress> {
        debug!("start_progress admin rpc request received");
        Ok(*meta.start_progress.read().unwrap())
    }

    fn add_authorized_voter(&self, meta: Self::Metadata, keypair_file: String) -> Result<()> {
        debug!("add_authorized_voter request received");

        let authorized_voter = read_keypair_file(keypair_file)
            .map_err(|err| jsonrpc_core::error::Error::invalid_params(format!("{err}")))?;

        AdminRpcImpl::add_authorized_voter_keypair(meta, authorized_voter)
    }

    fn add_authorized_voter_from_bytes(
        &self,
        meta: Self::Metadata,
        keypair: Vec<u8>,
    ) -> Result<()> {
        debug!("add_authorized_voter_from_bytes request received");

        let authorized_voter = Keypair::from_bytes(&keypair).map_err(|err| {
            jsonrpc_core::error::Error::invalid_params(format!(
                "Failed to read authorized voter keypair from provided byte array: {err}"
            ))
        })?;

        AdminRpcImpl::add_authorized_voter_keypair(meta, authorized_voter)
    }

    fn remove_all_authorized_voters(&self, meta: Self::Metadata) -> Result<()> {
        debug!("remove_all_authorized_voters received");
        meta.authorized_voter_keypairs.write().unwrap().clear();
        Ok(())
    }

    fn set_identity(
        &self,
        meta: Self::Metadata,
        keypair_file: String,
        require_tower: bool,
    ) -> Result<()> {
        debug!("set_identity request received");

        let identity_keypair = read_keypair_file(&keypair_file).map_err(|err| {
            jsonrpc_core::error::Error::invalid_params(format!(
                "Failed to read identity keypair from {keypair_file}: {err}"
            ))
        })?;

        AdminRpcImpl::set_identity_keypair(meta, identity_keypair, require_tower)
    }

    fn set_identity_from_bytes(
        &self,
        meta: Self::Metadata,
        identity_keypair: Vec<u8>,
        require_tower: bool,
    ) -> Result<()> {
        debug!("set_identity_from_bytes request received");

        let identity_keypair = Keypair::from_bytes(&identity_keypair).map_err(|err| {
            jsonrpc_core::error::Error::invalid_params(format!(
                "Failed to read identity keypair from provided byte array: {err}"
            ))
        })?;

        AdminRpcImpl::set_identity_keypair(meta, identity_keypair, require_tower)
    }

    fn set_staked_nodes_overrides(&self, meta: Self::Metadata, path: String) -> Result<()> {
        let loaded_config = load_staked_nodes_overrides(&path)
            .map_err(|err| {
                error!(
                    "Failed to load staked nodes overrides from {}: {}",
                    &path, err
                );
                jsonrpc_core::error::Error::internal_error()
            })?
            .staked_map_id;
        let mut write_staked_nodes = meta.staked_nodes_overrides.write().unwrap();
        write_staked_nodes.clear();
        write_staked_nodes.extend(loaded_config.into_iter());
        info!("Staked nodes overrides loaded from {}", path);
        debug!("overrides map: {:?}", write_staked_nodes);
        Ok(())
    }

    fn contact_info(&self, meta: Self::Metadata) -> Result<AdminRpcContactInfo> {
        meta.with_post_init(|post_init| Ok(post_init.cluster_info.my_contact_info().into()))
    }

    fn repair_whitelist(&self, meta: Self::Metadata) -> Result<AdminRpcRepairWhitelist> {
        debug!("repair_whitelist request received");

        meta.with_post_init(|post_init| {
            let whitelist: Vec<_> = post_init
                .repair_whitelist
                .read()
                .unwrap()
                .iter()
                .copied()
                .collect();
            Ok(AdminRpcRepairWhitelist { whitelist })
        })
    }

    fn set_repair_whitelist(&self, meta: Self::Metadata, whitelist: Vec<Pubkey>) -> Result<()> {
        debug!("set_repair_whitelist request received");

        let whitelist: HashSet<Pubkey> = whitelist.into_iter().collect();
        meta.with_post_init(|post_init| {
            *post_init.repair_whitelist.write().unwrap() = whitelist;
            warn!(
                "Repair whitelist set to {:?}",
                &post_init.repair_whitelist.read().unwrap()
            );
            Ok(())
        })
    }

    fn get_secondary_index_key_size(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
    ) -> Result<HashMap<RpcAccountIndex, usize>> {
        debug!(
            "get_secondary_index_key_size rpc request received: {:?}",
            pubkey_str
        );
        let index_key = verify_pubkey(&pubkey_str)?;
        meta.with_post_init(|post_init| {
            let bank = post_init.bank_forks.read().unwrap().root_bank();

            // Take ref to enabled AccountSecondaryIndexes
            let enabled_account_indexes = &bank.accounts().accounts_db.account_indexes;

            // Exit if secondary indexes are not enabled
            if enabled_account_indexes.is_empty() {
                debug!("get_secondary_index_key_size: secondary index not enabled.");
                return Ok(HashMap::new());
            };

            // Make sure the requested key is not explicitly excluded
            if !enabled_account_indexes.include_key(&index_key) {
                return Err(RpcCustomError::KeyExcludedFromSecondaryIndex {
                    index_key: index_key.to_string(),
                }
                .into());
            }

            // Grab a ref to the AccountsDbfor this Bank
            let accounts_index = &bank.accounts().accounts_db.accounts_index;

            // Find the size of the key in every index where it exists
            let found_sizes = enabled_account_indexes
                .indexes
                .iter()
                .filter_map(|index| {
                    accounts_index
                        .get_index_key_size(index, &index_key)
                        .map(|size| (rpc_account_index_from_account_index(index), size))
                })
                .collect::<HashMap<_, _>>();

            // Note: Will return an empty HashMap if no keys are found.
            if found_sizes.is_empty() {
                debug!("get_secondary_index_key_size: key not found in the secondary index.");
            }
            Ok(found_sizes)
        })
    }
}

impl AdminRpcImpl {
    fn add_authorized_voter_keypair(
        meta: AdminRpcRequestMetadata,
        authorized_voter: Keypair,
    ) -> Result<()> {
        let mut authorized_voter_keypairs = meta.authorized_voter_keypairs.write().unwrap();

        if authorized_voter_keypairs
            .iter()
            .any(|x| x.pubkey() == authorized_voter.pubkey())
        {
            Err(jsonrpc_core::error::Error::invalid_params(
                "Authorized voter already present",
            ))
        } else {
            authorized_voter_keypairs.push(Arc::new(authorized_voter));
            Ok(())
        }
    }

    fn set_identity_keypair(
        meta: AdminRpcRequestMetadata,
        identity_keypair: Keypair,
        require_tower: bool,
    ) -> Result<()> {
        meta.with_post_init(|post_init| {
            if require_tower {
                let _ = Tower::restore(meta.tower_storage.as_ref(), &identity_keypair.pubkey())
                    .map_err(|err| {
                        jsonrpc_core::error::Error::invalid_params(format!(
                            "Unable to load tower file for identity {}: {}",
                            identity_keypair.pubkey(),
                            err
                        ))
                    })?;
            }

            solana_metrics::set_host_id(identity_keypair.pubkey().to_string());
            post_init
                .cluster_info
                .set_keypair(Arc::new(identity_keypair));
            warn!("Identity set to {}", post_init.cluster_info.id());
            Ok(())
        })
    }
}

fn rpc_account_index_from_account_index(account_index: &AccountIndex) -> RpcAccountIndex {
    match account_index {
        AccountIndex::ProgramId => RpcAccountIndex::ProgramId,
        AccountIndex::SplTokenOwner => RpcAccountIndex::SplTokenOwner,
        AccountIndex::SplTokenMint => RpcAccountIndex::SplTokenMint,
    }
}

// Start the Admin RPC interface
pub fn run(ledger_path: &Path, metadata: AdminRpcRequestMetadata) {
    let admin_rpc_path = admin_rpc_path(ledger_path);

    let event_loop = tokio::runtime::Builder::new_multi_thread()
        .thread_name("solAdminRpcEl")
        .worker_threads(3) // Three still seems like a lot, and better than the default of available core count
        .enable_all()
        .build()
        .unwrap();

    Builder::new()
        .name("solAdminRpc".to_string())
        .spawn(move || {
            let mut io = MetaIoHandler::default();
            io.extend_with(AdminRpcImpl.to_delegate());

            let validator_exit = metadata.validator_exit.clone();
            let server = ServerBuilder::with_meta_extractor(io, move |_req: &RequestContext| {
                metadata.clone()
            })
            .event_loop_executor(event_loop.handle().clone())
            .start(&format!("{}", admin_rpc_path.display()));

            match server {
                Err(err) => {
                    warn!("Unable to start admin rpc service: {:?}", err);
                }
                Ok(server) => {
                    let close_handle = server.close_handle();
                    validator_exit
                        .write()
                        .unwrap()
                        .register_exit(Box::new(move || {
                            close_handle.close();
                        }));

                    server.wait();
                }
            }
        })
        .unwrap();
}

fn admin_rpc_path(ledger_path: &Path) -> PathBuf {
    #[cfg(target_family = "windows")]
    {
        // More information about the wackiness of pipe names over at
        // https://docs.microsoft.com/en-us/windows/win32/ipc/pipe-names
        if let Some(ledger_filename) = ledger_path.file_name() {
            PathBuf::from(format!(
                "\\\\.\\pipe\\{}-admin.rpc",
                ledger_filename.to_string_lossy()
            ))
        } else {
            PathBuf::from("\\\\.\\pipe\\admin.rpc")
        }
    }
    #[cfg(not(target_family = "windows"))]
    {
        ledger_path.join("admin.rpc")
    }
}

// Connect to the Admin RPC interface
pub async fn connect(ledger_path: &Path) -> std::result::Result<gen_client::Client, RpcError> {
    let admin_rpc_path = admin_rpc_path(ledger_path);
    if !admin_rpc_path.exists() {
        Err(RpcError::Client(format!(
            "{} does not exist",
            admin_rpc_path.display()
        )))
    } else {
        ipc::connect::<_, gen_client::Client>(&format!("{}", admin_rpc_path.display())).await
    }
}

pub fn runtime() -> jsonrpc_server_utils::tokio::runtime::Runtime {
    jsonrpc_server_utils::tokio::runtime::Runtime::new().expect("new tokio runtime")
}

#[derive(Default, Deserialize, Clone)]
pub struct StakedNodesOverrides {
    #[serde(deserialize_with = "deserialize_pubkey_map")]
    pub staked_map_id: HashMap<Pubkey, u64>,
}

pub fn deserialize_pubkey_map<'de, D>(des: D) -> std::result::Result<HashMap<Pubkey, u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let container: HashMap<String, u64> = serde::Deserialize::deserialize(des)?;
    let mut container_typed: HashMap<Pubkey, u64> = HashMap::new();
    for (key, value) in container.iter() {
        let typed_key = Pubkey::try_from(key.as_str())
            .map_err(|_| serde::de::Error::invalid_type(serde::de::Unexpected::Map, &"PubKey"))?;
        container_typed.insert(typed_key, *value);
    }
    Ok(container_typed)
}

pub fn load_staked_nodes_overrides(
    path: &String,
) -> std::result::Result<StakedNodesOverrides, Box<dyn error::Error>> {
    debug!("Loading staked nodes overrides configuration from {}", path);
    if Path::new(&path).exists() {
        let file = std::fs::File::open(path)?;
        Ok(serde_yaml::from_reader(file)?)
    } else {
        Err(format!("Staked nodes overrides provided '{path}' a non-existing file path.").into())
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        serde_json::Value,
        solana_account_decoder::parse_token::spl_token_pubkey,
        solana_core::tower_storage::NullTowerStorage,
        solana_ledger::genesis_utils::{create_genesis_config, GenesisConfigInfo},
        solana_rpc::rpc::create_validator_exit,
        solana_runtime::{
            accounts_index::AccountSecondaryIndexes,
            bank::{Bank, BankTestConfig},
            inline_spl_token,
        },
        solana_sdk::{
            account::{Account, AccountSharedData},
            system_program,
        },
        solana_streamer::socket::SocketAddrSpace,
        spl_token_2022::{
            solana_program::{program_option::COption, program_pack::Pack},
            state::{Account as TokenAccount, AccountState as TokenAccountState, Mint},
        },
        std::{collections::HashSet, sync::atomic::AtomicBool},
    };

    #[derive(Default)]
    struct TestConfig {
        account_indexes: AccountSecondaryIndexes,
    }

    struct RpcHandler {
        io: MetaIoHandler<AdminRpcRequestMetadata>,
        meta: AdminRpcRequestMetadata,
        bank_forks: Arc<RwLock<BankForks>>,
    }

    impl RpcHandler {
        fn _start() -> Self {
            Self::start_with_config(TestConfig::default())
        }

        fn start_with_config(config: TestConfig) -> Self {
            let identity = Pubkey::new_unique();
            let cluster_info = Arc::new(ClusterInfo::new(
                ContactInfo {
                    id: identity,
                    ..ContactInfo::default()
                },
                Arc::new(Keypair::new()),
                SocketAddrSpace::Unspecified,
            ));
            let exit = Arc::new(AtomicBool::new(false));
            let validator_exit = create_validator_exit(&exit);
            let (bank_forks, vote_keypair) = new_bank_forks_with_config(BankTestConfig {
                secondary_indexes: config.account_indexes,
            });
            let vote_account = vote_keypair.pubkey();
            let start_progress = Arc::new(RwLock::new(ValidatorStartProgress::default()));
            let repair_whitelist = Arc::new(RwLock::new(HashSet::new()));
            let meta = AdminRpcRequestMetadata {
                rpc_addr: None,
                start_time: SystemTime::now(),
                start_progress,
                validator_exit,
                authorized_voter_keypairs: Arc::new(RwLock::new(vec![vote_keypair])),
                tower_storage: Arc::new(NullTowerStorage {}),
                post_init: Arc::new(RwLock::new(Some(AdminRpcRequestMetadataPostInit {
                    cluster_info,
                    bank_forks: bank_forks.clone(),
                    vote_account,
                    repair_whitelist,
                }))),
                staked_nodes_overrides: Arc::new(RwLock::new(HashMap::new())),
            };
            let mut io = MetaIoHandler::default();
            io.extend_with(AdminRpcImpl.to_delegate());

            Self {
                io,
                meta,
                bank_forks,
            }
        }

        fn root_bank(&self) -> Arc<Bank> {
            self.bank_forks.read().unwrap().root_bank()
        }
    }

    fn new_bank_forks_with_config(
        config: BankTestConfig,
    ) -> (Arc<RwLock<BankForks>>, Arc<Keypair>) {
        let GenesisConfigInfo {
            genesis_config,
            voting_keypair,
            ..
        } = create_genesis_config(1_000_000_000);

        let bank = Bank::new_for_tests_with_config(&genesis_config, config);
        (
            Arc::new(RwLock::new(BankForks::new(bank))),
            Arc::new(voting_keypair),
        )
    }

    #[test]
    fn test_secondary_index_key_sizes() {
        for secondary_index_enabled in [true, false] {
            let account_indexes = if secondary_index_enabled {
                AccountSecondaryIndexes {
                    keys: None,
                    indexes: HashSet::from([
                        AccountIndex::ProgramId,
                        AccountIndex::SplTokenMint,
                        AccountIndex::SplTokenOwner,
                    ]),
                }
            } else {
                AccountSecondaryIndexes::default()
            };

            // RPC & Bank Setup
            let rpc = RpcHandler::start_with_config(TestConfig { account_indexes });

            let bank = rpc.root_bank();
            let RpcHandler { io, meta, .. } = rpc;

            // Pubkeys
            let token_account1_pubkey = Pubkey::new_unique();
            let token_account2_pubkey = Pubkey::new_unique();
            let token_account3_pubkey = Pubkey::new_unique();
            let mint1_pubkey = Pubkey::new_unique();
            let mint2_pubkey = Pubkey::new_unique();
            let wallet1_pubkey = Pubkey::new_unique();
            let wallet2_pubkey = Pubkey::new_unique();
            let non_existent_pubkey = Pubkey::new_unique();
            let delegate = spl_token_pubkey(&Pubkey::new_unique());

            let mut num_default_spl_token_program_accounts = 0;
            let mut num_default_system_program_accounts = 0;

            if !secondary_index_enabled {
                // Test first with no accounts added & no secondary indexes enabled:
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{token_account1_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert!(sizes.is_empty());
            } else {
                // Count SPL Token Program Default Accounts
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{}"]}}"#,
                    inline_spl_token::id(),
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                num_default_spl_token_program_accounts =
                    *sizes.get(&RpcAccountIndex::ProgramId).unwrap();
                // Count System Program Default Accounts
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{}"]}}"#,
                    system_program::id(),
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                num_default_system_program_accounts =
                    *sizes.get(&RpcAccountIndex::ProgramId).unwrap();
            }

            // Add 2 basic wallet accounts
            let wallet1_account = AccountSharedData::from(Account {
                lamports: 11111111,
                owner: system_program::id(),
                ..Account::default()
            });
            bank.store_account(&wallet1_pubkey, &wallet1_account);
            let wallet2_account = AccountSharedData::from(Account {
                lamports: 11111111,
                owner: system_program::id(),
                ..Account::default()
            });
            bank.store_account(&wallet2_pubkey, &wallet2_account);

            // Add a token account
            let mut account1_data = vec![0; TokenAccount::get_packed_len()];
            let token_account1 = TokenAccount {
                mint: spl_token_pubkey(&mint1_pubkey),
                owner: spl_token_pubkey(&wallet1_pubkey),
                delegate: COption::Some(delegate),
                amount: 420,
                state: TokenAccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 30,
                close_authority: COption::Some(spl_token_pubkey(&wallet1_pubkey)),
            };
            TokenAccount::pack(token_account1, &mut account1_data).unwrap();
            let token_account1 = AccountSharedData::from(Account {
                lamports: 111,
                data: account1_data.to_vec(),
                owner: inline_spl_token::id(),
                ..Account::default()
            });
            bank.store_account(&token_account1_pubkey, &token_account1);

            // Add the mint
            let mut mint1_data = vec![0; Mint::get_packed_len()];
            let mint1_state = Mint {
                mint_authority: COption::Some(spl_token_pubkey(&wallet1_pubkey)),
                supply: 500,
                decimals: 2,
                is_initialized: true,
                freeze_authority: COption::Some(spl_token_pubkey(&wallet1_pubkey)),
            };
            Mint::pack(mint1_state, &mut mint1_data).unwrap();
            let mint_account1 = AccountSharedData::from(Account {
                lamports: 222,
                data: mint1_data.to_vec(),
                owner: inline_spl_token::id(),
                ..Account::default()
            });
            bank.store_account(&mint1_pubkey, &mint_account1);

            // Add another token account with the different owner, but same delegate, and mint
            let mut account2_data = vec![0; TokenAccount::get_packed_len()];
            let token_account2 = TokenAccount {
                mint: spl_token_pubkey(&mint1_pubkey),
                owner: spl_token_pubkey(&wallet2_pubkey),
                delegate: COption::Some(delegate),
                amount: 420,
                state: TokenAccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 30,
                close_authority: COption::Some(spl_token_pubkey(&wallet2_pubkey)),
            };
            TokenAccount::pack(token_account2, &mut account2_data).unwrap();
            let token_account2 = AccountSharedData::from(Account {
                lamports: 333,
                data: account2_data.to_vec(),
                owner: inline_spl_token::id(),
                ..Account::default()
            });
            bank.store_account(&token_account2_pubkey, &token_account2);

            // Add another token account with the same owner and delegate but different mint
            let mut account3_data = vec![0; TokenAccount::get_packed_len()];
            let token_account3 = TokenAccount {
                mint: spl_token_pubkey(&mint2_pubkey),
                owner: spl_token_pubkey(&wallet2_pubkey),
                delegate: COption::Some(delegate),
                amount: 42,
                state: TokenAccountState::Initialized,
                is_native: COption::None,
                delegated_amount: 30,
                close_authority: COption::Some(spl_token_pubkey(&wallet2_pubkey)),
            };
            TokenAccount::pack(token_account3, &mut account3_data).unwrap();
            let token_account3 = AccountSharedData::from(Account {
                lamports: 444,
                data: account3_data.to_vec(),
                owner: inline_spl_token::id(),
                ..Account::default()
            });
            bank.store_account(&token_account3_pubkey, &token_account3);

            // Add the new mint
            let mut mint2_data = vec![0; Mint::get_packed_len()];
            let mint2_state = Mint {
                mint_authority: COption::Some(spl_token_pubkey(&wallet2_pubkey)),
                supply: 200,
                decimals: 3,
                is_initialized: true,
                freeze_authority: COption::Some(spl_token_pubkey(&wallet2_pubkey)),
            };
            Mint::pack(mint2_state, &mut mint2_data).unwrap();
            let mint_account2 = AccountSharedData::from(Account {
                lamports: 555,
                data: mint2_data.to_vec(),
                owner: inline_spl_token::id(),
                ..Account::default()
            });
            bank.store_account(&mint2_pubkey, &mint_account2);

            // Accounts should now look like the following:
            //
            //                   -----system_program------
            //                  /                         \
            //                 /-(owns)                    \-(owns)
            //                /                             \
            //             wallet1                   ---wallet2---
            //               /                      /             \
            //              /-(SPL::owns)          /-(SPL::owns)   \-(SPL::owns)
            //             /                      /                 \
            //      token_account1         token_account2       token_account3
            //            \                     /                   /
            //             \-(SPL::mint)       /-(SPL::mint)       /-(SPL::mint)
            //              \                 /                   /
            //               --mint_account1--               mint_account2

            if secondary_index_enabled {
                // ----------- Test for a non-existant key -----------
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{non_existent_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert!(sizes.is_empty());
                // --------------- Test Queries ---------------
                // 1) Wallet1 - Owns 1 SPL Token
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{wallet1_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                assert_eq!(*sizes.get(&RpcAccountIndex::SplTokenOwner).unwrap(), 1);
                // 2) Wallet2 - Owns 2 SPL Tokens
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{wallet2_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                assert_eq!(*sizes.get(&RpcAccountIndex::SplTokenOwner).unwrap(), 2);
                // 3) Mint1 - Is in 2 SPL Accounts
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{mint1_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                assert_eq!(*sizes.get(&RpcAccountIndex::SplTokenMint).unwrap(), 2);
                // 4) Mint2 - Is in 1 SPL Account
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{mint2_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                assert_eq!(*sizes.get(&RpcAccountIndex::SplTokenMint).unwrap(), 1);
                // 5) SPL Token Program Owns 6 Accounts - 1 Default, 5 created above.
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{}"]}}"#,
                    inline_spl_token::id(),
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                assert_eq!(
                    *sizes.get(&RpcAccountIndex::ProgramId).unwrap(),
                    (num_default_spl_token_program_accounts + 5)
                );
                // 5) System Program Owns 4 Accounts + 2 Default, 2 created above.
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{}"]}}"#,
                    system_program::id(),
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert_eq!(sizes.len(), 1);
                assert_eq!(
                    *sizes.get(&RpcAccountIndex::ProgramId).unwrap(),
                    (num_default_system_program_accounts + 2)
                );
            } else {
                // ------------ Secondary Indexes Disabled ------------
                let req = format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"getSecondaryIndexKeySize","params":["{token_account2_pubkey}"]}}"#,
                );
                let res = io.handle_request_sync(&req, meta.clone());
                let result: Value = serde_json::from_str(&res.expect("actual response"))
                    .expect("actual response deserialization");
                let sizes: HashMap<RpcAccountIndex, usize> =
                    serde_json::from_value(result["result"].clone()).unwrap();
                assert!(sizes.is_empty());
            }
        }
    }
}
