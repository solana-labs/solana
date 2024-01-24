use {
    crate::solana::wen_restart_proto::LastVotedForkSlotsRecord,
    log::*,
    solana_gossip::restart_crds_values::RestartLastVotedForkSlots,
    solana_runtime::epoch_stakes::EpochStakes,
    solana_sdk::{clock::Slot, pubkey::Pubkey},
    std::collections::{HashMap, HashSet},
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

pub struct LastVotedForkSlotsAggregateResult {
    pub slots_to_repair: Vec<Slot>,
    pub active_percent: f64, /* 0 ~ 100.0 */
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

    pub(crate) fn aggregate(
        &mut self,
        new_slots: RestartLastVotedForkSlots,
    ) -> Option<LastVotedForkSlotsRecord> {
        let total_stake = self.epoch_stakes.total_stake();
        let threshold_stake = (total_stake as f64 * self.repair_threshold) as u64;
        let from = &new_slots.from;
        self.active_peers.insert(*from);
        let sender_stake = Self::validator_stake(&self.epoch_stakes, from);
        if sender_stake == 0 {
            warn!(
                "Gossip should not accept zero-stake RestartLastVotedFork from {:?}",
                from
            );
            return None;
        }
        let new_slots_vec = new_slots.to_slots(self.root_slot);
        let record = LastVotedForkSlotsRecord {
            last_vote_fork_slots: new_slots_vec.clone(),
            last_vote_bankhash: new_slots.last_voted_hash.to_string(),
            shred_version: new_slots.shred_version as u32,
        };
        let new_slots_set: HashSet<Slot> = HashSet::from_iter(new_slots_vec);
        let old_slots_set = match self.last_voted_fork_slots.insert(new_slots.from, new_slots) {
            Some(old_slots) => HashSet::from_iter(old_slots.to_slots(self.root_slot)),
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

    pub(crate) fn get_aggregate_result(&self) -> LastVotedForkSlotsAggregateResult {
        let total_stake = self.epoch_stakes.total_stake();
        let total_active_stake = self.active_peers.iter().fold(0, |sum: u64, pubkey| {
            sum.saturating_add(Self::validator_stake(&self.epoch_stakes, pubkey))
        });
        let active_percent = total_active_stake as f64 / total_stake as f64 * 100.0;
        LastVotedForkSlotsAggregateResult {
            slots_to_repair: self.slots_to_repair.iter().cloned().collect(),
            active_percent,
        }
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
        solana_runtime::{
            bank::Bank,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{hash::Hash, signature::Signer, timing::timestamp},
    };

    #[test]
    fn test_aggregate() {
        solana_logger::setup();
        const TOTAL_VALIDATOR_COUNT: u16 = 10;
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
        let shred_version = 52;
        let last_voted_fork = vec![root_slot + 1, root_slot + 2, root_slot + 3];
        let my_index = 9;
        let mut slots_aggregate = LastVotedForkSlotsAggregate::new(
            root_slot,
            0.42,
            root_bank.epoch_stakes(root_bank.epoch()).unwrap(),
            &last_voted_fork,
            &validator_voting_keypairs[my_index].node_keypair.pubkey(),
        );
        let initial_num_active_validators = 3;
        for validator_voting_keypair in validator_voting_keypairs
            .iter()
            .take(initial_num_active_validators)
        {
            let pubkey = validator_voting_keypair.node_keypair.pubkey();
            assert_eq!(
                slots_aggregate.aggregate(
                    RestartLastVotedForkSlots::new(
                        pubkey,
                        timestamp(),
                        &last_voted_fork,
                        Hash::default(),
                        shred_version,
                    )
                    .unwrap(),
                ),
                Some(LastVotedForkSlotsRecord {
                    last_vote_fork_slots: last_voted_fork.clone(),
                    last_vote_bankhash: Hash::default().to_string(),
                    shred_version: shred_version as u32,
                }),
            );
        }
        let result = slots_aggregate.get_aggregate_result();
        let mut expected_active_percent =
            (initial_num_active_validators + 1) as f64 / TOTAL_VALIDATOR_COUNT as f64 * 100.0;
        assert_eq!(result.active_percent, expected_active_percent);
        assert!(result.slots_to_repair.is_empty());

        let new_active_validator = validator_voting_keypairs[initial_num_active_validators + 1]
            .node_keypair
            .pubkey();
        let new_active_validator_last_voted_slots = RestartLastVotedForkSlots::new(
            new_active_validator,
            timestamp(),
            &last_voted_fork,
            Hash::default(),
            shred_version,
        )
        .unwrap();
        assert_eq!(
            slots_aggregate.aggregate(new_active_validator_last_voted_slots),
            Some(LastVotedForkSlotsRecord {
                last_vote_fork_slots: last_voted_fork.clone(),
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: shred_version as u32,
            }),
        );
        let result = slots_aggregate.get_aggregate_result();
        expected_active_percent =
            (initial_num_active_validators + 2) as f64 / TOTAL_VALIDATOR_COUNT as f64 * 100.0;
        assert_eq!(result.active_percent, expected_active_percent);
        let mut actual_slots = Vec::from_iter(result.slots_to_repair);
        actual_slots.sort();
        assert_eq!(actual_slots, last_voted_fork);

        let replace_message_validator = validator_voting_keypairs[2].node_keypair.pubkey();
        // Allow specific validator to replace message.
        let replace_message_validator_last_fork = RestartLastVotedForkSlots::new(
            replace_message_validator,
            timestamp(),
            &[root_slot + 1, root_slot + 4, root_slot + 5],
            Hash::default(),
            shred_version,
        )
        .unwrap();
        assert_eq!(
            slots_aggregate.aggregate(replace_message_validator_last_fork),
            Some(LastVotedForkSlotsRecord {
                last_vote_fork_slots: vec![root_slot + 1, root_slot + 4, root_slot + 5],
                last_vote_bankhash: Hash::default().to_string(),
                shred_version: shred_version as u32,
            }),
        );
        let result = slots_aggregate.get_aggregate_result();
        assert_eq!(result.active_percent, expected_active_percent);
        let mut actual_slots = Vec::from_iter(result.slots_to_repair);
        actual_slots.sort();
        assert_eq!(actual_slots, vec![root_slot + 1]);
    }
}
