//! rewards_pools
//! * initialize genesis with rewards pools
//! * keep track of rewards
//! * own mining pools

use crate::storage_contract::create_rewards_pool;
use rand::{thread_rng, Rng};
use solana_sdk::genesis_config::GenesisConfig;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;

// base rewards pool ID
solana_sdk::declare_id!("StorageMiningPoo111111111111111111111111111");

// to cut down on collisions for redemptions, we make multiple accounts
pub const NUM_REWARDS_POOLS: usize = 32;

pub fn add_genesis_accounts(genesis_config: &mut GenesisConfig) -> u64 {
    let mut pubkey = id();

    for _i in 0..NUM_REWARDS_POOLS {
        genesis_config.add_rewards_pool(pubkey, create_rewards_pool());
        pubkey = Pubkey::new(hash(pubkey.as_ref()).as_ref());
    }
    0 // didn't consume any lamports
}

pub fn random_id() -> Pubkey {
    let mut id = Hash::new(id().as_ref());

    for _i in 0..thread_rng().gen_range(0, NUM_REWARDS_POOLS) {
        id = hash(id.as_ref());
    }

    Pubkey::new(id.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let mut genesis_config = GenesisConfig::default();
        add_genesis_accounts(&mut genesis_config);

        for _i in 0..NUM_REWARDS_POOLS {
            assert!(genesis_config.rewards_pools.get(&random_id()).is_some())
        }
    }
}
