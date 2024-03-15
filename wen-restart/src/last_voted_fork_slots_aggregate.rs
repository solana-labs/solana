use {
    crate::solana::wen_restart_proto::LastVotedForkSlotsRecord,
    anyhow::Result,
    log::*,
    solana_gossip::restart_crds_values::RestartLastVotedForkSlots,
    solana_runtime::epoch_stakes::EpochStakes,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    std::{
        collections::{HashMap, HashSet},
        str::FromStr,
    },
};

pub struct LastVotedForkSlotsAggregate {
    root_slot: Slot,
    repair_threshold: f64,
    // TODO(wen): using local root's EpochStakes, need to fix if crossing Epoch boundary.
    epoch_stakes: EpochStakes,
    last_voted_fork_slots: HashMap<Pubkey, RestartLastVotedForkSlots>,
    slots_stake_map: HashMap<Slot, u64>,
    active_peers: HashSet<Pubkey>,
    slots_to_repair: HashSet<Slot>,
}

impl LastVotedForkSlotsAggregate {
    pub(crate) fn new(
        root_slot: Slot,
        repair_threshold: f64,
        epoch_stakes: &EpochStakes,
        last_voted_fork_slots: &Vec<Slot>,
        my_pubkey: &Pubkey,
    ) -> Self {
        let mut active_peers = HashSet::new();
        let sender_stake = Self::validator_stake(epoch_stakes, my_pubkey);
        active_peers.insert(*my_pubkey);
        let mut slots_stake_map = HashMap::new();
        for slot in last_voted_fork_slots {
            if slot > &root_slot {
                slots_stake_map.insert(*slot, sender_stake);
            }
        }
        Self {
            root_slot,
            repair_threshold,
            epoch_stakes: epoch_stakes.clone(),
            last_voted_fork_slots: HashMap::new(),
            slots_stake_map,
            active_peers,
            slots_to_repair: HashSet::new(),
        }
    }

    fn validator_stake(epoch_stakes: &EpochStakes, pubkey: &Pubkey) -> u64 {
        epoch_stakes
            .node_id_to_vote_accounts()
            .get(pubkey)
            .map(|x| x.total_stake)
            .unwrap_or_default()
    }

    pub(crate) fn aggregate_from_record(
        &mut self,
        key_string: &str,
        record: &LastVotedForkSlotsRecord,
    ) -> Result<Option<LastVotedForkSlotsRecord>> {
        let from = Pubkey::from_str(key_string)?;
        let last_voted_hash = Hash::from_str(&record.last_vote_bankhash)?;
        let converted_record = RestartLastVotedForkSlots::new(
            from,
            record.wallclock,
            &record.last_voted_fork_slots,
            last_voted_hash,
            record.shred_version as u16,
        )?;
        Ok(self.aggregate(converted_record))
    }

    pub(crate) fn aggregate(
        &mut self,
        new_slots: RestartLastVotedForkSlots,
    ) -> Option<LastVotedForkSlotsRecord> {
        let total_stake = self.epoch_stakes.total_stake();
        let threshold_stake = (total_stake as f64 * self.repair_threshold) as u64;
        let from = &new_slots.from;
        let sender_stake = Self::validator_stake(&self.epoch_stakes, from);
        if sender_stake == 0 {
            warn!(
                "Gossip should not accept zero-stake RestartLastVotedFork from {:?}",
                from
            );
            return None;
        }
        self.active_peers.insert(*from);
        let new_slots_vec = new_slots.to_slots(self.root_slot);
        let record = LastVotedForkSlotsRecord {
            last_voted_fork_slots: new_slots_vec.clone(),
            last_vote_bankhash: new_slots.last_voted_hash.to_string(),
            shred_version: new_slots.shred_version as u32,
            wallclock: new_slots.wallclock,
        };
        let new_slots_set: HashSet<Slot> = HashSet::from_iter(new_slots_vec);
        let old_slots_set = match self.last_voted_fork_slots.insert(*from, new_slots.clone()) {
            Some(old_slots) => {
                if old_slots == new_slots {
                    return None;
                } else {
                    HashSet::from_iter(old_slots.to_slots(self.root_slot))
                }
            }
            None => HashSet::new(),
        };
        for slot in old_slots_set.difference(&new_slots_set) {
            let entry = self.slots_stake_map.get_mut(slot).unwrap();
            *entry = entry.saturating_sub(sender_stake);
            if *entry < threshold_stake {
                self.slots_to_repair.remove(slot);
            }
        }
        for slot in new_slots_set.difference(&old_slots_set) {
            let entry = self.slots_stake_map.entry(*slot).or_insert(0);
            *entry = entry.saturating_add(sender_stake);
            if *entry >= threshold_stake {
                self.slots_to_repair.insert(*slot);
            }
        }
        Some(record)
    }

