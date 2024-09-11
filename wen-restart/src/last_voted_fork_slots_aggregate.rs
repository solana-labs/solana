use {
    crate::solana::wen_restart_proto::LastVotedForkSlotsRecord,
    anyhow::Result,
    log::*,
    solana_gossip::restart_crds_values::RestartLastVotedForkSlots,
    solana_runtime::bank::Bank,
    solana_sdk::{
        clock::{Epoch, Slot},
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        collections::{BTreeSet, HashMap},
        str::FromStr,
        sync::Arc,
    },
};

// If at least 1/3 of the stake has voted for any slot in next Epoch, we think
// the cluster's clock is in sync and everyone will enter the new Epoch soon.
// So we require that we have >80% stake in the new Epoch to exit.
// We use actively_voting_for_this_epoch_stake to determine whether 1/3 of the
// stake has voted for any slot in this Epoch, and then we use actively_voting_stake
// to determine if we have >80% stake in this Epoch.
const EPOCH_CONSIDERED_FOR_EXIT_THRESHOLD: f64 = 1f64 / 3f64;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LastVotedForkSlotsEpochInfo {
    pub epoch: Epoch,
    pub total_stake: u64,
    // Total stake of active peers in this epoch, no matter they voted for a slot
    // in this epoch or not.
    pub actively_voting_stake: u64,
    // Total stake of active peers which has voted for any slot in this epoch.
    // Note that if last_vote slot belongs to epoch n, then this validator should
    // have voted for at least one slot in epoch n - 1, so it should be counted
    // in earlier epoch as well.
    pub actively_voting_for_this_epoch_stake: u64,
}

pub(crate) struct LastVotedForkSlotsAggregate {
    epoch_info_vec: Vec<LastVotedForkSlotsEpochInfo>,
    last_voted_fork_slots: HashMap<Pubkey, RestartLastVotedForkSlots>,
    my_pubkey: Pubkey,
    repair_threshold: f64,
    root_bank: Arc<Bank>,
    slots_stake_map: HashMap<Slot, u64>,
    slots_to_repair: BTreeSet<Slot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LastVotedForkSlotsFinalResult {
    pub slots_stake_map: HashMap<Slot, u64>,
    pub epoch_info_vec: Vec<LastVotedForkSlotsEpochInfo>,
}

#[derive(Debug, PartialEq)]
pub enum LastVotedForkSlotsAggregateResult {
    AlreadyExists,
    DifferentVersionExists(RestartLastVotedForkSlots, RestartLastVotedForkSlots),
    Inserted(LastVotedForkSlotsRecord),
}

impl LastVotedForkSlotsAggregate {
    pub(crate) fn new(
        root_bank: Arc<Bank>,
        repair_threshold: f64,
        my_last_voted_fork_slots: &Vec<Slot>,
        my_pubkey: &Pubkey,
    ) -> Self {
        let mut slots_stake_map = HashMap::new();
        let root_slot = root_bank.slot();
        let root_epoch = root_bank.epoch();
        for slot in my_last_voted_fork_slots {
            if slot >= &root_slot {
                let epoch = root_bank.epoch_schedule().get_epoch(*slot);
                if let Some(sender_stake) = root_bank.epoch_node_id_to_stake(epoch, my_pubkey) {
                    slots_stake_map.insert(*slot, sender_stake);
                } else {
                    warn!("The root bank {root_slot} does not have the stake for slot {slot}");
                }
            }
        }

        let my_last_vote_epoch = root_bank
            .get_epoch_and_slot_index(
                *my_last_voted_fork_slots
                    .iter()
                    .max()
                    .expect("my voted slots should not be empty"),
            )
            .0;
        // We would only consider slots in root_epoch and the next epoch.
        let epoch_info_vec: Vec<LastVotedForkSlotsEpochInfo> = (root_epoch
            ..root_epoch
                .checked_add(2)
                .expect("root_epoch should not be so big"))
            .map(|epoch| {
                let total_stake = root_bank
                    .epoch_total_stake(epoch)
                    .expect("epoch stake not found");
                let my_stake = root_bank
                    .epoch_node_id_to_stake(epoch, my_pubkey)
                    .unwrap_or(0);
                let actively_voting_for_this_epoch_stake = if epoch <= my_last_vote_epoch {
                    my_stake
                } else {
                    0
                };
                LastVotedForkSlotsEpochInfo {
                    epoch,
                    total_stake,
                    actively_voting_stake: my_stake,
                    actively_voting_for_this_epoch_stake,
                }
            })
            .collect();
        Self {
            epoch_info_vec,
            last_voted_fork_slots: HashMap::new(),
            my_pubkey: *my_pubkey,
            repair_threshold,
            root_bank,
            slots_stake_map,
            slots_to_repair: BTreeSet::new(),
        }
    }

