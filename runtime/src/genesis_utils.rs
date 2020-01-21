use solana_sdk::{
    account::Account,
    fee_calculator::FeeCalculator,
    genesis_config::GenesisConfig,
    pubkey::Pubkey,
    rent::Rent,
    signature::{Keypair, KeypairUtil},
    system_program::{self, solana_system_program},
};
use solana_stake_program::stake_state;
use solana_vote_program::vote_state;

// The default stake placed with the bootstrap validator
pub const BOOTSTRAP_VALIDATOR_LAMPORTS: u64 = 42;

pub struct GenesisConfigInfo {
    pub genesis_config: GenesisConfig,
    pub mint_keypair: Keypair,
    pub voting_keypair: Keypair,
}

pub fn create_genesis_config(mint_lamports: u64) -> GenesisConfigInfo {
    create_genesis_config_with_leader(mint_lamports, &Pubkey::new_rand(), 0)
}

pub fn create_genesis_config_with_leader(
    mint_lamports: u64,
    bootstrap_validator_pubkey: &Pubkey,
    bootstrap_validator_stake_lamports: u64,
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
            Account::new(BOOTSTRAP_VALIDATOR_LAMPORTS, 0, &system_program::id()),
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