    pub(crate) fn active_percent(&self) -> f64 {
        let total_stake = self.epoch_stakes.total_stake();
        let total_active_stake = self.active_peers.iter().fold(0, |sum: u64, pubkey| {
            sum.saturating_add(Self::validator_stake(&self.epoch_stakes, pubkey))
        });
        total_active_stake as f64 / total_stake as f64 * 100.0
    }

    pub(crate) fn slots_to_repair_iter(&self) -> impl Iterator<Item = &Slot> {
        self.slots_to_repair.iter()
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            last_voted_fork_slots_aggregate::LastVotedForkSlotsAggregate,
            solana::wen_restart_proto::LastVotedForkSlotsRecord,
        },
        solana_gossip::restart_crds_values::RestartLastVotedForkSlots,
        solana_program::{clock::Slot, pubkey::Pubkey},
        solana_runtime::{
            bank::Bank,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{hash::Hash, signature::Signer, timing::timestamp},
    };

    const TOTAL_VALIDATOR_COUNT: u16 = 10;
    const MY_INDEX: usize = 9;
    const REPAIR_THRESHOLD: f64 = 0.42;
    const SHRED_VERSION: u16 = 52;

    struct TestAggregateInitResult {
        pub slots_aggregate: LastVotedForkSlotsAggregate,
        pub validator_voting_keypairs: Vec<ValidatorVoteKeypairs>,
        pub root_slot: Slot,
        pub last_voted_fork_slots: Vec<Slot>,
    }

