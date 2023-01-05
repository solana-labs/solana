use {
    crate::cluster_info_vote_listener::VoteTracker,
    solana_ledger::blockstore::Blockstore,
    solana_runtime::bank::Bank,
    solana_sdk::{clock::Slot, hash::Hash, timing::timestamp},
    std::{collections::BTreeSet, time::Instant},
};

pub struct OptimisticConfirmationVerifier {
    snapshot_start_slot: Slot,
    unchecked_slots: BTreeSet<(Slot, Hash)>,
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
    pub fn verify_for_unrooted_optimistic_slots(
        &mut self,
        root_bank: &Bank,
        blockstore: &Blockstore,
    ) -> Vec<(Slot, Hash)> {
        let root = root_bank.slot();
        let root_ancestors = &root_bank.ancestors;
        let slots_after_root = self
            .unchecked_slots
            .split_off(&((root + 1), Hash::default()));
        // `slots_before_root` now contains all slots <= root
        let slots_before_root = std::mem::replace(&mut self.unchecked_slots, slots_after_root);
        slots_before_root
            .into_iter()
            .filter(|(optimistic_slot, optimistic_hash)| {
                (*optimistic_slot == root && *optimistic_hash != root_bank.hash())
                    || (!root_ancestors.contains_key(optimistic_slot) &&
                    // In this second part of the `and`, we account for the possibility that
                    // there was some other root `rootX` set in BankForks where:
                    //
                    // `root` > `rootX` > `optimistic_slot`
                    //
                    // in which case `root` may  not contain the ancestor information for
                    // slots < `rootX`, so we also have to check if `optimistic_slot` was rooted
                    // through blockstore.
                    !blockstore.is_root(*optimistic_slot))
            })
            .collect()
    }

    pub fn add_new_optimistic_confirmed_slots(
        &mut self,
        new_optimistic_slots: Vec<(Slot, Hash)>,
        blockstore: &Blockstore,
    ) {
        if new_optimistic_slots.is_empty() {
            return;
        }

        datapoint_info!(
            "optimistic_slot_elapsed",
            (
                "average_elapsed_ms",
                self.last_optimistic_slot_ts.elapsed().as_millis() as i64,
                i64
            ),
        );

        // We don't have any information about ancestors before the snapshot root,
        // so ignore those slots
        for (new_optimistic_slot, hash) in new_optimistic_slots {
            if new_optimistic_slot > self.snapshot_start_slot {
                if let Err(e) = blockstore.insert_optimistic_slot(
                    new_optimistic_slot,
                    &hash,
                    timestamp().try_into().unwrap(),
                ) {
                    error!(
                        "failed to record optimistic slot in blockstore: slot={}: {:?}",
                        new_optimistic_slot, &e
                    );
                }
                datapoint_info!("optimistic_slot", ("slot", new_optimistic_slot, i64),);
                self.unchecked_slots.insert((new_optimistic_slot, hash));
            }
        }

        self.last_optimistic_slot_ts = Instant::now();
    }

    pub fn format_optimistic_confirmed_slot_violation_log(slot: Slot) -> String {
        format!("Optimistically confirmed slot {slot} was not rooted")
    }

