use crate::cluster_info_vote_listener::VoteTracker;
use solana_ledger::blockstore::Blockstore;
use solana_runtime::bank::Bank;
use solana_sdk::clock::Slot;
use std::{collections::BTreeSet, time::Instant};

pub struct OptimisticConfirmationVerifier {
    snapshot_start_slot: Slot,
    unchecked_slots: BTreeSet<Slot>,
    last_optimistic_slot_ts: Instant,
}

impl OptimisticConfirmationVerifier {
    pub fn new(snapshot_start_slot: Slot) -> Self {
        Self {
            snapshot_start_slot,
            unchecked_slots: BTreeSet::default(),
            last_optimistic_slot_ts: Instant::now(),
        }
    }

    // Returns any optimistic slots that were not rooted
    pub fn check_optimistic_slots_rooted(
        &mut self,
        root_bank: &Bank,
        blockstore: &Blockstore,
    ) -> Vec<Slot> {
        let root = root_bank.slot();
        let root_ancestors = &root_bank.ancestors;
        let mut slots_before_root = self.unchecked_slots.split_off(&(root + 1));
        // `slots_before_root` now contains all slots <= root
        std::mem::swap(&mut slots_before_root, &mut self.unchecked_slots);
        slots_before_root
            .into_iter()
            .filter(|optimistic_slot| {
                !root_ancestors.contains_key(&optimistic_slot) &&
            // In this second part of the `and`, we account for the possibility that 
            // there was some other root `rootX` set in BankForks where:
            //
            // `root` > `rootX` > `optimistic_slot`
            //
            // in which case `root` may  not contain the ancestor information for
            // slots < `rootX`, so we also have to check if `optimistic_slot` was rooted
            // through blockstore.
            !blockstore.is_root(*optimistic_slot)
            })
            .collect()
    }

    pub fn add_new_optimistic_confirmed_slots(&mut self, new_optimistic_slots: &[Slot]) {
        if new_optimistic_slots.is_empty() {
            return;
        }

        datapoint_info!(
            "optimistic_slot_elapsed",
            (
                "average_elapsed_ms",
                self.last_optimistic_slot_ts.elapsed().as_millis() as i64
                    / new_optimistic_slots.len() as i64,
                i64
            ),
        );

        // We don't have any information about ancestors before the snapshot root,
        // so ignore those slots
        for new_optimistic_slot in new_optimistic_slots {
            if *new_optimistic_slot > self.snapshot_start_slot {
                datapoint_warn!("optimistic_slot", ("slot", *new_optimistic_slot, i64),);
                self.unchecked_slots.insert(*new_optimistic_slot);
            }
        }

        self.last_optimistic_slot_ts = Instant::now();
    }

