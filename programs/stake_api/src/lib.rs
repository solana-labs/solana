use crate::config::create_genesis_account;
use crate::rewards_pools::create_rewards_accounts;
use solana_sdk::genesis_config::GenesisConfig;

pub mod config;
pub mod rewards_pools;
pub mod stake_instruction;
pub mod stake_state;

const STAKE_PROGRAM_ID: [u8; 32] = [
    6, 161, 216, 23, 145, 55, 84, 42, 152, 52, 55, 189, 254, 42, 122, 178, 85, 127, 83, 92, 138,
    120, 114, 43, 104, 164, 157, 192, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    STAKE_PROGRAM_ID,
    "Stake11111111111111111111111111111111111111"
);

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) {
    for (pubkey, account) in create_rewards_accounts() {
        genesis_config.add_rewards_pool(pubkey, account);
    }

    let (pubkey, account) = create_genesis_account();
    genesis_config.add_account(pubkey, account);
}