    pub fn log_unrooted_optimistic_slots(
        root_bank: &Bank,
        vote_tracker: &VoteTracker,
        unrooted_optimistic_slots: &[(Slot, Hash)],
    ) {
        let root = root_bank.slot();
        for (optimistic_slot, hash) in unrooted_optimistic_slots.iter() {
            let epoch = root_bank.epoch_schedule().get_epoch(*optimistic_slot);
            let epoch_stakes = root_bank.epoch_stakes(epoch);
            let total_epoch_stake = epoch_stakes.map(|e| e.total_stake()).unwrap_or(0);
            let voted_stake = {
                let slot_tracker = vote_tracker.get_slot_vote_tracker(*optimistic_slot);
                let r_slot_tracker = slot_tracker.as_ref().map(|s| s.read().unwrap());
                let voted_stake = r_slot_tracker
                    .as_ref()
                    .and_then(|s| s.optimistic_votes_tracker(hash))
                    .map(|s| s.stake())
                    .unwrap_or(0);

                error!(
                    "{},
                    hash: {},
                    epoch: {},
                    voted keys: {:?},
                    root: {},
                    root bank hash: {},
                    voted stake: {},
                    total epoch stake: {},
                    pct: {}",
                    Self::format_optimistic_confirmed_slot_violation_log(*optimistic_slot),
                    hash,
                    epoch,
                    r_slot_tracker
                        .as_ref()
                        .and_then(|s| s.optimistic_votes_tracker(hash))
                        .map(|s| s.voted()),
                    root,
                    root_bank.hash(),
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
    use {
        super::*, crate::vote_simulator::VoteSimulator,
        solana_ledger::get_tmp_ledger_path_auto_delete, solana_runtime::bank::Bank,
        solana_sdk::pubkey::Pubkey, std::collections::HashMap, trees::tr,
    };

    #[test]
    fn test_add_new_optimistic_confirmed_slots() {
        let snapshot_start_slot = 10;
        let bank_hash = Hash::default();
        let mut optimistic_confirmation_verifier =
            OptimisticConfirmationVerifier::new(snapshot_start_slot);
        let blockstore_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(blockstore_path.path()).unwrap();
        optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(
            vec![(snapshot_start_slot - 1, bank_hash)],
            &blockstore,
        );
        assert_eq!(blockstore.get_latest_optimistic_slots(10).unwrap().len(), 0);
        optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(
            vec![(snapshot_start_slot, bank_hash)],
            &blockstore,
        );
        assert_eq!(blockstore.get_latest_optimistic_slots(10).unwrap().len(), 0);
        optimistic_confirmation_verifier.add_new_optimistic_confirmed_slots(
            vec![(snapshot_start_slot + 1, bank_hash)],
            &blockstore,
        );
        assert_eq!(blockstore.get_latest_optimistic_slots(10).unwrap().len(), 1);
        assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
        assert!(optimistic_confirmation_verifier
            .unchecked_slots
            .contains(&(snapshot_start_slot + 1, bank_hash)));
    }

    #[test]
    fn test_get_unrooted_optimistic_slots_same_slot_different_hash() {
        let snapshot_start_slot = 0;
        let mut optimistic_confirmation_verifier =
            OptimisticConfirmationVerifier::new(snapshot_start_slot);
        let bad_bank_hash = Hash::new(&[42u8; 32]);
        let blockstore_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(blockstore_path.path()).unwrap();
        let optimistic_slots = vec![(1, bad_bank_hash), (3, Hash::default())];
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(optimistic_slots, &blockstore);
        assert_eq!(blockstore.get_latest_optimistic_slots(10).unwrap().len(), 2);
        let vote_simulator = setup_forks();
        let bank1 = vote_simulator.bank_forks.read().unwrap().get(1).unwrap();
        assert_eq!(
            optimistic_confirmation_verifier
                .verify_for_unrooted_optimistic_slots(&bank1, &blockstore),
            vec![(1, bad_bank_hash)]
        );
        assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
        assert!(optimistic_confirmation_verifier
            .unchecked_slots
            .contains(&(3, Hash::default())));
    }

    #[test]
    fn test_get_unrooted_optimistic_slots() {
        let snapshot_start_slot = 0;
        let mut optimistic_confirmation_verifier =
            OptimisticConfirmationVerifier::new(snapshot_start_slot);
        let blockstore_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(blockstore_path.path()).unwrap();
        let mut vote_simulator = setup_forks();
        let optimistic_slots: Vec<_> = vec![1, 3, 5]
            .into_iter()
            .map(|s| {
                (
                    s,
                    vote_simulator
                        .bank_forks
                        .read()
                        .unwrap()
                        .get(s)
                        .unwrap()
                        .hash(),
                )
            })
            .collect();

        // If root is on same fork, nothing should be returned
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(optimistic_slots.clone(), &blockstore);
        assert_eq!(blockstore.get_latest_optimistic_slots(10).unwrap().len(), 3);
        let bank5 = vote_simulator.bank_forks.read().unwrap().get(5).unwrap();
        assert!(optimistic_confirmation_verifier
            .verify_for_unrooted_optimistic_slots(&bank5, &blockstore)
            .is_empty());
        // 5 is >= than all the unchecked slots, so should clear everything
        assert!(optimistic_confirmation_verifier.unchecked_slots.is_empty());

        // If root is on same fork, nothing should be returned
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(optimistic_slots.clone(), &blockstore);
        let bank3 = vote_simulator.bank_forks.read().unwrap().get(3).unwrap();
        assert!(optimistic_confirmation_verifier
            .verify_for_unrooted_optimistic_slots(&bank3, &blockstore)
            .is_empty());
        // 3 is bigger than only slot 1, so slot 5 should be left over
        assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
        assert!(optimistic_confirmation_verifier
            .unchecked_slots
            .contains(&optimistic_slots[2]));

        // If root is on different fork, the slots < root on different fork should
        // be returned
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(optimistic_slots.clone(), &blockstore);
        let bank4 = vote_simulator.bank_forks.read().unwrap().get(4).unwrap();
        assert_eq!(
            optimistic_confirmation_verifier
                .verify_for_unrooted_optimistic_slots(&bank4, &blockstore),
            vec![optimistic_slots[1]]
        );
        // 4 is bigger than only slots 1 and 3, so slot 5 should be left over
        assert_eq!(optimistic_confirmation_verifier.unchecked_slots.len(), 1);
        assert!(optimistic_confirmation_verifier
            .unchecked_slots
            .contains(&optimistic_slots[2]));

        // Now set a root at slot 5, purging BankForks of slots < 5
        vote_simulator.set_root(5);

        // Add a new bank 7 that descends from 6
        let bank6 = vote_simulator.bank_forks.read().unwrap().get(6).unwrap();
        vote_simulator
            .bank_forks
            .write()
            .unwrap()
            .insert(Bank::new_from_parent(&bank6, &Pubkey::default(), 7));
        let bank7 = vote_simulator.bank_forks.read().unwrap().get(7).unwrap();
        assert!(!bank7.ancestors.contains_key(&3));

        // Should return slots 1, 3 as part of the rooted fork because there's no
        // ancestry information
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(optimistic_slots.clone(), &blockstore);
        assert_eq!(
            optimistic_confirmation_verifier
                .verify_for_unrooted_optimistic_slots(&bank7, &blockstore),
            optimistic_slots[0..=1].to_vec()
        );
        assert!(optimistic_confirmation_verifier.unchecked_slots.is_empty());

        // If we know set the root in blockstore, should return nothing
        blockstore.set_roots(vec![1, 3].iter()).unwrap();
        optimistic_confirmation_verifier
            .add_new_optimistic_confirmed_slots(optimistic_slots, &blockstore);
        assert!(optimistic_confirmation_verifier
            .verify_for_unrooted_optimistic_slots(&bank7, &blockstore)
            .is_empty());
        assert!(optimistic_confirmation_verifier.unchecked_slots.is_empty());
        assert_eq!(blockstore.get_latest_optimistic_slots(10).unwrap().len(), 3);
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
        vote_simulator.fill_bank_forks(forks, &HashMap::new(), true);
        vote_simulator
    }
}