    pub fn log_unrooted_optimistic_slots(
        root_bank: &Bank,
        vote_tracker: &VoteTracker,
        unrooted_optimistic_slots: &[Slot],
    ) {
        let root = root_bank.slot();
        for optimistic_slot in unrooted_optimistic_slots.iter() {
            let epoch = root_bank.epoch_schedule().get_epoch(*optimistic_slot);
            let epoch_stakes = root_bank.epoch_stakes(epoch);
            let total_epoch_stake = epoch_stakes.map(|e| e.total_stake()).unwrap_or(0);
            let voted_stake = {
                let slot_tracker = vote_tracker.get_slot_vote_tracker(*optimistic_slot);
                let r_slot_tracker = slot_tracker.as_ref().map(|s| s.read().unwrap());
                let voted_stake = r_slot_tracker
                    .as_ref()
                    .map(|s| s.voted_no_switching().stake())
                    .unwrap_or(0);

                warn!(
                    "Optimistic slot {}, epoch: {} was not rooted, voted keys: {:?}, root: {}, voted stake: {},
                    total epoch stake: {}, pct: {}",
                    optimistic_slot,
                    epoch,
                    r_slot_tracker.as_ref().map(|s| s.voted_no_switching().voted()),
                    root,
                    voted_stake,
                    total_epoch_stake,
                    voted_stake as f64 / total_epoch_stake as f64,
                );
                voted_stake
            };

            datapoint_warn!(
                "optimistic_slot_not_rooted",
                ("slot", *optimistic_slot, i64),
                ("epoch", epoch, i64),
                ("root", root, i64),
                ("voted_stake", voted_stake, i64),
                ("total_epoch_stake", total_epoch_stake, i64),
            );
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::consensus::test::VoteSimulator;
    use solana_ledger::get_tmp_ledger_path;
    use solana_runtime::bank::Bank;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;
    use trees::tr;

    #[test]
    fn test_add_new_optimistic_confirmed_slots() {
        let snapshot_start_slot = 10;
        let mut optimistic_confirmation_verifier =
            OptimisticConfirmationVerifier::new(snapshot_start_slot);
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(&[snapshot_start_slot - 1]);
        optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(&[snapshot_start_slot]);
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(&[snapshot_start_slot + 1]);
        assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
        assert!(optimistic_confirmation_verifier
            .unchecked_slots
            .contains(&(snapshot_start_slot + 1)));
    }

    #[test]
    fn test_check_optimistic_slots_rooted() {
        let snapshot_start_slot = 0;
        let mut optimistic_confirmation_verifier =
            OptimisticConfirmationVerifier::new(snapshot_start_slot);
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();
            let mut vote_simulator = setup_forks();
            let optimistic_slots = vec![1, 3, 5];

            // If root is on same fork, nothing should be returned
            optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(&optimistic_slots);
            let bank5 = vote_simulator
                .bank_forks
                .read()
                .unwrap()
                .get(5)
                .cloned()
                .unwrap();
            assert!(optimistic_confirmation_verifier
                .check_optimistic_slots_rooted(&bank5, &blockstore)
                .is_empty());
            // 5 is >= than all the unchecked slots, so should clear everything
            assert!(optimistic_confirmation_verifier.unchecked_slots.is_empty());

            // If root is on same fork, nothing should be returned
            optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(&optimistic_slots);
            let bank3 = vote_simulator
                .bank_forks
                .read()
                .unwrap()
                .get(3)
                .cloned()
                .unwrap();
            assert!(optimistic_confirmation_verifier
                .check_optimistic_slots_rooted(&bank3, &blockstore)
                .is_empty());
            // 3 is bigger than only slot 1, so slot 5 should be left over
            assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
            assert!(optimistic_confirmation_verifier
                .unchecked_slots
                .contains(&5));

            // If root is on different fork, the slots < root on different fork should
            // be returned
            optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(&optimistic_slots);
            let bank4 = vote_simulator
                .bank_forks
                .read()
                .unwrap()
                .get(4)
                .cloned()
                .unwrap();
            assert_eq!(
                optimistic_confirmation_verifier.check_optimistic_slots_rooted(&bank4, &blockstore),
                vec![3]
            );
            // 4 is bigger than only slots 1 and 3, so slot 5 should be left over
            assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
            assert!(optimistic_confirmation_verifier
                .unchecked_slots
                .contains(&5));

            // Now set a root at slot 5, purging BankForks of slots < 5
            vote_simulator.set_root(5);

            // Add a new bank 7 that descends from 6
            let bank6 = vote_simulator
                .bank_forks
                .read()
                .unwrap()
                .get(6)
                .cloned()
                .unwrap();
            vote_simulator
                .bank_forks
                .write()
                .unwrap()
                .insert(Bank::new_from_parent(&bank6, &Pubkey::default(), 7));
            let bank7 = vote_simulator
                .bank_forks
                .read()
                .unwrap()
                .get(7)
                .unwrap()
                .clone();
            assert!(!bank7.ancestors.contains_key(&3));

            // Should return slots 1, 3 as part of the rooted fork because there's no
            // ancestry information
            optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(&optimistic_slots);
            assert_eq!(
                optimistic_confirmation_verifier.check_optimistic_slots_rooted(&bank7, &blockstore),
                vec![1, 3]
            );
            assert!(optimistic_confirmation_verifier.unchecked_slots.is_empty());

            // If we know set the root in blockstore, should return nothing
            blockstore.set_roots(&[1, 3]).unwrap();
            optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(&optimistic_slots);
            assert!(optimistic_confirmation_verifier
                .check_optimistic_slots_rooted(&bank7, &blockstore)
                .is_empty());
            assert!(optimistic_confirmation_verifier.unchecked_slots.is_empty());
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    fn setup_forks() -> VoteSimulator {
        /*
            Build fork structure:
                 slot 0
                   |
                 slot 1
                 /    \
            slot 2    |
               |    slot 3
            slot 4    |
                    slot 5
                      |
                    slot 6
        */
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(4))) / (tr(3) / (tr(5) / (tr(6)))));

        let mut vote_simulator = VoteSimulator::new(1);
        vote_simulator.fill_bank_forks(forks, &HashMap::new());
        vote_simulator
    }
}
