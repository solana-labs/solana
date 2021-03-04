use {
    crate::{
        cluster_info::Node,
        gossip_service::discover_cluster,
        rpc::JsonRpcConfig,
        validator::{Validator, ValidatorConfig, ValidatorExit, ValidatorStartProgress},
    },
    solana_client::rpc_client::RpcClient,
    solana_ledger::{blockstore::create_new_ledger, create_new_tmp_ledger},
    solana_runtime::{
        bank_forks::{ArchiveFormat, SnapshotConfig, SnapshotVersion},
        genesis_utils::create_genesis_config_with_leader_ex,
        hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
    },
    solana_sdk::{
        account::Account,
        clock::{Slot, DEFAULT_MS_PER_SLOT},
        commitment_config::CommitmentConfig,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        hash::Hash,
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rent::Rent,
        signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
    },
    std::{
        collections::HashMap,
        fs::remove_dir_all,
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::PathBuf,
        sync::{Arc, RwLock},
        thread::sleep,
        time::Duration,
    },
};

#[derive(Clone)]
pub struct ProgramInfo {
    pub program_id: Pubkey,
    pub loader: Pubkey,
    pub program_path: PathBuf,
}

#[derive(Default)]
pub struct TestValidatorGenesis {
    fee_rate_governor: FeeRateGovernor,
    ledger_path: Option<PathBuf>,
    rent: Rent,
    rpc_config: JsonRpcConfig,
    rpc_ports: Option<(u16, u16)>, // (JsonRpc, JsonRpcPubSub), None == random ports
    warp_slot: Option<Slot>,
    no_bpf_jit: bool,
    accounts: HashMap<Pubkey, Account>,
    programs: Vec<ProgramInfo>,
    pub validator_exit: Arc<RwLock<ValidatorExit>>,
    pub start_progress: Arc<RwLock<ValidatorStartProgress>>,
}

impl TestValidatorGenesis {
    pub fn ledger_path<P: Into<PathBuf>>(&mut self, ledger_path: P) -> &mut Self {
        self.ledger_path = Some(ledger_path.into());
        self
    }

    pub fn fee_rate_governor(&mut self, fee_rate_governor: FeeRateGovernor) -> &mut Self {
        self.fee_rate_governor = fee_rate_governor;
        self
    }

    pub fn rent(&mut self, rent: Rent) -> &mut Self {
        self.rent = rent;
        self
    }

    pub fn rpc_config(&mut self, rpc_config: JsonRpcConfig) -> &mut Self {
        self.rpc_config = rpc_config;
        self
    }

    pub fn rpc_port(&mut self, rpc_port: u16) -> &mut Self {
        self.rpc_ports = Some((rpc_port, rpc_port + 1));
        self
    }

    pub fn warp_slot(&mut self, warp_slot: Slot) -> &mut Self {
        self.warp_slot = Some(warp_slot);
        self
    }

    pub fn bpf_jit(&mut self, bpf_jit: bool) -> &mut Self {
        self.no_bpf_jit = !bpf_jit;
        self
    }

    /// Add an account to the test environment
    pub fn add_account(&mut self, address: Pubkey, account: Account) -> &mut Self {
        self.accounts.insert(address, account);
        self
    }

    pub fn add_accounts<T>(&mut self, accounts: T) -> &mut Self
    where
        T: IntoIterator<Item = (Pubkey, Account)>,
    {
        for (address, account) in accounts {
            self.add_account(address, account);
        }
        self
    }

    pub fn clone_accounts<T>(&mut self, addresses: T, rpc_client: &RpcClient) -> &mut Self
    where
        T: IntoIterator<Item = Pubkey>,
    {
        for address in addresses {
            info!("Fetching {} over RPC...", address);
            let account = rpc_client.get_account(&address).unwrap_or_else(|err| {
                error!("Failed to fetch {}: {}", address, err);
                crate::validator::abort();
            });
            self.add_account(address, account);
        }
        self
    }

