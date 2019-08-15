use solana_sdk::{
    account::Account,
    fee_calculator::FeeCalculator,
    genesis_block::{Builder, GenesisBlock},
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_program,
};
use solana_stake_api;
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

    // TODO: de-duplicate the stake once passive staking
    //  is fully implemented
    let (vote_account, vote_state) = vote_state::create_bootstrap_leader_account(
        &voting_keypair.pubkey(),
        &bootstrap_leader_pubkey,
        0,
        bootstrap_leader_stake_lamports,
    );

    let mut builder = Builder::new()
        .accounts(&[
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
            (
                staking_keypair.pubkey(),
                solana_stake_api::stake_state::create_stake_account(
                    &voting_keypair.pubkey(),
                    &vote_state,
                    bootstrap_leader_stake_lamports,
                ),
            ),
        ])
        // Bare minimum program set
        .native_instruction_processors(&[
            solana_bpf_loader_program!(),
            solana_vote_program!(),
            solana_stake_program!(),
        ])
        .fee_calculator(FeeCalculator::new(0)); // most tests don't want fees

    builder = solana_stake_api::genesis(builder);
    builder = solana_storage_api::rewards_pools::genesis(builder);

    GenesisBlockInfo {
        genesis_block: builder.build(),
        mint_keypair,
        voting_keypair,
    }
}
