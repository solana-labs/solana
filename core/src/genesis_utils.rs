pub use solana_runtime::genesis_utils::{
    create_genesis_block_with_leader, BOOTSTRAP_LEADER_LAMPORTS,
};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;

// same as genesis_block::create_genesis_block, but with bootstrap_leader staking logic
//  for the core crate tests
pub fn create_genesis_block(mint_lamports: u64) -> (GenesisBlock, Keypair) {
    let (genesis_block, mint_keypair, _vote_keypair) = create_genesis_block_with_leader(
        mint_lamports,
        &Pubkey::new_rand(),
        BOOTSTRAP_LEADER_LAMPORTS,
    );
    (genesis_block, mint_keypair)
}