    pub(crate) fn aggregate_from_record(
        &mut self,
        key_string: &str,
        record: &LastVotedForkSlotsRecord,
    ) -> Result<LastVotedForkSlotsAggregateResult> {
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
    ) -> LastVotedForkSlotsAggregateResult {
        let from = &new_slots.from;
        if from == &self.my_pubkey {
            return LastVotedForkSlotsAggregateResult::AlreadyExists;
        }
        let root_slot = self.root_bank.slot();
        // to_slots will discard any slot < root_slot.
        let new_slots_vec = new_slots.to_slots(root_slot);
        if new_slots_vec.is_empty() {
            // This could be a validator that has super old vote, we still want to
            // count it in active peers though because it will switch to agreed upon
            // heaviest fork later.
            info!("The slots from {from} is older than root slot {root_slot}");
        }
        if let Some(old_slots) = self.last_voted_fork_slots.get(from) {
            if old_slots.to_slots(self.root_bank.slot()) == new_slots_vec {
                return LastVotedForkSlotsAggregateResult::AlreadyExists;
            } else {
                return LastVotedForkSlotsAggregateResult::DifferentVersionExists(
                    old_slots.clone(),
                    new_slots.clone(),
                );
            }
        }
        self.last_voted_fork_slots.insert(*from, new_slots.clone());
        let last_vote_epoch = self
            .root_bank
            .get_epoch_and_slot_index(new_slots.last_voted_slot)
            .0;
        self.update_epoch_info(from, last_vote_epoch);
        self.insert_message(from, &new_slots_vec);
        LastVotedForkSlotsAggregateResult::Inserted(LastVotedForkSlotsRecord {
            last_voted_fork_slots: new_slots_vec,
            last_vote_bankhash: new_slots.last_voted_hash.to_string(),
            shred_version: new_slots.shred_version as u32,
            wallclock: new_slots.wallclock,
        })
    }

    fn insert_message(&mut self, from: &Pubkey, new_slots_vec: &Vec<Slot>) {
        for slot in new_slots_vec {
            let epoch = self.root_bank.epoch_schedule().get_epoch(*slot);
            let entry = self.slots_stake_map.entry(*slot).or_insert(0);
            if let Some(sender_stake) = self.root_bank.epoch_node_id_to_stake(epoch, from) {
                *entry = entry.saturating_add(sender_stake);
                let repair_threshold_stake = (self.root_bank.epoch_total_stake(epoch).unwrap()
                    as f64
                    * self.repair_threshold) as u64;
                if *entry >= repair_threshold_stake {
                    self.slots_to_repair.insert(*slot);
                }
            }
        }
    }

    fn update_epoch_info(&mut self, from: &Pubkey, last_vote_epoch: Epoch) {
        for entry in self.epoch_info_vec.iter_mut() {
            if let Some(stake) = self.root_bank.epoch_node_id_to_stake(entry.epoch, from) {
                entry.actively_voting_stake =
                    entry.actively_voting_stake.checked_add(stake).unwrap();
                if entry.epoch <= last_vote_epoch {
                    entry.actively_voting_for_this_epoch_stake = entry
                        .actively_voting_for_this_epoch_stake
                        .checked_add(stake)
                        .unwrap();
                }
            }
        }
    }

    pub(crate) fn min_active_percent(&self) -> f64 {
        self.epoch_info_vec
            .iter()
            .filter(|info| {
                info.actively_voting_for_this_epoch_stake as f64 / info.total_stake as f64
                    > EPOCH_CONSIDERED_FOR_EXIT_THRESHOLD
            })
            .map(|info| info.actively_voting_stake as f64 / info.total_stake as f64 * 100.0)
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .unwrap_or(0.0)
    }

    pub(crate) fn slots_to_repair_iter(&self) -> impl Iterator<Item = &Slot> {
        self.slots_to_repair.iter()
    }

