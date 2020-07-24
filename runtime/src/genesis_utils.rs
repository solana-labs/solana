use solana_sdk::{
    account::Account,
    fee_calculator::FeeRateGovernor,
    genesis_config::GenesisConfig,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_program,
};
use solana_stake_program::stake_state;
use solana_vote_program::vote_state;
use std::{borrow::Borrow, sync::Arc};

// The default stake placed with the bootstrap validator
pub const BOOTSTRAP_VALIDATOR_LAMPORTS: u64 = 42;

pub struct ValidatorVoteKeypairs {
    pub node_keypair: Arc<Keypair>,
    pub vote_keypair: Arc<Keypair>,
    pub stake_keypair: Arc<Keypair>,
}

impl ValidatorVoteKeypairs {
    pub fn new(
        node_keypair: Arc<Keypair>,
        vote_keypair: Arc<Keypair>,
        stake_keypair: Arc<Keypair>,
    ) -> Self {
        Self {
            node_keypair,
            vote_keypair,
            stake_keypair,
        }
    }

    pub fn new_rand() -> Self {
        Self {
            node_keypair: Arc::new(Keypair::new()),
            vote_keypair: Arc::new(Keypair::new()),
            stake_keypair: Arc::new(Keypair::new()),
        }
    }
}

pub struct GenesisConfigInfo {
    pub genesis_config: GenesisConfig,
    pub mint_keypair: Keypair,
    pub voting_keypair: Arc<Keypair>,
}

pub fn create_genesis_config(mint_lamports: u64) -> GenesisConfigInfo {
    create_genesis_config_with_leader(mint_lamports, &Pubkey::new_rand(), 0)
}

pub fn create_genesis_config_with_vote_accounts(
    mint_lamports: u64,
    voting_keypairs: &[impl Borrow<ValidatorVoteKeypairs>],
    stakes: Vec<u64>,
) -> GenesisConfigInfo {
    assert_eq!(voting_keypairs.len(), stakes.len());

    let mut genesis_config_info = create_genesis_config_with_leader_ex(
        mint_lamports,
        &voting_keypairs[0].borrow().node_keypair.pubkey(),
        voting_keypairs[0].borrow().vote_keypair.clone(),
        voting_keypairs[0].borrow().stake_keypair.clone(),
        stakes[0],
        BOOTSTRAP_VALIDATOR_LAMPORTS,
    );

    for (validator_voting_keypairs, stake) in voting_keypairs.iter().zip(stakes) {
        let node_pubkey = validator_voting_keypairs.borrow().node_keypair.pubkey();
        let vote_pubkey = validator_voting_keypairs.borrow().vote_keypair.pubkey();
        let stake_pubkey = validator_voting_keypairs.borrow().stake_keypair.pubkey();

        // Create accounts
        let node_account = Account::new(BOOTSTRAP_VALIDATOR_LAMPORTS, 0, &system_program::id());
        let vote_account = vote_state::create_account(&vote_pubkey, &node_pubkey, 0, stake);
        let stake_account = stake_state::create_account(
            &stake_pubkey,
            &vote_pubkey,
            &vote_account,
            &genesis_config_info.genesis_config.rent,
            stake,
        );

        // Put newly created accounts into genesis
        genesis_config_info.genesis_config.accounts.extend(vec![
            (node_pubkey, node_account),
            (vote_pubkey, vote_account),
            (stake_pubkey, stake_account),
        ]);
    }

    genesis_config_info
}

pub fn create_genesis_config_with_leader(
    mint_lamports: u64,
    bootstrap_validator_pubkey: &Pubkey,
    bootstrap_validator_stake_lamports: u64,
) -> GenesisConfigInfo {
    create_genesis_config_with_leader_ex(
        mint_lamports,
        bootstrap_validator_pubkey,
        Arc::new(Keypair::new()),
        Arc::new(Keypair::new()),
        bootstrap_validator_stake_lamports,
        BOOTSTRAP_VALIDATOR_LAMPORTS,
    )
}

pub fn create_genesis_config_with_leader_ex(
    mint_lamports: u64,
    bootstrap_validator_pubkey: &Pubkey,
    bootstrap_validator_voting_keypair: Arc<Keypair>,
    bootstrap_validator_staking_keypair: Arc<Keypair>,
    bootstrap_validator_stake_lamports: u64,
    bootstrap_validator_lamports: u64,
) -> GenesisConfigInfo {
    let mint_keypair = Keypair::new();
    let bootstrap_validator_vote_account = vote_state::create_account(
        &bootstrap_validator_voting_keypair.pubkey(),
        &bootstrap_validator_pubkey,
        0,
        bootstrap_validator_stake_lamports,
    );

    let rent = Rent::free();

    let bootstrap_validator_stake_account = stake_state::create_account(
        &bootstrap_validator_staking_keypair.pubkey(),
        &bootstrap_validator_voting_keypair.pubkey(),
        &bootstrap_validator_vote_account,
        &rent,
        bootstrap_validator_stake_lamports,
    );

    let accounts = [
        (
            mint_keypair.pubkey(),
            Account::new(mint_lamports, 0, &system_program::id()),
        ),
        (
            *bootstrap_validator_pubkey,
            Account::new(bootstrap_validator_lamports, 0, &system_program::id()),
        ),
        (
            bootstrap_validator_voting_keypair.pubkey(),
            bootstrap_validator_vote_account,
        ),
        (
            bootstrap_validator_staking_keypair.pubkey(),
            bootstrap_validator_stake_account,
        ),
    ]
    .iter()
    .cloned()
    .collect();

    let fee_rate_governor = FeeRateGovernor::new(0, 0); // most tests can't handle transaction fees
    let mut genesis_config = GenesisConfig {
        accounts,
        fee_rate_governor,
        rent,
        ..GenesisConfig::default()
    };

    solana_stake_program::add_genesis_accounts(&mut genesis_config);

    GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        voting_keypair: bootstrap_validator_voting_keypair,
    }
}