    /// Add an account to the test environment with the account data in the provided `filename`
    pub fn add_account_with_file_data(
        &mut self,
        address: Pubkey,
        lamports: u64,
        owner: Pubkey,
        filename: &str,
    ) -> &mut Self {
        self.add_account(
            address,
            Account {
                lamports,
                data: solana_program_test::read_file(
                    solana_program_test::find_file(filename).unwrap_or_else(|| {
                        panic!("Unable to locate {}", filename);
                    }),
                ),
                owner,
                executable: false,
                rent_epoch: 0,
            },
        )
    }

    /// Add an account to the test environment with the account data in the provided as a base 64
    /// string
    pub fn add_account_with_base64_data(
        &mut self,
        address: Pubkey,
        lamports: u64,
        owner: Pubkey,
        data_base64: &str,
    ) -> &mut Self {
        self.add_account(
            address,
            Account {
                lamports,
                data: base64::decode(data_base64)
                    .unwrap_or_else(|err| panic!("Failed to base64 decode: {}", err)),
                owner,
                executable: false,
                rent_epoch: 0,
            },
        )
    }

    /// Add a BPF program to the test environment.
    ///
    /// `program_name` will also used to locate the BPF shared object in the current or fixtures
    /// directory.
    pub fn add_program(&mut self, program_name: &str, program_id: Pubkey) -> &mut Self {
        let program_path = solana_program_test::find_file(&format!("{}.so", program_name))
            .unwrap_or_else(|| panic!("Unable to locate program {}", program_name));

        self.programs.push(ProgramInfo {
            program_id,
            loader: solana_sdk::bpf_loader::id(),
            program_path,
        });
        self
    }

    /// Add a list of programs to the test environment.
    ///pub fn add_programs_with_path<'a>(&'a mut self, programs: &[ProgramInfo]) -> &'a mut Self {
    pub fn add_programs_with_path(&mut self, programs: &[ProgramInfo]) -> &mut Self {
        for program in programs {
            self.programs.push(program.clone());
        }
        self
    }

    /// Start a test validator with the address of the mint account that will receive tokens
    /// created at genesis.
    ///
    pub fn start_with_mint_address(
        &self,
        mint_address: Pubkey,
    ) -> Result<TestValidator, Box<dyn std::error::Error>> {
        TestValidator::start(mint_address, self)
    }

    /// Start a test validator
    ///
    /// Returns a new `TestValidator` as well as the keypair for the mint account that will receive tokens
    /// created at genesis.
    ///
    /// This function panics on initialization failure.
    pub fn start(&self) -> (TestValidator, Keypair) {
        let mint_keypair = Keypair::new();
        TestValidator::start(mint_keypair.pubkey(), self)
            .map(|test_validator| (test_validator, mint_keypair))
            .expect("Test validator failed to start")
    }
}

pub struct TestValidator {
    ledger_path: PathBuf,
    preserve_ledger: bool,
    rpc_pubsub_url: String,
    rpc_url: String,
    tpu: SocketAddr,
    gossip: SocketAddr,
    validator: Option<Validator>,
    vote_account_address: Pubkey,
}

impl TestValidator {
    /// Create and start a `TestValidator` with no transaction fees and minimal rent.
    ///
    /// This function panics on initialization failure.
    pub fn with_no_fees(mint_address: Pubkey) -> Self {
        TestValidatorGenesis::default()
            .fee_rate_governor(FeeRateGovernor::new(0, 0))
            .rent(Rent {
                lamports_per_byte_year: 1,
                exemption_threshold: 1.0,
                ..Rent::default()
            })
            .start_with_mint_address(mint_address)
            .expect("validator start failed")
    }

    /// Create and start a `TestValidator` with custom transaction fees and minimal rent.
    ///
    /// This function panics on initialization failure.
    pub fn with_custom_fees(mint_address: Pubkey, target_lamports_per_signature: u64) -> Self {
        TestValidatorGenesis::default()
            .fee_rate_governor(FeeRateGovernor::new(target_lamports_per_signature, 0))
            .rent(Rent {
                lamports_per_byte_year: 1,
                exemption_threshold: 1.0,
                ..Rent::default()
            })
            .start_with_mint_address(mint_address)
            .expect("validator start failed")
    }

