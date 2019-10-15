use solana_sdk::{
    account::Account,
    fee_calculator::FeeCalculator,
    genesis_block::GenesisBlock,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_program::{self, solana_system_program},
};
use solana_stake_api::stake_state;
use solana_vote_api::vote_state;

// The default stake placed with the bootstrap leader
pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 42;

pub struct GenesisBlockInfo {
    pub genesis_block: GenesisBlock,
    pub mint_keypair: Keypair,
    pub voting_keypair: Keypair,
}

pub fn create_genesis_block(mint_lamports: u64) -> GenesisBlockInfo {
    create_genesis_block_with_leader(mint_lamports, &Pubkey::new_rand(), 0)
}

pub fn create_genesis_block_with_leader(
    mint_lamports: u64,
    bootstrap_leader_pubkey: &Pubkey,
    bootstrap_leader_stake_lamports: u64,
) -> GenesisBlockInfo {
    let bootstrap_leader_lamports = BOOTSTRAP_LEADER_LAMPORTS; // TODO: pass this in as an argument?
    let mint_keypair = Keypair::new();
    let voting_keypair = Keypair::new();
    let staking_keypair = Keypair::new();

    // TODO: de-duplicate the stake... passive staking is fully implemented
    let vote_account = vote_state::create_account(
        &voting_keypair.pubkey(),
        &bootstrap_leader_pubkey,
        0,
        bootstrap_leader_stake_lamports,
    );

    let stake_account = stake_state::create_account(
        &staking_keypair.pubkey(),
        &voting_keypair.pubkey(),
        &vote_account,
        bootstrap_leader_stake_lamports,
    );

    let accounts = vec![
        // the mint
        (
            mint_keypair.pubkey(),
            Account::new(mint_lamports, 0, &system_program::id()),
        ),
        // node needs an account to issue votes and storage proofs from, this will require
        //  airdrops at some point to cover fees...
        (
            *bootstrap_leader_pubkey,
            Account::new(bootstrap_leader_lamports, 0, &system_program::id()),
        ),
        // where votes go to
        (voting_keypair.pubkey(), vote_account),
        // passive bootstrap leader stake, duplicates above temporarily
        (staking_keypair.pubkey(), stake_account),
    ];

    // Bare minimum program set
    let native_instruction_processors = vec![
        solana_system_program(),
        solana_bpf_loader_program!(),
        solana_vote_program!(),
        solana_stake_program!(),
    ];
    let fee_calculator = FeeCalculator::new(0, 0); // most tests don't want fees

    let mut genesis_block = GenesisBlock {
        accounts,
        native_instruction_processors,
        fee_calculator,
        ..GenesisBlock::default()
    };

    solana_stake_api::add_genesis_accounts(&mut genesis_block);
    solana_storage_api::rewards_pools::add_genesis_accounts(&mut genesis_block);

    GenesisBlockInfo {
        genesis_block,
        mint_keypair,
        voting_keypair,
    }
}
