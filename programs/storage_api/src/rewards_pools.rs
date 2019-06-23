//! rewards_pools
//! * initialize genesis with rewards pools
//! * keep track of rewards
//! * own mining pools

use crate::storage_contract::create_rewards_pool;
use rand::{thread_rng, Rng};
use solana_sdk::genesis_block::Builder;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;

// base rewards pool ID
const ID: [u8; 32] = [
    6, 162, 25, 123, 127, 71, 141, 232, 129, 171, 58, 183, 79, 88, 181, 17, 163, 11, 51, 111, 22,
    123, 67, 115, 5, 131, 109, 161, 16, 0, 0, 0,
];

solana_sdk::solana_name_id!(ID, "StorageMiningPoo111111111111111111111111111");

// to cut down on collisions for redemptions, we make multiple accounts
pub const NUM_REWARDS_POOLS: usize = 32;

pub fn genesis(mut builder: Builder) -> Builder {
    let mut pubkey = id();

    for _i in 0..NUM_REWARDS_POOLS {
        builder = builder.rewards_pool(pubkey, create_rewards_pool());
        pubkey = Pubkey::new(hash(pubkey.as_ref()).as_ref());
    }
    builder
}

pub fn random_id() -> Pubkey {
    let mut id = Hash::new(&ID);

    for _i in 0..thread_rng().gen_range(0, NUM_REWARDS_POOLS) {
        id = hash(id.as_ref());
    }

    Pubkey::new(id.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::genesis_block::Builder;

    #[test]
    fn test() {
        let builder = Builder::new();

        let genesis_block = genesis(builder).build();

        for _i in 0..NUM_REWARDS_POOLS {
            let id = random_id();
            assert!(genesis_block
                .rewards_pools
                .iter()
                .position(|x| x.0 == id)
                .is_some());
        }
    }
}