    /// Initialize the ledger directory
    ///
    /// If `ledger_path` is `None`, a temporary ledger will be created.  Otherwise the ledger will
    /// be initialized in the provided directory if it doesn't already exist.
    ///
    /// Returns the path to the ledger directory.
    fn initialize_ledger(
        mint_address: Pubkey,
        config: &TestValidatorGenesis,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let validator_identity = Keypair::new();
        let validator_vote_account = Keypair::new();
        let validator_stake_account = Keypair::new();
        let validator_identity_lamports = sol_to_lamports(500.);
        let validator_stake_lamports = sol_to_lamports(1_000_000.);
        let mint_lamports = sol_to_lamports(500_000_000.);

        let mut accounts = config.accounts.clone();
        for (address, account) in solana_program_test::programs::spl_programs(&config.rent) {
            accounts.entry(address).or_insert(account);
        }
        for program in &config.programs {
            let data = solana_program_test::read_file(&program.program_path);
            accounts.insert(
                program.program_id,
                Account {
                    lamports: Rent::default().minimum_balance(data.len()).min(1),
                    data,
                    owner: program.loader,
                    executable: true,
                    rent_epoch: 0,
                },
            );
        }

        let mut genesis_config = create_genesis_config_with_leader_ex(
            mint_lamports,
            &mint_address,
            &validator_identity.pubkey(),
            &validator_vote_account.pubkey(),
            &validator_stake_account.pubkey(),
            validator_stake_lamports,
            validator_identity_lamports,
            config.fee_rate_governor.clone(),
            config.rent,
            solana_sdk::genesis_config::ClusterType::Development,
            accounts.into_iter().collect(),
        );
        genesis_config.epoch_schedule = solana_sdk::epoch_schedule::EpochSchedule::without_warmup();

        let ledger_path = match &config.ledger_path {
            None => create_new_tmp_ledger!(&genesis_config).0,
            Some(ledger_path) => {
                if ledger_path.join("validator-keypair.json").exists() {
                    return Ok(ledger_path.to_path_buf());
                }

                let _ = create_new_ledger(
                    ledger_path,
                    &genesis_config,
                    MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
                    solana_ledger::blockstore_db::AccessType::PrimaryOnly,
                )
                .map_err(|err| {
                    format!(
                        "Failed to create ledger at {}: {}",
                        ledger_path.display(),
                        err
                    )
                })?;
                ledger_path.to_path_buf()
            }
        };

        write_keypair_file(
            &validator_identity,
            ledger_path.join("validator-keypair.json").to_str().unwrap(),
        )?;
        write_keypair_file(
            &validator_vote_account,
            ledger_path
                .join("vote-account-keypair.json")
                .to_str()
                .unwrap(),
        )?;

        Ok(ledger_path)
    }

