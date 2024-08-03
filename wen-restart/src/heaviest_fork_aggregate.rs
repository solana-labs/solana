use {
    crate::solana::wen_restart_proto::HeaviestForkRecord,
    anyhow::Result,
    log::*,
    solana_gossip::restart_crds_values::RestartHeaviestFork,
    solana_runtime::epoch_stakes::EpochStakes,
    solana_sdk::{clock::Slot, hash::Hash, pubkey::Pubkey},
    std::{
        collections::{HashMap, HashSet},
        str::FromStr,
    },
};

pub(crate) struct HeaviestForkAggregate {
    supermajority_threshold: f64,
    my_shred_version: u16,
    my_pubkey: Pubkey,
    // TODO(wen): using local root's EpochStakes, need to fix if crossing Epoch boundary.
    epoch_stakes: EpochStakes,
    heaviest_forks: HashMap<Pubkey, RestartHeaviestFork>,
    block_stake_map: HashMap<(Slot, Hash), u64>,
    active_peers: HashSet<Pubkey>,
    active_peers_seen_supermajority: HashSet<Pubkey>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeaviestForkFinalResult {
    pub block_stake_map: HashMap<(Slot, Hash), u64>,
    pub total_active_stake: u64,
    pub total_active_stake_seen_supermajority: u64,
}

impl HeaviestForkAggregate {
    pub(crate) fn new(
        wait_for_supermajority_threshold_percent: u64,
        my_shred_version: u16,
        epoch_stakes: &EpochStakes,
        my_heaviest_fork_slot: Slot,
        my_heaviest_fork_hash: Hash,
        my_pubkey: &Pubkey,
    ) -> Self {
        let mut active_peers = HashSet::new();
        active_peers.insert(*my_pubkey);
        let mut block_stake_map = HashMap::new();
        block_stake_map.insert(
            (my_heaviest_fork_slot, my_heaviest_fork_hash),
            epoch_stakes.node_id_to_stake(my_pubkey).unwrap_or(0),
        );
        Self {
            supermajority_threshold: wait_for_supermajority_threshold_percent as f64 / 100.0,
            my_shred_version,
            my_pubkey: *my_pubkey,
            epoch_stakes: epoch_stakes.clone(),
            heaviest_forks: HashMap::new(),
            block_stake_map,
            active_peers,
            active_peers_seen_supermajority: HashSet::new(),
        }
    }

    pub(crate) fn aggregate_from_record(
        &mut self,
        key_string: &str,
        record: &HeaviestForkRecord,
    ) -> Result<Option<HeaviestForkRecord>> {
        let from = Pubkey::from_str(key_string)?;
        let bankhash = Hash::from_str(&record.bankhash)?;
        let restart_heaviest_fork = RestartHeaviestFork {
            from,
            wallclock: record.wallclock,
            last_slot: record.slot,
            last_slot_hash: bankhash,
            observed_stake: record.total_active_stake,
            shred_version: record.shred_version as u16,
        };
        Ok(self.aggregate(restart_heaviest_fork))
    }

    fn should_replace(
        current_heaviest_fork: &RestartHeaviestFork,
        new_heaviest_fork: &RestartHeaviestFork,
    ) -> bool {
        if current_heaviest_fork == new_heaviest_fork {
            return false;
        }
        if current_heaviest_fork.wallclock > new_heaviest_fork.wallclock {
            return false;
        }
        if current_heaviest_fork.last_slot == new_heaviest_fork.last_slot
            && current_heaviest_fork.last_slot_hash == new_heaviest_fork.last_slot_hash
            && current_heaviest_fork.observed_stake == new_heaviest_fork.observed_stake
        {
            return false;
        }
        true
    }

