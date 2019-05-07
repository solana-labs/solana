use solana_sdk::account::Account;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_program;
use solana_vote_api::vote_state;

// The default stake placed with the bootstrap leader
pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 42;

pub fn create_genesis_block_with_leader(
    mint_lamports: u64,
    leader_id: &Pubkey,
    leader_stake_lamports: u64,
) -> (GenesisBlock, Keypair, Keypair) {
    let mint_keypair = Keypair::new();
    let voting_keypair = Keypair::new();

    let genesis_block = GenesisBlock::new(
        &leader_id,
        &[
            (
                mint_keypair.pubkey(),
                Account::new(mint_lamports, 0, &system_program::id()),
            ),
            (
                voting_keypair.pubkey(),
                vote_state::create_bootstrap_leader_account(
                    &voting_keypair.pubkey(),
                    &leader_id,
                    0,
                    leader_stake_lamports,
                ),
            ),
        ],
        &[],
    );

    (genesis_block, mint_keypair, voting_keypair)
}

// same as genesis_block::create_genesis_block, but with leader staking logic specific
//  to this crate
pub fn create_genesis_block(mint_lamports: u64) -> (GenesisBlock, Keypair) {
    let (genesis_block, mint_keypair, _vote_keypair) = create_genesis_block_with_leader(
        mint_lamports,
        &Pubkey::new_rand(),
        BOOTSTRAP_LEADER_LAMPORTS,
    );
    (genesis_block, mint_keypair)
}