    /// Starts a TestValidator at the provided ledger directory
    fn start(
        mint_address: Pubkey,
        config: &TestValidatorGenesis,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let preserve_ledger = config.ledger_path.is_some();
        let ledger_path = TestValidator::initialize_ledger(mint_address, config)?;

        let validator_identity =
            read_keypair_file(ledger_path.join("validator-keypair.json").to_str().unwrap())?;
        let validator_vote_account = read_keypair_file(
            ledger_path
                .join("vote-account-keypair.json")
                .to_str()
                .unwrap(),
        )?;

        let mut node = Node::new_localhost_with_pubkey(&validator_identity.pubkey());
        if let Some((rpc, rpc_pubsub)) = config.rpc_ports {
            node.info.rpc = SocketAddr::new(node.info.gossip.ip(), rpc);
            node.info.rpc_pubsub = SocketAddr::new(node.info.gossip.ip(), rpc_pubsub);
        }

        let vote_account_address = validator_vote_account.pubkey();
        let rpc_url = format!("http://{}", node.info.rpc);
        let rpc_pubsub_url = format!("ws://{}/", node.info.rpc_pubsub);
        let tpu = node.info.tpu;
        let gossip = node.info.gossip;

        let mut rpc_config = config.rpc_config.clone();
        rpc_config.identity_pubkey = validator_identity.pubkey();

        let validator_config = ValidatorConfig {
            rpc_addrs: Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), node.info.rpc.port()),
                SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                    node.info.rpc_pubsub.port(),
                ),
            )),
            rpc_config,
            accounts_hash_interval_slots: 100,
            account_paths: vec![ledger_path.join("accounts")],
            poh_verify: false, // Skip PoH verification of ledger on startup for speed
            snapshot_config: Some(SnapshotConfig {
                snapshot_interval_slots: 100,
                snapshot_path: ledger_path.join("snapshot"),
                snapshot_package_output_path: ledger_path.to_path_buf(),
                archive_format: ArchiveFormat::Tar,
                snapshot_version: SnapshotVersion::default(),
            }),
            enforce_ulimit_nofile: false,
            warp_slot: config.warp_slot,
            bpf_jit: !config.no_bpf_jit,
            validator_exit: config.validator_exit.clone(),
            ..ValidatorConfig::default()
        };

        let validator = Some(Validator::new(
            node,
            &Arc::new(validator_identity),
            &ledger_path,
            &validator_vote_account.pubkey(),
            vec![Arc::new(validator_vote_account)],
            vec![],
            &validator_config,
            true, // should_check_duplicate_instance
            config.start_progress.clone(),
        ));

        // Needed to avoid panics in `solana-responder-gossip` in tests that create a number of
        // test validators concurrently...
        discover_cluster(&gossip, 1)
            .map_err(|err| format!("TestValidator startup failed: {:?}", err))?;

        // This is a hack to delay until the fees are non-zero for test consistency
        // (fees from genesis are zero until the first block with a transaction in it is completed
        //  due to a bug in the Bank)
        {
            let rpc_client =
                RpcClient::new_with_commitment(rpc_url.clone(), CommitmentConfig::processed());

            if let Ok(result) = rpc_client.get_fee_rate_governor() {
                let fee_rate_governor = result.value;
                if fee_rate_governor.target_lamports_per_signature > 0 {
                    loop {
                        match rpc_client.get_recent_blockhash() {
                            Ok((_blockhash, fee_calculator)) => {
                                if fee_calculator.lamports_per_signature != 0 {
                                    break;
                                }
                            }
                            Err(err) => {
                                warn!("get_recent_blockhash() failed: {:?}", err);
                                break;
                            }
                        }
                        sleep(Duration::from_millis(DEFAULT_MS_PER_SLOT));
                    }
                }
            }
        }

        Ok(TestValidator {
            ledger_path,
            preserve_ledger,
            rpc_pubsub_url,
            rpc_url,
            gossip,
            tpu,
            validator,
            vote_account_address,
        })
    }

    /// Return the validator's TPU address
    pub fn tpu(&self) -> &SocketAddr {
        &self.tpu
    }

    /// Return the validator's Gossip address
    pub fn gossip(&self) -> &SocketAddr {
        &self.gossip
    }

    /// Return the validator's JSON RPC URL
    pub fn rpc_url(&self) -> String {
        self.rpc_url.clone()
    }

    /// Return the validator's JSON RPC PubSub URL
    pub fn rpc_pubsub_url(&self) -> String {
        self.rpc_pubsub_url.clone()
    }

    /// Return the validator's vote account address
    pub fn vote_account_address(&self) -> Pubkey {
        self.vote_account_address
    }

    /// Return an RpcClient for the validator.  As a convenience, also return a recent blockhash and
    /// associated fee calculator
    pub fn rpc_client(&self) -> (RpcClient, Hash, FeeCalculator) {
        let rpc_client =
            RpcClient::new_with_commitment(self.rpc_url.clone(), CommitmentConfig::processed());
        let (recent_blockhash, fee_calculator) = rpc_client
            .get_recent_blockhash()
            .expect("get_recent_blockhash");

        (rpc_client, recent_blockhash, fee_calculator)
    }

    pub fn join(mut self) {
        if let Some(validator) = self.validator.take() {
            validator.join();
        }
    }
}

impl Drop for TestValidator {
    fn drop(&mut self) {
        if let Some(validator) = self.validator.take() {
            validator.close();
        }
        if !self.preserve_ledger {
            remove_dir_all(&self.ledger_path).unwrap_or_else(|err| {
                panic!(
                    "Failed to remove ledger directory {}: {}",
                    self.ledger_path.display(),
                    err
                )
            });
        }
    }
}