    fn test_aggregate_init() -> TestAggregateInitResult {
        solana_logger::setup();
        let validator_voting_keypairs: Vec<_> = (0..TOTAL_VALIDATOR_COUNT)
            .map(|_| ValidatorVoteKeypairs::new_rand())
            .collect();
        let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
            10_000,
            &validator_voting_keypairs,
            vec![100; validator_voting_keypairs.len()],
        );
        let (_, bank_forks) = Bank::new_with_bank_forks_for_tests(&genesis_config);
        let root_bank = bank_forks.read().unwrap().root_bank();
        let root_slot = root_bank.slot();
        let last_voted_fork_slots = vec![
            root_slot.saturating_add(1),
            root_slot.saturating_add(2),
            root_slot.saturating_add(3),
        ];
        TestAggregateInitResult {
            slots_aggregate: LastVotedForkSlotsAggregate::new(
                root_slot,
                REPAIR_THRESHOLD,
                root_bank.epoch_stakes(root_bank.epoch()).unwrap(),
                &last_voted_fork_slots,
                &validator_voting_keypairs[MY_INDEX].node_keypair.pubkey(),
            ),
            validator_voting_keypairs,
            root_slot,
            last_voted_fork_slots,
        }
    }

    #[test]
    fn test_aggregate() {
        let mut test_state = test_aggregate_init();
        let root_slot = test_state.root_slot;
        let initial_num_active_validators = 3;
        for validator_voting_keypair in test_state
            .validator_voting_keypairs
            .iter()
            .take(initial_num_active_validators)
        {
            let pubkey = validator_voting_keypair.node_keypair.pubkey();
            let now = timestamp();
            assert_eq!(
                test_state.slots_aggregate.aggregate(
                    RestartLastVotedForkSlots::new(
                        pubkey,
                        now,
                        &test_state.last_voted_fork_slots,
                        Hash::default(),
                        SHRED_VERSION,
                    )
                    .unwrap(),
                ),
                Some(LastVotedForkSlotsRecord {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash: Hash::default().to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                }),
            );
        }
        assert_eq!(
            test_state.slots_aggregate.active_percent(),
            (initial_num_active_validators + 1) as f64 / TOTAL_VALIDATOR_COUNT as f64 * 100.0
        );
        assert!(test_state
            .slots_aggregate
            .slots_to_repair_iter()
            .next()
            .is_none());

        let new_active_validator = test_state.validator_voting_keypairs
            [initial_num_active_validators + 1]
            .node_keypair
            .pubkey();
        let now = timestamp();
        let new_active_validator_last_voted_slots = RestartLastVotedForkSlots::new(
            new_active_validator,
            now,
            &test_state.last_voted_fork_slots,
            Hash::default(),
            SHRED_VERSION,
        )
        .unwrap();
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate(new_active_validator_last_voted_slots),
            Some(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: now,
            }),
        );
        let expected_active_percent =
            (initial_num_active_validators + 2) as f64 / TOTAL_VALIDATOR_COUNT as f64 * 100.0;
        assert_eq!(
            test_state.slots_aggregate.active_percent(),
            expected_active_percent
        );
        let mut actual_slots =
            Vec::from_iter(test_state.slots_aggregate.slots_to_repair_iter().cloned());
        actual_slots.sort();
        assert_eq!(actual_slots, test_state.last_voted_fork_slots);

        let replace_message_validator = test_state.validator_voting_keypairs[2]
            .node_keypair
            .pubkey();
        // Allow specific validator to replace message.
        let now = timestamp();
        let replace_message_validator_last_fork = RestartLastVotedForkSlots::new(
            replace_message_validator,
            now,
            &[root_slot + 1, root_slot + 4, root_slot + 5],
            Hash::default(),
            SHRED_VERSION,
        )
        .unwrap();
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate(replace_message_validator_last_fork),
            Some(LastVotedForkSlotsRecord {
                last_voted_fork_slots: vec![root_slot + 1, root_slot + 4, root_slot + 5],
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: now,
            }),
        );
        assert_eq!(
            test_state.slots_aggregate.active_percent(),
            expected_active_percent
        );
        let mut actual_slots =
            Vec::from_iter(test_state.slots_aggregate.slots_to_repair_iter().cloned());
        actual_slots.sort();
        assert_eq!(actual_slots, vec![root_slot + 1]);

        // test that zero stake validator is ignored.
        let random_pubkey = Pubkey::new_unique();
        assert_eq!(
            test_state.slots_aggregate.aggregate(
                RestartLastVotedForkSlots::new(
                    random_pubkey,
                    timestamp(),
                    &[root_slot + 1, root_slot + 4, root_slot + 5],
                    Hash::default(),
                    SHRED_VERSION,
                )
                .unwrap(),
            ),
            None,
        );
        assert_eq!(
            test_state.slots_aggregate.active_percent(),
            expected_active_percent
        );
        let mut actual_slots =
            Vec::from_iter(test_state.slots_aggregate.slots_to_repair_iter().cloned());
        actual_slots.sort();
        assert_eq!(actual_slots, vec![root_slot + 1]);
    }

    #[test]
    fn test_aggregate_from_record() {
        let mut test_state = test_aggregate_init();
        let root_slot = test_state.root_slot;
        let last_vote_bankhash = Hash::new_unique();
        let time1 = timestamp();
        let record = LastVotedForkSlotsRecord {
            wallclock: time1,
            last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
            last_vote_bankhash: last_vote_bankhash.to_string(),
            shred_version: SHRED_VERSION as u32,
        };
        assert_eq!(test_state.slots_aggregate.active_percent(), 10.0);
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate_from_record(
                    &test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey()
                        .to_string(),
                    &record,
                )
                .unwrap(),
            Some(record.clone()),
        );
        assert_eq!(test_state.slots_aggregate.active_percent(), 20.0);
        // Now if you get the same result from Gossip again, it should be ignored.
        assert_eq!(
            test_state.slots_aggregate.aggregate(
                RestartLastVotedForkSlots::new(
                    test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey(),
                    time1,
                    &test_state.last_voted_fork_slots,
                    last_vote_bankhash,
                    SHRED_VERSION,
                )
                .unwrap(),
            ),
            None,
        );

        // But if it's a new record from the same validator, it will be replaced.
        let time2 = timestamp();
        let last_voted_fork_slots2 =
            vec![root_slot + 1, root_slot + 2, root_slot + 3, root_slot + 4];
        let last_vote_bankhash2 = Hash::new_unique();
        assert_eq!(
            test_state.slots_aggregate.aggregate(
                RestartLastVotedForkSlots::new(
                    test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey(),
                    time2,
                    &last_voted_fork_slots2,
                    last_vote_bankhash2,
                    SHRED_VERSION,
                )
                .unwrap(),
            ),
            Some(LastVotedForkSlotsRecord {
                wallclock: time2,
                last_voted_fork_slots: last_voted_fork_slots2.clone(),
                last_vote_bankhash: last_vote_bankhash2.to_string(),
                shred_version: SHRED_VERSION as u32,
            }),
        );
        // percentage doesn't change since it's a replace.
        assert_eq!(test_state.slots_aggregate.active_percent(), 20.0);

        // Record from validator with zero stake should be ignored.
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate_from_record(
                    &Pubkey::new_unique().to_string(),
                    &LastVotedForkSlotsRecord {
                        wallclock: timestamp(),
                        last_voted_fork_slots: vec![root_slot + 10, root_slot + 300],
                        last_vote_bankhash: Hash::new_unique().to_string(),
                        shred_version: SHRED_VERSION as u32,
                    }
                )
                .unwrap(),
            None,
        );
        // percentage doesn't change since the previous aggregate is ignored.
        assert_eq!(test_state.slots_aggregate.active_percent(), 20.0);
    }

    #[test]
    fn test_aggregate_from_record_failures() {
        solana_logger::setup();
        let mut test_state = test_aggregate_init();
        let last_vote_bankhash = Hash::new_unique();
        let mut last_voted_fork_slots_record = LastVotedForkSlotsRecord {
            wallclock: timestamp(),
            last_voted_fork_slots: test_state.last_voted_fork_slots,
            last_vote_bankhash: last_vote_bankhash.to_string(),
            shred_version: SHRED_VERSION as u32,
        };
        // First test that this is a valid record.
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate_from_record(
                    &test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey()
                        .to_string(),
                    &last_voted_fork_slots_record,
                )
                .unwrap(),
            Some(last_voted_fork_slots_record.clone()),
        );
        // Then test that it fails if the record is invalid.

        // Invalid pubkey.
        assert!(test_state
            .slots_aggregate
            .aggregate_from_record("invalid_pubkey", &last_voted_fork_slots_record,)
            .is_err());

        // Invalid hash.
        last_voted_fork_slots_record.last_vote_bankhash.clear();
        assert!(test_state
            .slots_aggregate
            .aggregate_from_record(
                &test_state.validator_voting_keypairs[0]
                    .node_keypair
                    .pubkey()
                    .to_string(),
                &last_voted_fork_slots_record,
            )
            .is_err());
        last_voted_fork_slots_record.last_vote_bankhash.pop();

        // Empty last voted fork.
        last_voted_fork_slots_record.last_vote_bankhash = last_vote_bankhash.to_string();
        last_voted_fork_slots_record.last_voted_fork_slots.clear();
        assert!(test_state
            .slots_aggregate
            .aggregate_from_record(
                &test_state.validator_voting_keypairs[0]
                    .node_keypair
                    .pubkey()
                    .to_string(),
                &last_voted_fork_slots_record,
            )
            .is_err());
    }
}
