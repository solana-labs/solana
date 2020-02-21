use solana_sdk::{
    account::Account,
    fee_calculator::FeeCalculator,
    genesis_config::GenesisConfig,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, Signer},
    system_program::{self, solana_system_program},
};
use solana_stake_program::stake_state;
use solana_vote_program::vote_state;
use std::borrow::Borrow;

// The default stake placed with the bootstrap validator
pub const BOOTSTRAP_VALIDATOR_LAMPORTS: u64 = 42;

pub struct ValidatorVoteKeypairs {
    pub node_keypair: Keypair,
    pub vote_keypair: Keypair,
    pub stake_keypair: Keypair,
}

impl ValidatorVoteKeypairs {
    pub fn new(node_keypair: Keypair, vote_keypair: Keypair, stake_keypair: Keypair) -> Self {
        Self {
            node_keypair,
            vote_keypair,
            stake_keypair,
        }
    }
}

pub struct GenesisConfigInfo {
    pub genesis_config: GenesisConfig,
    pub mint_keypair: Keypair,
    pub voting_keypair: Keypair,
}

pub fn create_genesis_config(mint_lamports: u64) -> GenesisConfigInfo {
    create_genesis_config_with_leader(mint_lamports, &Pubkey::new_rand(), 0)
}

pub fn create_genesis_config_with_vote_accounts(
    mint_lamports: u64,
    voting_keypairs: &[impl Borrow<ValidatorVoteKeypairs>],
) -> GenesisConfigInfo {
    let mut genesis_config_info = create_genesis_config(mint_lamports);
    for validator_voting_keypairs in voting_keypairs {
        let node_pubkey = validator_voting_keypairs.borrow().node_keypair.pubkey();
        let vote_pubkey = validator_voting_keypairs.borrow().vote_keypair.pubkey();
        let stake_pubkey = validator_voting_keypairs.borrow().stake_keypair.pubkey();

        // Create accounts
        let vote_account = vote_state::create_account(&vote_pubkey, &node_pubkey, 0, 100);
        let stake_account = stake_state::create_account(
            &stake_pubkey,
            &vote_pubkey,
            &vote_account,
            &genesis_config_info.genesis_config.rent,
            100,
        );

        // Put newly created accounts into genesis
        genesis_config_info.genesis_config.accounts.extend(vec![
            (vote_pubkey, vote_account.clone()),
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
        bootstrap_validator_stake_lamports,
        BOOTSTRAP_VALIDATOR_LAMPORTS,
    )
}

pub fn create_genesis_config_with_leader_ex(
    mint_lamports: u64,
    bootstrap_validator_pubkey: &Pubkey,
    bootstrap_validator_stake_lamports: u64,
    bootstrap_validator_lamports: u64,
) -> GenesisConfigInfo {
    let mint_keypair = Keypair::new();
    let bootstrap_validator_voting_keypair = Keypair::new();
    let bootstrap_validator_staking_keypair = Keypair::new();

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

    // Bare minimum program set
    let native_instruction_processors = vec![
        solana_system_program(),
        solana_bpf_loader_program!(),
        solana_vote_program!(),
        solana_stake_program!(),
    ];

    let fee_calculator = FeeCalculator::new(0, 0); // most tests can't handle transaction fees
    let mut genesis_config = GenesisConfig {
        accounts,
        native_instruction_processors,
        fee_calculator,
        rent,
        ..GenesisConfig::default()
    };

    solana_stake_program::add_genesis_accounts(&mut genesis_config);
    solana_storage_program::rewards_pools::add_genesis_accounts(&mut genesis_config);

    GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        voting_keypair: bootstrap_validator_voting_keypair,
    }
}
