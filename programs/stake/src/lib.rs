use crate::config::create_genesis_account;
use crate::rewards_pools::create_rewards_accounts;
use crate::stake_instruction::process_instruction;
use solana_sdk::genesis_config::GenesisConfig;

pub mod config;
pub mod rewards_pools;
pub mod stake_instruction;
pub mod stake_state;

solana_sdk::declare_program!(
    "Stake11111111111111111111111111111111111111",
    solana_stake_program,
    process_instruction
);

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) {
    for (pubkey, account) in create_rewards_accounts() {
        genesis_config.add_rewards_pool(pubkey, account);
    }

    let (pubkey, account) = create_genesis_account();
    genesis_config.add_account(pubkey, account);
}
