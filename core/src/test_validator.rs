use crate::{
    cluster_info::Node,
    contact_info::ContactInfo,
    gossip_service::discover_cluster,
    validator::{Validator, ValidatorConfig},
};
use solana_ledger::create_new_tmp_ledger;
use solana_sdk::{
    hash::Hash,
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
            mint_lamports: 1_000_000,
        }
    }
}

impl TestValidator {
    pub fn run() -> Self {
        Self::run_with_options(TestValidatorOptions::default())
    }

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

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair,
        } = create_genesis_config_with_leader_ex(
            mint_lamports,
            &contact_info.id,
            &Keypair::new(),
            &Pubkey::new_rand(),
            42,
            bootstrap_validator_lamports,
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
