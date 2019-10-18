pub use solana_runtime::genesis_utils::{
    create_genesis_block_with_leader, GenesisBlockInfo, BOOTSTRAP_LEADER_LAMPORTS,
};
use solana_sdk::pubkey::Pubkey;

// same as genesis_block::create_genesis_block, but with bootstrap_leader staking logic
//  for the core crate tests
pub fn create_genesis_block(mint_lamports: u64) -> GenesisBlockInfo {
    create_genesis_block_with_leader(
        mint_lamports,
        &Pubkey::new_rand(),
        BOOTSTRAP_LEADER_LAMPORTS,
    )
}
