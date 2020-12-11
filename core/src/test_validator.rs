use {
    crate::{
        cluster_info::Node,
        gossip_service::discover_cluster,
        rpc::JsonRpcConfig,
        validator::{Validator, ValidatorConfig},
    },
    solana_client::rpc_client::RpcClient,
    solana_ledger::{blockstore::create_new_ledger, create_new_tmp_ledger},
    solana_runtime::{
        bank_forks::{CompressionType, SnapshotConfig, SnapshotVersion},
        genesis_utils::create_genesis_config_with_leader_ex,
        hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
    },
    solana_sdk::{
        clock::DEFAULT_MS_PER_SLOT,
        commitment_config::CommitmentConfig,
        fee_calculator::FeeRateGovernor,
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rent::Rent,
        signature::{read_keypair_file, write_keypair_file, Keypair, Signer},
    },
    std::{
        fs::remove_dir_all,
        net::SocketAddr,
        path::{Path, PathBuf},
        sync::Arc,
        thread::sleep,
        time::Duration,
    },
};

pub struct TestValidatorGenesisConfig {
    pub fee_rate_governor: FeeRateGovernor,
    pub mint_address: Pubkey,
    pub rent: Rent,
}

#[derive(Default)]
pub struct TestValidatorStartConfig {
    pub preserve_ledger: bool,
    pub rpc_config: JsonRpcConfig,
    pub rpc_ports: Option<(u16, u16)>, // (JsonRpc, JsonRpcPubSub), None == random ports
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
    /// The default test validator is intended to be generically suitable for unit testing.
    ///
    /// It uses a unique temporary ledger that is deleted on `close` and randomly assigned ports.
    /// All test tokens will be minted into `mint_address`
    ///
    /// This function panics on initialization failure.
    pub fn new(mint_address: Pubkey) -> Self {
        let ledger_path = Self::initialize_ledger(
            None,
            TestValidatorGenesisConfig {
                fee_rate_governor: FeeRateGovernor::default(),
                mint_address,
                rent: Rent::default(),
            },
        )
        .unwrap();
        Self::start(&ledger_path, TestValidatorStartConfig::default()).unwrap()
    }

    /// Create a `TestValidator` with no transaction fees and minimal rent.
    ///
    /// This function panics on initialization failure.
    pub fn with_no_fees(mint_address: Pubkey) -> Self {
        let ledger_path = Self::initialize_ledger(
            None,
            TestValidatorGenesisConfig {
                fee_rate_governor: FeeRateGovernor::new(0, 0),
                mint_address,
                rent: Rent {
                    lamports_per_byte_year: 1,
                    exemption_threshold: 1.0,
                    ..Rent::default()
                },
            },
        )
        .unwrap();
        Self::start(&ledger_path, TestValidatorStartConfig::default()).unwrap()
    }

    /// Create a `TestValidator` with custom transaction fees and minimal rent.
    ///
    /// This function panics on initialization failure.
    pub fn with_custom_fees(mint_address: Pubkey, target_lamports_per_signature: u64) -> Self {
        let ledger_path = Self::initialize_ledger(
            None,
            TestValidatorGenesisConfig {
                fee_rate_governor: FeeRateGovernor::new(target_lamports_per_signature, 0),
                mint_address,
                rent: Rent {
                    lamports_per_byte_year: 1,
                    exemption_threshold: 1.0,
                    ..Rent::default()
                },
            },
        )
        .unwrap();
        Self::start(&ledger_path, TestValidatorStartConfig::default()).unwrap()
    }

