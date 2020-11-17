use crate::{
    cluster_info::Node,
    contact_info::ContactInfo,
    gossip_service::discover_cluster,
    validator::{Validator, ValidatorConfig},
};
use solana_ledger::create_new_tmp_ledger;
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
        use solana_ledger::genesis_utils::BOOTSTRAP_VALIDATOR_LAMPORTS;
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
            42,
            bootstrap_validator_lamports,
            solana_sdk::genesis_config::ClusterType::Development,
        );
        genesis_config.rent.lamports_per_byte_year = 1;
        genesis_config.rent.exemption_threshold = 1.0;
        genesis_config.fee_rate_governor = FeeRateGovernor::new(fees, 0);

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);

        let config = ValidatorConfig {
            rpc_addrs: Some((node.info.rpc, node.info.rpc_pubsub, node.info.rpc_banks)),
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
