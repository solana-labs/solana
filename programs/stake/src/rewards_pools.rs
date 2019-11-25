//! rewards_pools
//! * initialize genesis with rewards pools
//! * keep track of rewards
//! * own mining pools
use crate::stake_state::StakeState;
use rand::{thread_rng, Rng};
use solana_sdk::{
    account::Account,
    hash::{hash, Hash},
    pubkey::Pubkey,
};

// base rewards pool ID
solana_sdk::declare_id!("StakeRewards1111111111111111111111111111111");

// to cut down on collisions for redemptions, we make multiple accounts
pub const NUM_REWARDS_POOLS: usize = 256;

pub fn random_id() -> Pubkey {
    let mut id = Hash::new(id().as_ref());

    for _i in 0..thread_rng().gen_range(0, NUM_REWARDS_POOLS) {
        id = hash(id.as_ref());
    }

    Pubkey::new(id.as_ref())
}

pub fn create_genesis_accounts() -> Vec<(Pubkey, Account)> {
    let mut accounts = Vec::with_capacity(NUM_REWARDS_POOLS);
    let mut pubkey = id();

    for _i in 0..NUM_REWARDS_POOLS {
        accounts.push((
            pubkey,
            Account::new_data(std::u64::MAX, &StakeState::RewardsPool, &crate::id()).unwrap(),
        ));
        pubkey = Pubkey::new(hash(pubkey.as_ref()).as_ref());
    }
    accounts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {
        let accounts = create_genesis_accounts();

        for _i in 0..NUM_REWARDS_POOLS {
            let id = random_id();
            assert!(accounts.iter().position(|x| x.0 == id).is_some());
        }
    }
}