    /// Initialize the test validator's ledger directory
    ///
    /// If `ledger_path` is `None`, a temporary ledger will be created.  Otherwise the ledger will
    /// be initialized in the provided directory.
    ///
    /// Returns the path to the ledger directory.
    pub fn initialize_ledger(
        ledger_path: Option<&Path>,
        config: TestValidatorGenesisConfig,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let TestValidatorGenesisConfig {
            fee_rate_governor,
            mint_address,
            rent,
        } = config;

        let validator_identity_keypair = Keypair::new();
        let validator_vote_account = Keypair::new();
        let validator_stake_account = Keypair::new();
        let validator_identity_lamports = sol_to_lamports(500.);
        let validator_stake_lamports = sol_to_lamports(1_000_000.);
        let mint_lamports = sol_to_lamports(500_000_000.);

        let initial_accounts = solana_program_test::programs::spl_programs(&rent);
        let genesis_config = create_genesis_config_with_leader_ex(
            mint_lamports,
            &mint_address,
            &validator_identity_keypair.pubkey(),
            &validator_vote_account.pubkey(),
            &validator_stake_account.pubkey(),
            validator_stake_lamports,
            validator_identity_lamports,
            fee_rate_governor,
            rent,
            solana_sdk::genesis_config::ClusterType::Development,
            initial_accounts,
        );

        let ledger_path = match ledger_path {
            None => create_new_tmp_ledger!(&genesis_config).0,
            Some(ledger_path) => {
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
            &validator_identity_keypair,
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
    pub fn start(
        ledger_path: &Path,
        config: TestValidatorStartConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let validator_identity_keypair =
            read_keypair_file(ledger_path.join("validator-keypair.json").to_str().unwrap())?;
        let validator_vote_account = read_keypair_file(
            ledger_path
                .join("vote-account-keypair.json")
                .to_str()
                .unwrap(),
        )?;

        let mut node = Node::new_localhost_with_pubkey(&validator_identity_keypair.pubkey());
        if let Some((rpc, rpc_pubsub)) = config.rpc_ports {
            node.info.rpc = SocketAddr::new(node.info.gossip.ip(), rpc);
            node.info.rpc_pubsub = SocketAddr::new(node.info.gossip.ip(), rpc_pubsub);
        }

        let vote_account_address = validator_vote_account.pubkey();
        let rpc_url = format!("http://{}:{}", node.info.rpc.ip(), node.info.rpc.port());
        let rpc_pubsub_url = format!("ws://{}/", node.info.rpc_pubsub);
        let tpu = node.info.tpu;
        let gossip = node.info.gossip;

        let validator_config = ValidatorConfig {
            rpc_addrs: Some((node.info.rpc, node.info.rpc_pubsub)),
            rpc_config: config.rpc_config,
            accounts_hash_interval_slots: 100,
            account_paths: vec![ledger_path.join("accounts")],
            poh_verify: false, // Skip PoH verification of ledger on startup for speed
            snapshot_config: Some(SnapshotConfig {
                snapshot_interval_slots: 100,
                snapshot_path: ledger_path.join("snapshot"),
                snapshot_package_output_path: ledger_path.to_path_buf(),
                compression: CompressionType::NoCompression,
                snapshot_version: SnapshotVersion::default(),
            }),
            ..ValidatorConfig::default()
        };

        let validator = Some(Validator::new(
            node,
            &Arc::new(validator_identity_keypair),
            &ledger_path,
            &validator_vote_account.pubkey(),
            vec![Arc::new(validator_vote_account)],
            None,
            &validator_config,
        ));

        // Needed to avoid panics in `solana-responder-gossip` in tests that create a number of
        // test validators concurrently...
        discover_cluster(&gossip, 1).expect("TestValidator startup failed");

        Ok(TestValidator {
            ledger_path: ledger_path.to_path_buf(),
            preserve_ledger: false,
            rpc_pubsub_url,
            rpc_url,
            gossip,
            tpu,
            validator,
            vote_account_address,
        })
    }

    /// Return the test validator's TPU address
    pub fn tpu(&self) -> &SocketAddr {
        &self.tpu
    }

    /// Return the test validator's Gossip address
    pub fn gossip(&self) -> &SocketAddr {
        &self.gossip
    }

    /// Return the test validator's JSON RPC URL
    pub fn rpc_url(&self) -> String {
        self.rpc_url.clone()
    }

    /// Return the test validator's JSON RPC PubSub URL
    pub fn rpc_pubsub_url(&self) -> String {
        self.rpc_pubsub_url.clone()
    }

    /// Return the vote account address of the validator
    pub fn vote_account_address(&self) -> Pubkey {
        self.vote_account_address
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
