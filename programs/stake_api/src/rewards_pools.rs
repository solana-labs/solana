//! rewards_pools
//! * initialize genesis with rewards pools
//! * keep track of rewards
//! * own mining pools

use crate::stake_state::StakeState;
use rand::{thread_rng, Rng};
use solana_sdk::account::Account;
use solana_sdk::genesis_block::Builder;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;

// base rewards pool ID
const ID: [u8; 32] = [
    6, 161, 216, 23, 186, 139, 91, 88, 83, 34, 32, 112, 237, 188, 184, 153, 69, 67, 238, 112, 93,
    54, 133, 142, 145, 182, 214, 15, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(ID, "StakeRewards1111111111111111111111111111111");

// to cut down on collisions for redemptions, we make multiple accounts
pub const NUM_REWARDS_POOLS: usize = 256;

pub fn genesis(mut builder: Builder) -> Builder {
    let mut pubkey = id();

    for _i in 0..NUM_REWARDS_POOLS {
        builder = builder.rewards_pool(
            pubkey,
            Account::new_data(std::u64::MAX, &StakeState::RewardsPool, &crate::id()).unwrap(),
        );
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
