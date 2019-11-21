use solana_sdk::genesis_config::GenesisConfig;

pub mod config;
pub mod rewards_pools;
pub mod stake_instruction;
pub mod stake_state;

const STAKE_PROGRAM_ID: [u8; 32] = [
    6, 161, 216, 23, 145, 55, 84, 42, 152, 52, 55, 189, 254, 42, 122, 178, 85, 127, 83, 92, 138,
    120, 114, 43, 104, 164, 157, 192, 0, 0, 0, 0,
];

solana_sdk::declare_program!(
    STAKE_PROGRAM_ID,
    "Stake11111111111111111111111111111111111111",
    solana_stake_program,
    stake_instruction::process_instruction
);

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64 {
    for (pubkey, account) in rewards_pools::create_genesis_accounts() {
        genesis_config.add_rewards_pool(pubkey, account);
    }

    config::add_genesis_account(genesis_config)
}
