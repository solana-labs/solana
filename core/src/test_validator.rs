use {
    crate::{
        cluster_info::Node,
        validator::{Validator, ValidatorConfig},
    },
    solana_ledger::create_new_tmp_ledger,
    solana_sdk::{
        fee_calculator::FeeRateGovernor,
        hash::Hash,
        native_token::sol_to_lamports,
        pubkey::Pubkey,
        rent::Rent,
        signature::{Keypair, Signer},
    },
    std::{fs::remove_dir_all, net::SocketAddr, path::PathBuf, sync::Arc},
};

pub struct TestValidatorConfig {
    pub fee_rate_governor: FeeRateGovernor,
    pub mint_lamports: u64,
    pub rent: Rent,
    pub validator_identity_keypair: Keypair,
    pub validator_identity_lamports: u64,
    pub validator_stake_lamports: u64,
}

impl Default for TestValidatorConfig {
    fn default() -> Self {
        Self {
            fee_rate_governor: FeeRateGovernor::default(),
            mint_lamports: sol_to_lamports(500_000_000.),
            rent: Rent::default(),
            validator_identity_keypair: Keypair::new(),
            validator_identity_lamports: sol_to_lamports(500.),
            validator_stake_lamports: sol_to_lamports(1.),
        }
    }
}

pub struct TestValidator {
    validator: Validator,
    ledger_path: PathBuf,
    preserve_ledger: bool,

    genesis_hash: Hash,
    mint_keypair: Keypair,
    vote_account_address: Pubkey,

    tpu: SocketAddr,
    rpc_url: String,
    rpc_pubsub_url: String,
}

impl Default for TestValidator {
    fn default() -> Self {
        Self::new(TestValidatorConfig::default())
    }
}

impl TestValidator {
    pub fn with_no_fees() -> Self {
        Self::new(TestValidatorConfig {
            fee_rate_governor: FeeRateGovernor::new(0, 0),
            rent: Rent {
                lamports_per_byte_year: 1,
                exemption_threshold: 1.0,
                ..Rent::default()
            },
            ..TestValidatorConfig::default()
        })
    }

    pub fn with_custom_fees(target_lamports_per_signature: u64) -> Self {
        Self::new(TestValidatorConfig {
            fee_rate_governor: FeeRateGovernor::new(target_lamports_per_signature, 0),
            rent: Rent {
                lamports_per_byte_year: 1,
                exemption_threshold: 1.0,
                ..Rent::default()
            },
            ..TestValidatorConfig::default()
        })
    }

    pub fn new(config: TestValidatorConfig) -> Self {
        use solana_ledger::genesis_utils::{
            create_genesis_config_with_leader_ex, GenesisConfigInfo,
        };

        let TestValidatorConfig {
            fee_rate_governor,
            mint_lamports,
            rent,
            validator_identity_keypair,
            validator_identity_lamports,
            validator_stake_lamports,
        } = config;
        let validator_identity_keypair = Arc::new(validator_identity_keypair);

        let node = Node::new_localhost_with_pubkey(&validator_identity_keypair.pubkey());

        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair: vote_account_keypair,
        } = create_genesis_config_with_leader_ex(
            mint_lamports,
            &node.info.id,
            &Keypair::new(),
            &Keypair::new().pubkey(),
            validator_stake_lamports,
            validator_identity_lamports,
            solana_sdk::genesis_config::ClusterType::Development,
        );

        genesis_config.rent = rent;
        genesis_config.fee_rate_governor = fee_rate_governor;

        let (ledger_path, blockhash) = create_new_tmp_ledger!(&genesis_config);

        let config = ValidatorConfig {
            rpc_addrs: Some((node.info.rpc, node.info.rpc_pubsub, node.info.rpc_banks)),
            ..ValidatorConfig::default()
        };

        let vote_account_address = vote_account_keypair.pubkey();
        let rpc_url = format!("http://{}:{}", node.info.rpc.ip(), node.info.rpc.port());
        let rpc_pubsub_url = format!("ws://{}/", node.info.rpc_pubsub);
        let tpu = node.info.tpu;

        let validator = Validator::new(
            node,
            &validator_identity_keypair,
            &ledger_path,
            &vote_account_keypair.pubkey(),
            vec![Arc::new(vote_account_keypair)],
            None,
            &config,
        );

        TestValidator {
            validator,
            vote_account_address,
            mint_keypair,
            ledger_path,
            genesis_hash: blockhash,
            tpu,
            rpc_url,
            rpc_pubsub_url,
            preserve_ledger: false,
        }
    }

    pub fn close(self) {
        self.validator.close().unwrap();
        if !self.preserve_ledger {
            remove_dir_all(&self.ledger_path).unwrap();
        }
    }

    pub fn tpu(&self) -> &SocketAddr {
        &self.tpu
    }

    pub fn mint_keypair(&self) -> Keypair {
        Keypair::from_bytes(&self.mint_keypair.to_bytes()).unwrap()
    }

    pub fn rpc_url(&self) -> String {
        self.rpc_url.clone()
    }

    pub fn rpc_pubsub_url(&self) -> String {
        self.rpc_pubsub_url.clone()
    }

    pub fn genesis_hash(&self) -> Hash {
        self.genesis_hash
    }

    pub fn vote_account_address(&self) -> Pubkey {
        self.vote_account_address
    }
}
