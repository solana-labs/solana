use {
    crate::{
        cluster_info::Node,
        contact_info::ContactInfo,
        gossip_service::discover_cluster,
        validator::{Validator, ValidatorConfig},
    },
    solana_ledger::create_new_tmp_ledger,
    solana_sdk::{
        fee_calculator::FeeRateGovernor,
        hash::Hash,
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
    },
    std::{path::PathBuf, sync::Arc},
};

pub struct TestValidator {
    pub server: Validator,
    pub leader_data: ContactInfo,
    pub alice: Keypair,
    pub ledger_path: PathBuf,
    pub genesis_hash: Hash,
    pub vote_pubkey: Pubkey,
}

struct TestValidatorConfig {
    fee_rate_governor: FeeRateGovernor,
    validator_identity_lamports: u64,
    validator_stake_lamports: u64,
    mint_lamports: u64,
}

impl Default for TestValidatorConfig {
    fn default() -> Self {
        TestValidatorConfig {
            fee_rate_governor: FeeRateGovernor::new(0, 0),
            validator_identity_lamports: sol_to_lamports(500.),
            validator_stake_lamports: sol_to_lamports(1.),
            mint_lamports: sol_to_lamports(500_000_000.),
        }
    }
}

impl TestValidator {
    pub fn with_no_fee() -> Self {
        Self::new(TestValidatorConfig {
            fee_rate_governor: FeeRateGovernor::new(0, 0),
            ..TestValidatorConfig::default()
        })
    }

    pub fn with_custom_fee(target_lamports_per_signature: u64) -> Self {
        Self::new(TestValidatorConfig {
            fee_rate_governor: FeeRateGovernor::new(target_lamports_per_signature, 0),
            ..TestValidatorConfig::default()
        })
    }

    fn new(config: TestValidatorConfig) -> Self {
        use solana_ledger::genesis_utils::{
            create_genesis_config_with_leader_ex, GenesisConfigInfo,
        };

        let TestValidatorConfig {
            fee_rate_governor,
            validator_identity_lamports,
            validator_stake_lamports,
            mint_lamports,
        } = config;
        let node_keypair = Arc::new(Keypair::new());
        let node = Node::new_localhost_with_pubkey(&node_keypair.pubkey());
        let contact_info = node.info.clone();

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair,
        } = create_genesis_config_with_leader_ex(
            mint_lamports,
            &contact_info.id,
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            validator_stake_lamports,
            validator_identity_lamports,
            solana_sdk::genesis_config::ClusterType::Development,
        );
        genesis_config.rent.lamports_per_byte_year = 1;
        genesis_config.rent.exemption_threshold = 1.0;
        genesis_config.fee_rate_governor = fee_rate_governor;

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