    pub(crate) fn aggregate(
        &mut self,
        received_heaviest_fork: RestartHeaviestFork,
    ) -> Option<HeaviestForkRecord> {
        let total_stake = self.epoch_stakes.total_stake();
        let from = &received_heaviest_fork.from;
        let sender_stake = self.epoch_stakes.node_id_to_stake(from).unwrap_or(0);
        if sender_stake == 0 {
            warn!(
                "Gossip should not accept zero-stake RestartLastVotedFork from {:?}",
                from
            );
            return None;
        }
        if from == &self.my_pubkey {
            return None;
        }
        if received_heaviest_fork.shred_version != self.my_shred_version {
            warn!(
                "Gossip should not accept RestartLastVotedFork with different shred version {} from {:?}",
                received_heaviest_fork.shred_version, from
            );
            return None;
        }
        let record = HeaviestForkRecord {
            slot: received_heaviest_fork.last_slot,
            bankhash: received_heaviest_fork.last_slot_hash.to_string(),
            total_active_stake: received_heaviest_fork.observed_stake,
            shred_version: received_heaviest_fork.shred_version as u32,
            wallclock: received_heaviest_fork.wallclock,
        };
        if let Some(old_heaviest_fork) = self
            .heaviest_forks
            .insert(*from, received_heaviest_fork.clone())
        {
            if Self::should_replace(&old_heaviest_fork, &received_heaviest_fork) {
                let entry = self
                    .block_stake_map
                    .get_mut(&(
                        old_heaviest_fork.last_slot,
                        old_heaviest_fork.last_slot_hash,
                    ))
                    .unwrap();
                info!(
                    "{:?} Replacing old heaviest fork from {:?} with {:?}",
                    from, old_heaviest_fork, received_heaviest_fork
                );
                *entry = entry.saturating_sub(sender_stake);
            } else {
                return None;
            }
        }
        let entry = self
            .block_stake_map
            .entry((
                received_heaviest_fork.last_slot,
                received_heaviest_fork.last_slot_hash,
            ))
            .or_insert(0);
        *entry = entry.saturating_add(sender_stake);
        self.active_peers.insert(*from);
        if received_heaviest_fork.observed_stake as f64 / total_stake as f64
            >= self.supermajority_threshold
        {
            self.active_peers_seen_supermajority.insert(*from);
        }
        if !self
            .active_peers_seen_supermajority
            .contains(&self.my_pubkey)
            && self.total_active_stake() as f64 / total_stake as f64 >= self.supermajority_threshold
        {
            self.active_peers_seen_supermajority.insert(self.my_pubkey);
        }
        Some(record)
    }

    // TODO(wen): use better epoch stake and add a test later.
    pub(crate) fn total_active_stake(&self) -> u64 {
        self.active_peers.iter().fold(0, |sum: u64, pubkey| {
            sum.saturating_add(self.epoch_stakes.node_id_to_stake(pubkey).unwrap_or(0))
        })
    }

    pub(crate) fn total_active_stake_seen_supermajority(&self) -> u64 {
        self.active_peers_seen_supermajority
            .iter()
            .fold(0, |sum: u64, pubkey| {
                sum.saturating_add(self.epoch_stakes.node_id_to_stake(pubkey).unwrap_or(0))
            })
    }

    pub(crate) fn block_stake_map(self) -> HashMap<(Slot, Hash), u64> {
        self.block_stake_map
    }
}

#[cfg(test)]
mod tests {
    use {
        crate::{
            heaviest_fork_aggregate::HeaviestForkAggregate,
            solana::wen_restart_proto::HeaviestForkRecord,
        },
        solana_gossip::restart_crds_values::RestartHeaviestFork,
        solana_program::{clock::Slot, pubkey::Pubkey},
        solana_runtime::{
            bank::Bank,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{hash::Hash, signature::Signer, timing::timestamp},
    };

    const TOTAL_VALIDATOR_COUNT: u16 = 20;
    const MY_INDEX: usize = 19;
    const SHRED_VERSION: u16 = 52;

    struct TestAggregateInitResult {
        pub heaviest_fork_aggregate: HeaviestForkAggregate,
        pub validator_voting_keypairs: Vec<ValidatorVoteKeypairs>,
        pub heaviest_slot: Slot,
        pub heaviest_hash: Hash,
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
        let heaviest_slot = root_bank.slot().saturating_add(3);
        let heaviest_hash = Hash::new_unique();
        TestAggregateInitResult {
            heaviest_fork_aggregate: HeaviestForkAggregate::new(
                75,
                SHRED_VERSION,
                root_bank.epoch_stakes(root_bank.epoch()).unwrap(),
                heaviest_slot,
                heaviest_hash,
                &validator_voting_keypairs[MY_INDEX].node_keypair.pubkey(),
            ),
            validator_voting_keypairs,
            heaviest_slot,
            heaviest_hash,
        }
    }

    #[test]
    fn test_aggregate_from_gossip() {
        let mut test_state = test_aggregate_init();
        let initial_num_active_validators = 3;
        for validator_voting_keypair in test_state
            .validator_voting_keypairs
            .iter()
            .take(initial_num_active_validators)
        {
            let pubkey = validator_voting_keypair.node_keypair.pubkey();
            let now = timestamp();
            assert_eq!(
                test_state
                    .heaviest_fork_aggregate
                    .aggregate(RestartHeaviestFork {
                        from: pubkey,
                        wallclock: now,
                        last_slot: test_state.heaviest_slot,
                        last_slot_hash: test_state.heaviest_hash,
                        observed_stake: 100,
                        shred_version: SHRED_VERSION,
                    },),
                Some(HeaviestForkRecord {
                    slot: test_state.heaviest_slot,
                    bankhash: test_state.heaviest_hash.to_string(),
                    total_active_stake: 100,
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                }),
            );
        }
        assert_eq!(
            test_state.heaviest_fork_aggregate.total_active_stake(),
            (initial_num_active_validators + 1) as u64 * 100
        );

        let new_active_validator = test_state.validator_voting_keypairs
            [initial_num_active_validators + 1]
            .node_keypair
            .pubkey();
        let now = timestamp();
        let new_active_validator_last_voted_slots = RestartHeaviestFork {
            from: new_active_validator,
            wallclock: now,
            last_slot: test_state.heaviest_slot,
            last_slot_hash: test_state.heaviest_hash,
            observed_stake: 100,
            shred_version: SHRED_VERSION,
        };
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate(new_active_validator_last_voted_slots),
            Some(HeaviestForkRecord {
                slot: test_state.heaviest_slot,
                bankhash: test_state.heaviest_hash.to_string(),
                total_active_stake: 100,
                shred_version: SHRED_VERSION as u32,
                wallclock: now,
            }),
        );
        let expected_total_active_stake = (initial_num_active_validators + 2) as u64 * 100;
        assert_eq!(
            test_state.heaviest_fork_aggregate.total_active_stake(),
            expected_total_active_stake
        );
        let replace_message_validator = test_state.validator_voting_keypairs[2]
            .node_keypair
            .pubkey();
        // Allow specific validator to replace message.
        let now = timestamp();
        let new_hash = Hash::new_unique();
        let replace_message_validator_last_fork = RestartHeaviestFork {
            from: replace_message_validator,
            wallclock: now,
            last_slot: test_state.heaviest_slot + 1,
            last_slot_hash: new_hash,
            observed_stake: 100,
            shred_version: SHRED_VERSION,
        };
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate(replace_message_validator_last_fork),
            Some(HeaviestForkRecord {
                slot: test_state.heaviest_slot + 1,
                bankhash: new_hash.to_string(),
                total_active_stake: 100,
                shred_version: SHRED_VERSION as u32,
                wallclock: now,
            }),
        );
        assert_eq!(
            test_state.heaviest_fork_aggregate.total_active_stake(),
            expected_total_active_stake
        );

