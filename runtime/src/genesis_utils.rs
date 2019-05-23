use solana_sdk::account::Account;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_stake_api::stake_state;
use solana_vote_api::vote_state;

// The default stake placed with the bootstrap leader
pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 42;

pub struct GenesisBlockInfo {
    pub genesis_block: GenesisBlock,
    pub mint_keypair: Keypair,
    pub voting_keypair: Keypair,
}

pub fn create_genesis_block_with_leader(
    mint_lamports: u64,
    bootstrap_leader_id: &Pubkey,
    bootstrap_leader_stake_lamports: u64,
) -> GenesisBlockInfo {
    let mint_keypair = Keypair::new();
    let voting_keypair = Keypair::new();
    let staking_keypair = Keypair::new();

    // TODO: de-duplicate the stake once passive staking
    //  is fully implemented
    let (vote_account, vote_state) = vote_state::create_bootstrap_leader_account(
        &voting_keypair.pubkey(),
        &bootstrap_leader_id,
        0,
        bootstrap_leader_stake_lamports,
    );

    let genesis_block = GenesisBlock::new(
        &bootstrap_leader_id,
        &[
            // the mint
            (
                mint_keypair.pubkey(),
                Account::new(mint_lamports, 0, &system_program::id()),
            ),
            // node needs an account to issue votes from, this will require
            //  airdrops at some point to cover fees...
            (
                *bootstrap_leader_id,
                Account::new(1, 0, &system_program::id()),
            ),
            // where votes go to
            (voting_keypair.pubkey(), vote_account),
            // passive bootstrap leader stake, duplicates above temporarily
            (
                staking_keypair.pubkey(),
                stake_state::create_delegate_stake_account(
                    &voting_keypair.pubkey(),
                    &vote_state,
                    bootstrap_leader_stake_lamports,
                ),
            ),
        ],
        &[solana_vote_program!(), solana_stake_program!()],
    );

    GenesisBlockInfo {
        genesis_block,
        mint_keypair,
        voting_keypair,
    }
}
