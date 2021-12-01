//! Map pubkeys to stake delegations
//!
//! This module implements clone-on-write semantics for `StakeDelegations` to reduce unnecessary
//! cloning of the underlying map.
use {
    solana_sdk::{pubkey::Pubkey, stake::state::Delegation},
    std::{
        collections::HashMap,
        ops::{Deref, DerefMut},
        sync::Arc,
    },
};

/// A map of pubkey-to-stake-delegation with clone-on-write semantics
#[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize, AbiExample)]
pub struct StakeDelegations(Arc<StakeDelegationsInner>);

impl Deref for StakeDelegations {
    type Target = StakeDelegationsInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for StakeDelegations {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

/// The inner type, which maps pubkeys to stake delegations
type StakeDelegationsInner = HashMap<Pubkey, Delegation>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stake_delegations_is_cow() {
        let voter_pubkey = Pubkey::new_unique();
        let stake = rand::random();
        let activation_epoch = rand::random();
        let warmup_cooldown_rate = rand::random();
        let delegation =
            Delegation::new(&voter_pubkey, stake, activation_epoch, warmup_cooldown_rate);

        let pubkey = Pubkey::new_unique();

        let mut stake_delegations = StakeDelegations::default();
        stake_delegations.insert(pubkey, delegation);

        // Test: Clone the stake delegations and **do not modify**.  Assert the underlying maps are
        // the same instance.
        {
            let stake_delegations2 = stake_delegations.clone();
            assert_eq!(stake_delegations, stake_delegations2);
            assert!(
                Arc::ptr_eq(&stake_delegations.0, &stake_delegations2.0),
                "Inner Arc must point to the same HashMap"
            );
            assert!(
                std::ptr::eq(stake_delegations.deref(), stake_delegations2.deref()),
                "Deref must point to the same HashMap"
            );
        }

        // Test: Clone the stake delegations and then modify (remove the K-V, then re-add the same
        // one, so the stake delegations are still logically equal).  Assert the underlying maps
        // are unique instances.
        {
            let mut stake_delegations2 = stake_delegations.clone();
            stake_delegations2.clear();
            assert_ne!(stake_delegations, stake_delegations2);
            stake_delegations2.insert(pubkey, delegation);
            assert_eq!(stake_delegations, stake_delegations2);
            assert!(
                !Arc::ptr_eq(&stake_delegations.0, &stake_delegations2.0),
                "Inner Arc must point to different HashMaps"
            );
            assert!(
                !std::ptr::eq(stake_delegations.deref(), stake_delegations2.deref()),
                "Deref must point to different HashMaps"
            );
        }
    }
}