    pub(crate) fn get_final_result(self) -> LastVotedForkSlotsFinalResult {
        LastVotedForkSlotsFinalResult {
            slots_stake_map: self.slots_stake_map,
            epoch_info_vec: self.epoch_info_vec,
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            last_voted_fork_slots_aggregate::*, solana::wen_restart_proto::LastVotedForkSlotsRecord,
        },
        solana_gossip::restart_crds_values::RestartLastVotedForkSlots,
        solana_program::clock::Slot,
        solana_runtime::{
            accounts_background_service::AbsRequestSender,
            bank::Bank,
            epoch_stakes::EpochStakes,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{hash::Hash, signature::Signer, timing::timestamp},
        solana_vote::vote_account::VoteAccount,
        solana_vote_program::vote_state::create_account_with_authorized,
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
        let bank0 = bank_forks.read().unwrap().root_bank();
        let bank1 = Bank::new_from_parent(bank0.clone(), &Pubkey::default(), 1);
        bank_forks.write().unwrap().insert(bank1);
        assert!(bank_forks
            .write()
            .unwrap()
            .set_root(1, &AbsRequestSender::default(), None)
            .is_ok());
        let root_bank = bank_forks.read().unwrap().root_bank();
        let root_slot = root_bank.slot();
        let last_voted_fork_slots = vec![
            root_slot.saturating_add(1),
            root_slot.saturating_add(2),
            root_slot.saturating_add(3),
        ];
        TestAggregateInitResult {
            slots_aggregate: LastVotedForkSlotsAggregate::new(
                root_bank,
                REPAIR_THRESHOLD,
                &last_voted_fork_slots,
                &validator_voting_keypairs[MY_INDEX].node_keypair.pubkey(),
            ),
            validator_voting_keypairs,
            root_slot,
            last_voted_fork_slots,
        }
    }

    #[test]
    fn test_aggregate_success() {
        let mut test_state = test_aggregate_init();
        let root_slot = test_state.root_slot;
        // Until 33% stake vote, the percentage should be 0.
        assert_eq!(test_state.slots_aggregate.min_active_percent(), 0.0);
        let initial_num_active_validators = 2;
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
                LastVotedForkSlotsAggregateResult::Inserted(LastVotedForkSlotsRecord {
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash: Hash::default().to_string(),
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                }),
            );
        }
        assert_eq!(test_state.slots_aggregate.min_active_percent(), 0.0);
        assert!(test_state
            .slots_aggregate
            .slots_to_repair_iter()
            .next()
            .is_none());

