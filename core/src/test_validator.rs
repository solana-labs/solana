<<<<<<< HEAD
use crate::{
    cluster_info::Node,
    contact_info::ContactInfo,
    gossip_service::discover_cluster,
    validator::{Validator, ValidatorConfig},
=======
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
        bank_forks::{ArchiveFormat, SnapshotConfig, SnapshotVersion},
        genesis_utils::create_genesis_config_with_leader_ex,
        hardened_unpack::MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
    },
    solana_sdk::{
        account::Account,
        clock::DEFAULT_MS_PER_SLOT,
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
        sync::Arc,
        thread::sleep,
        time::Duration,
    },
>>>>>>> 7be677080... Rename CompressionType to ArchiveFormat
};
use solana_ledger::create_new_tmp_ledger;
use solana_runtime::genesis_utils::{
    bootstrap_validator_stake_lamports, BOOTSTRAP_VALIDATOR_LAMPORTS,
};
use solana_sdk::{
    clock::DEFAULT_DEV_SLOTS_PER_EPOCH,
    hash::Hash,
    native_token::sol_to_lamports,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::{path::PathBuf, sync::Arc};

pub struct TestValidator {
    pub server: Validator,
    pub leader_data: ContactInfo,
    pub alice: Keypair,
    pub ledger_path: PathBuf,
    pub genesis_hash: Hash,
    pub vote_pubkey: Pubkey,
}

pub struct TestValidatorOptions {
    pub fees: u64,
    pub bootstrap_validator_lamports: u64,
    pub mint_lamports: u64,
}

impl Default for TestValidatorOptions {
    fn default() -> Self {
        TestValidatorOptions {
            fees: 0,
            bootstrap_validator_lamports: BOOTSTRAP_VALIDATOR_LAMPORTS,
            mint_lamports: sol_to_lamports(1_000_000.0),
        }
    }
}

impl TestValidator {
    pub fn run() -> Self {
        Self::run_with_options(TestValidatorOptions::default())
    }

    /// Instantiates a TestValidator with custom fees. The bootstrap_validator_lamports will
    /// default to enough to cover 1 epoch of votes. This is an abitrary value based on current and
    /// foreseen uses of TestValidator. May need to be bumped if uses change in the future.
    pub fn run_with_fees(fees: u64) -> Self {
        let bootstrap_validator_lamports = fees * DEFAULT_DEV_SLOTS_PER_EPOCH * 5;
        Self::run_with_options(TestValidatorOptions {
            fees,
            bootstrap_validator_lamports,
            ..TestValidatorOptions::default()
        })
    }

    /// Instantiates a TestValidator with completely customized options.
    ///
    /// Note: if `fees` are non-zero, be sure to set a value for `bootstrap_validator_lamports`
    /// that can cover enough vote transaction fees for the test. TestValidatorOptions::default()
    /// may not be sufficient.
    pub fn run_with_options(options: TestValidatorOptions) -> Self {
        use solana_ledger::genesis_utils::{
            create_genesis_config_with_leader_ex, GenesisConfigInfo,
        };
        use solana_sdk::fee_calculator::FeeRateGovernor;

        let TestValidatorOptions {
            fees,
            bootstrap_validator_lamports,
            mint_lamports,
        } = options;
        let node_keypair = Arc::new(Keypair::new());
        let node = Node::new_localhost_with_pubkey(&node_keypair.pubkey());
        let contact_info = node.info.clone();

        if fees > 0 && bootstrap_validator_lamports < fees * DEFAULT_DEV_SLOTS_PER_EPOCH {
            warn!(
                "TestValidator::bootstrap_validator_lamports less than one epoch. \
                Only enough to cover {:?} slots",
                bootstrap_validator_lamports / fees
            );
        }

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair,
        } = create_genesis_config_with_leader_ex(
            mint_lamports,
            &contact_info.id,
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            bootstrap_validator_stake_lamports(),
            bootstrap_validator_lamports,
            solana_sdk::genesis_config::ClusterType::Development,
        );
        genesis_config.rent.lamports_per_byte_year = 1;
        genesis_config.rent.exemption_threshold = 1.0;
        genesis_config.fee_rate_governor = FeeRateGovernor::new(fees, 0);

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);

<<<<<<< HEAD
        let config = ValidatorConfig {
            rpc_addrs: Some((node.info.rpc, node.info.rpc_pubsub)),
=======
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

        let validator_config = ValidatorConfig {
            rpc_addrs: Some((
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), node.info.rpc.port()),
                SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                    node.info.rpc_pubsub.port(),
                ),
            )),
            rpc_config: config.rpc_config.clone(),
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
>>>>>>> 7be677080... Rename CompressionType to ArchiveFormat
            ..ValidatorConfig::default()
        };
        let vote_pubkey = voting_keypair.pubkey();
        let node = Validator::new(
            node,
            &node_keypair,
            &ledger_path,
            &voting_keypair.pubkey(),
            vec![Arc::new(voting_keypair)],
            None,
            &config,
        );
        discover_cluster(&contact_info.gossip, 1).expect("Node startup failed");
        TestValidator {
            server: node,
            leader_data: contact_info,
            alice: mint_keypair,
            ledger_path,
            genesis_hash: blockhash,
            vote_pubkey,
        }
    }
}