        // test that zero stake validator is ignored.
        let random_pubkey = Pubkey::new_unique();
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate(RestartHeaviestFork {
                    from: random_pubkey,
                    wallclock: timestamp(),
                    last_slot: test_state.heaviest_slot,
                    last_slot_hash: test_state.heaviest_hash,
                    observed_stake: 100,
                    shred_version: SHRED_VERSION,
                },),
            None,
        );
        assert_eq!(
            test_state.heaviest_fork_aggregate.total_active_stake(),
            expected_total_active_stake
        );

        // If everyone is seeing only 70%, the total active stake seeing supermajority is 0.
        for validator_voting_keypair in test_state.validator_voting_keypairs.iter().take(13) {
            let pubkey = validator_voting_keypair.node_keypair.pubkey();
            let now = timestamp();
            assert_eq!(
                test_state
                    .heaviest_fork_aggregate
                    .aggregate(RestartHeaviestFork {
                        from: pubkey,
                        wallclock: now,
                        last_slot: test_state.heaviest_slot,
                        last_slot_hash: test_state.heaviest_hash,
                        observed_stake: 1400,
                        shred_version: SHRED_VERSION,
                    },),
                Some(HeaviestForkRecord {
                    slot: test_state.heaviest_slot,
                    bankhash: test_state.heaviest_hash.to_string(),
                    total_active_stake: 1400,
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                }),
            );
        }
        assert_eq!(
            test_state.heaviest_fork_aggregate.total_active_stake(),
            1400
        );
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .total_active_stake_seen_supermajority(),
            0
        );

        // test that when 75% of the stake is seeing supermajority,
        // the active percent seeing supermajority is 75%.
        for validator_voting_keypair in test_state.validator_voting_keypairs.iter().take(14) {
            let pubkey = validator_voting_keypair.node_keypair.pubkey();
            let now = timestamp();
            assert_eq!(
                test_state
                    .heaviest_fork_aggregate
                    .aggregate(RestartHeaviestFork {
                        from: pubkey,
                        wallclock: now,
                        last_slot: test_state.heaviest_slot,
                        last_slot_hash: test_state.heaviest_hash,
                        observed_stake: 1500,
                        shred_version: SHRED_VERSION,
                    },),
                Some(HeaviestForkRecord {
                    slot: test_state.heaviest_slot,
                    bankhash: test_state.heaviest_hash.to_string(),
                    total_active_stake: 1500,
                    shred_version: SHRED_VERSION as u32,
                    wallclock: now,
                }),
            );
        }

        assert_eq!(
            test_state.heaviest_fork_aggregate.total_active_stake(),
            1500
        );
        // I myself is seeing supermajority as well, with the 14 validators
        // reporting 70%, the total active stake seeing supermajority is 1500 (75%).
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .total_active_stake_seen_supermajority(),
            1500
        );

        // test that message from my pubkey is ignored.
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate(RestartHeaviestFork {
                    from: test_state.validator_voting_keypairs[MY_INDEX]
                        .node_keypair
                        .pubkey(),
                    wallclock: timestamp(),
                    last_slot: test_state.heaviest_slot,
                    last_slot_hash: test_state.heaviest_hash,
                    observed_stake: 100,
                    shred_version: SHRED_VERSION,
                },),
            None,
        );
    }

    #[test]
    fn test_aggregate_from_record() {
        let mut test_state = test_aggregate_init();
        let time1 = timestamp();
        let record = HeaviestForkRecord {
            wallclock: time1,
            slot: test_state.heaviest_slot,
            bankhash: test_state.heaviest_hash.to_string(),
            shred_version: SHRED_VERSION as u32,
            total_active_stake: 100,
        };
        assert_eq!(test_state.heaviest_fork_aggregate.total_active_stake(), 100);
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
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
        assert_eq!(test_state.heaviest_fork_aggregate.total_active_stake(), 200);
        // Now if you get the same result from Gossip again, it should be ignored.
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate(RestartHeaviestFork {
                    from: test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey(),
                    wallclock: time1,
                    last_slot: test_state.heaviest_slot,
                    last_slot_hash: test_state.heaviest_hash,
                    observed_stake: 100,
                    shred_version: SHRED_VERSION,
                },),
            None,
        );

        // But if it's a new record from the same validator, it will be replaced.
        let time2 = timestamp();
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate(RestartHeaviestFork {
                    from: test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey(),
                    wallclock: time2,
                    last_slot: test_state.heaviest_slot,
                    last_slot_hash: test_state.heaviest_hash,
                    observed_stake: 200,
                    shred_version: SHRED_VERSION,
                },),
            Some(HeaviestForkRecord {
                wallclock: time2,
                slot: test_state.heaviest_slot,
                bankhash: test_state.heaviest_hash.to_string(),
                shred_version: SHRED_VERSION as u32,
                total_active_stake: 200,
            }),
        );
        // percentage doesn't change since it's a replace.
        assert_eq!(test_state.heaviest_fork_aggregate.total_active_stake(), 200);

        // Record from validator with zero stake should be ignored.
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate_from_record(
                    &Pubkey::new_unique().to_string(),
                    &HeaviestForkRecord {
                        wallclock: timestamp(),
                        slot: test_state.heaviest_slot,
                        bankhash: test_state.heaviest_hash.to_string(),
                        shred_version: SHRED_VERSION as u32,
                        total_active_stake: 100,
                    }
                )
                .unwrap(),
            None,
        );
        // percentage doesn't change since the previous aggregate is ignored.
        assert_eq!(test_state.heaviest_fork_aggregate.total_active_stake(), 200);

        // Record from my pubkey should be ignored.
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate_from_record(
                    &test_state.validator_voting_keypairs[MY_INDEX]
                        .node_keypair
                        .pubkey()
                        .to_string(),
                    &HeaviestForkRecord {
                        wallclock: timestamp(),
                        slot: test_state.heaviest_slot,
                        bankhash: test_state.heaviest_hash.to_string(),
                        shred_version: SHRED_VERSION as u32,
                        total_active_stake: 100,
                    }
                )
                .unwrap(),
            None,
        );
    }

    #[test]
    fn test_aggregate_from_record_failures() {
        let mut test_state = test_aggregate_init();
        let mut heaviest_fork_record = HeaviestForkRecord {
            wallclock: timestamp(),
            slot: test_state.heaviest_slot,
            bankhash: test_state.heaviest_hash.to_string(),
            shred_version: SHRED_VERSION as u32,
            total_active_stake: 100,
        };
        // First test that this is a valid record.
        assert_eq!(
            test_state
                .heaviest_fork_aggregate
                .aggregate_from_record(
                    &test_state.validator_voting_keypairs[0]
                        .node_keypair
                        .pubkey()
                        .to_string(),
                    &heaviest_fork_record,
                )
                .unwrap(),
            Some(heaviest_fork_record.clone()),
        );
        // Then test that it fails if the record is invalid.

        // Invalid pubkey.
        assert!(test_state
            .heaviest_fork_aggregate
            .aggregate_from_record("invalid_pubkey", &heaviest_fork_record,)
            .is_err());

        // Invalid hash.
        heaviest_fork_record.bankhash.clear();
        assert!(test_state
            .heaviest_fork_aggregate
            .aggregate_from_record(
                &test_state.validator_voting_keypairs[0]
                    .node_keypair
                    .pubkey()
                    .to_string(),
                &heaviest_fork_record,
            )
            .is_err());
    }
}
