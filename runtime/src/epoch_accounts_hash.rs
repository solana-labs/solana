//! The Epoch Accounts Hash (EAH) is a special hash of the whole accounts state that occurs once
//! per epoch.
//!
//! This hash is special because all nodes in the cluster will calculate the accounts hash at a
//! predetermined slot in the epoch and then save that result into a later Bank at a predetermined
//! slot.
//!
//! This results in all nodes effectively voting on the accounts state (at least) once per epoch.

use {
    crate::bank::Bank,
    serde::{Deserialize, Serialize},
    solana_sdk::{clock::Slot, hash::Hash},
};

/// The EpochAccountsHash holds the result after calculating the accounts hash once per epoch
#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Eq, Clone, Copy)]
pub struct EpochAccountsHash(Hash);

impl AsRef<Hash> for EpochAccountsHash {
    fn as_ref(&self) -> &Hash {
        &self.0
    }
}

/// Calculation of the EAH occurs once per epoch.  All nodes in the cluster must agree on which
/// slot the EAH is based on.  This slot will be at an offset into the epoch, and referred to as
/// the "start" slot for the EAH calculation.
#[must_use]
#[allow(dead_code)]
pub fn calculation_offset_start(bank: &Bank) -> Slot {
    let slots_per_epoch = bank.epoch_schedule().slots_per_epoch;
    slots_per_epoch / 4
}

/// Calculation of the EAH occurs once per epoch.  All nodes in the cluster must agree on which
/// bank will hash the EAH into its `Bank::hash`.  This slot will be at an offset into the epoch,
/// and referred to as the "stop" slot for the EAH calculation.  All nodes must complete the EAH
/// calculation before this slot!
#[must_use]
#[allow(dead_code)]
pub fn calculation_offset_stop(bank: &Bank) -> Slot {
    let slots_per_epoch = bank.epoch_schedule().slots_per_epoch;
    slots_per_epoch / 4 * 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculation_offset_bounds() {
        let bank = Bank::default_for_tests();
        let start = calculation_offset_start(&bank);
        let stop = calculation_offset_stop(&bank);
        assert!(start < stop);
    }
}