        // Now add one more validator, min_active_percent should be 40% but repair
        // is still empty (< 42%).
        let new_active_validator = test_state.validator_voting_keypairs
            [initial_num_active_validators]
            .node_keypair
            .pubkey();
        let timestamp1 = timestamp();
        let new_active_validator_last_voted_slots = RestartLastVotedForkSlots::new(
            new_active_validator,
            timestamp1,
            &test_state.last_voted_fork_slots,
            Hash::default(),
            SHRED_VERSION,
        )
        .unwrap();
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate(new_active_validator_last_voted_slots),
            LastVotedForkSlotsAggregateResult::Inserted(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: timestamp1,
            }),
        );
        let expected_active_percent =
            (initial_num_active_validators + 2) as f64 / TOTAL_VALIDATOR_COUNT as f64 * 100.0;
        assert_eq!(
            test_state.slots_aggregate.min_active_percent(),
            expected_active_percent
        );
        assert!(test_state
            .slots_aggregate
            .slots_to_repair_iter()
            .next()
            .is_none());

        // Add one more validator, then repair is > 42% and no longer empty.
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
            LastVotedForkSlotsAggregateResult::Inserted(LastVotedForkSlotsRecord {
                last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: now,
            }),
        );
        let expected_active_percent =
            (initial_num_active_validators + 3) as f64 / TOTAL_VALIDATOR_COUNT as f64 * 100.0;
        assert_eq!(
            test_state.slots_aggregate.min_active_percent(),
            expected_active_percent
        );
        let mut actual_slots =
            Vec::from_iter(test_state.slots_aggregate.slots_to_repair_iter().cloned());
        actual_slots.sort();
        assert_eq!(actual_slots, test_state.last_voted_fork_slots);

        let replace_message_validator = test_state.validator_voting_keypairs
            [initial_num_active_validators]
            .node_keypair
            .pubkey();
        // Do not allow validator to replace message.
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
                .aggregate(replace_message_validator_last_fork.clone()),
            LastVotedForkSlotsAggregateResult::DifferentVersionExists(
                RestartLastVotedForkSlots::new(
                    replace_message_validator,
                    timestamp1,
                    &test_state.last_voted_fork_slots,
                    Hash::default(),
                    SHRED_VERSION
                )
                .unwrap(),
                replace_message_validator_last_fork
            ),
        );
        assert_eq!(
            test_state.slots_aggregate.min_active_percent(),
            expected_active_percent
        );
        let mut actual_slots =
            Vec::from_iter(test_state.slots_aggregate.slots_to_repair_iter().cloned());
        actual_slots.sort();
        assert_eq!(
            actual_slots,
            vec![root_slot + 1, root_slot + 2, root_slot + 3]
        );

        // test that message from my pubkey is ignored.
        assert_eq!(
            test_state.slots_aggregate.aggregate(
                RestartLastVotedForkSlots::new(
                    test_state.validator_voting_keypairs[MY_INDEX]
                        .node_keypair
                        .pubkey(),
                    timestamp(),
                    &[root_slot + 1, root_slot + 4, root_slot + 5],
                    Hash::default(),
                    SHRED_VERSION,
                )
                .unwrap(),
            ),
            LastVotedForkSlotsAggregateResult::AlreadyExists,
        );

        // Test that someone sending super old vote is still counted in active peers.
        let super_old_validator = test_state.validator_voting_keypairs
            [initial_num_active_validators + 2]
            .node_keypair
            .pubkey();
        let now = timestamp();
        let super_old_validator_last_voted_slots = RestartLastVotedForkSlots::new(
            super_old_validator,
            now,
            &[root_slot - 1],
            Hash::default(),
            SHRED_VERSION,
        )
        .unwrap();
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate(super_old_validator_last_voted_slots),
            LastVotedForkSlotsAggregateResult::Inserted(LastVotedForkSlotsRecord {
                last_voted_fork_slots: vec![],
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: SHRED_VERSION as u32,
                wallclock: now,
            }),
        );
        assert_eq!(
            test_state.slots_aggregate.get_final_result(),
            LastVotedForkSlotsFinalResult {
                slots_stake_map: vec![
                    (root_slot + 1, 500),
                    (root_slot + 2, 500),
                    (root_slot + 3, 500),
                ]
                .into_iter()
                .collect(),
                epoch_info_vec: vec![
                    LastVotedForkSlotsEpochInfo {
                        epoch: 0,
                        total_stake: 1000,
                        actively_voting_stake: 600,
                        actively_voting_for_this_epoch_stake: 600,
                    },
                    LastVotedForkSlotsEpochInfo {
                        epoch: 1,
                        total_stake: 1000,
                        actively_voting_stake: 600,
                        actively_voting_for_this_epoch_stake: 0,
                    }
                ],
            },
        );
    }

    #[test]
    fn test_aggregate_from_record_success() {
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
        assert_eq!(test_state.slots_aggregate.min_active_percent(), 0.0);
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
            LastVotedForkSlotsAggregateResult::Inserted(record.clone()),
        );
        // Before 33% voted for slot in this epoch, the percentage should be 0.
        assert_eq!(test_state.slots_aggregate.min_active_percent(), 0.0);
        for i in 1..3 {
            assert_eq!(test_state.slots_aggregate.min_active_percent(), 0.0);
            let pubkey = test_state.validator_voting_keypairs[i]
                .node_keypair
                .pubkey();
            let now = timestamp();
            let last_voted_fork_slots = RestartLastVotedForkSlots::new(
                pubkey,
                now,
                &test_state.last_voted_fork_slots,
                last_vote_bankhash,
                SHRED_VERSION,
            )
            .unwrap();
            assert_eq!(
                test_state.slots_aggregate.aggregate(last_voted_fork_slots),
                LastVotedForkSlotsAggregateResult::Inserted(LastVotedForkSlotsRecord {
                    wallclock: now,
                    last_voted_fork_slots: test_state.last_voted_fork_slots.clone(),
                    last_vote_bankhash: last_vote_bankhash.to_string(),
                    shred_version: SHRED_VERSION as u32,
                }),
            );
        }
        assert_eq!(test_state.slots_aggregate.min_active_percent(), 40.0);
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
            LastVotedForkSlotsAggregateResult::AlreadyExists,
        );

        // If it's a new record from the same validator, it will be ignored.
        let time2 = timestamp();
        let last_voted_fork_slots2 =
            vec![root_slot + 1, root_slot + 2, root_slot + 3, root_slot + 4];
        let last_vote_bankhash2 = Hash::new_unique();
        let new_last_voted_fork_slots = RestartLastVotedForkSlots::new(
            test_state.validator_voting_keypairs[0]
                .node_keypair
                .pubkey(),
            time2,
            &last_voted_fork_slots2,
            last_vote_bankhash2,
            SHRED_VERSION,
        )
        .unwrap();
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate(new_last_voted_fork_slots.clone(),),
            LastVotedForkSlotsAggregateResult::DifferentVersionExists(
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
                new_last_voted_fork_slots
            ),
        );
        // percentage doesn't change since it's a replace.
        assert_eq!(test_state.slots_aggregate.min_active_percent(), 40.0);

        // Record from my pubkey should be ignored.
        assert_eq!(
            test_state
                .slots_aggregate
                .aggregate_from_record(
                    &test_state.validator_voting_keypairs[MY_INDEX]
                        .node_keypair
                        .pubkey()
                        .to_string(),
                    &LastVotedForkSlotsRecord {
                        wallclock: timestamp(),
                        last_voted_fork_slots: vec![root_slot + 10, root_slot + 300],
                        last_vote_bankhash: Hash::new_unique().to_string(),
                        shred_version: SHRED_VERSION as u32,
                    }
                )
                .unwrap(),
            LastVotedForkSlotsAggregateResult::AlreadyExists,
        );
        assert_eq!(
            test_state.slots_aggregate.get_final_result(),
            LastVotedForkSlotsFinalResult {
                slots_stake_map: vec![
                    (root_slot + 1, 400),
                    (root_slot + 2, 400),
                    (root_slot + 3, 400),
                ]
                .into_iter()
                .collect(),
                epoch_info_vec: vec![
                    LastVotedForkSlotsEpochInfo {
                        epoch: 0,
                        total_stake: 1000,
                        actively_voting_stake: 400,
                        actively_voting_for_this_epoch_stake: 400,
                    },
                    LastVotedForkSlotsEpochInfo {
                        epoch: 1,
                        total_stake: 1000,
                        actively_voting_stake: 400,
                        actively_voting_for_this_epoch_stake: 0,
                    }
                ],
            },
        );
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
            LastVotedForkSlotsAggregateResult::Inserted(last_voted_fork_slots_record.clone()),
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

    #[test]
    fn test_aggregate_init_across_epoch() {
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
        // Add bank 1 linking directly to 0, tweak its epoch_stakes, and then add it to bank_forks.
        let mut new_root_bank = Bank::new_from_parent(root_bank.clone(), &Pubkey::default(), 1);

        // For epoch 1, let our validator have 90% of the stake.
        let vote_accounts_hash_map = validator_voting_keypairs
            .iter()
            .enumerate()
            .map(|(i, keypairs)| {
                let stake = if i == MY_INDEX {
                    900 * (TOTAL_VALIDATOR_COUNT - 1) as u64
                } else {
                    100
                };
                let authorized_voter = keypairs.vote_keypair.pubkey();
                let node_id = keypairs.node_keypair.pubkey();
                (
                    authorized_voter,
                    (
                        stake,
                        VoteAccount::try_from(create_account_with_authorized(
                            &node_id,
                            &authorized_voter,
                            &node_id,
                            0,
                            100,
                        ))
                        .unwrap(),
                    ),
                )
            })
            .collect();
        let epoch1_eopch_stakes = EpochStakes::new_for_tests(vote_accounts_hash_map, 1);
        new_root_bank.set_epoch_stakes_for_test(1, epoch1_eopch_stakes);

        let last_voted_fork_slots = vec![root_bank.slot() + 1, root_bank.get_slots_in_epoch(0) + 1];
        let slots_aggregate = LastVotedForkSlotsAggregate::new(
            Arc::new(new_root_bank),
            REPAIR_THRESHOLD,
            &last_voted_fork_slots,
            &validator_voting_keypairs[MY_INDEX].node_keypair.pubkey(),
        );
        assert_eq!(
            slots_aggregate.get_final_result(),
            LastVotedForkSlotsFinalResult {
                slots_stake_map: vec![
                    (root_bank.slot() + 1, 100),
                    (root_bank.get_slots_in_epoch(0) + 1, 8100),
                ]
                .into_iter()
                .collect(),
                epoch_info_vec: vec![
                    LastVotedForkSlotsEpochInfo {
                        epoch: 0,
                        total_stake: 1000,
                        actively_voting_stake: 100,
                        actively_voting_for_this_epoch_stake: 100,
                    },
                    LastVotedForkSlotsEpochInfo {
                        epoch: 1,
                        total_stake: 9000,
                        actively_voting_stake: 8100,
                        actively_voting_for_this_epoch_stake: 8100,
                    }
                ],
            }
        );
    }
}
