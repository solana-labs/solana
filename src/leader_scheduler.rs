//! The `leader_scheduler` module implements a structure and functions for tracking and
//! managing the schedule for leader rotation

use crate::entry::{create_ticks, next_entry_mut, Entry};
use crate::voting_keypair::VotingKeypair;
use bincode::serialize;
use byteorder::{LittleEndian, ReadBytesExt};
use solana_runtime::bank::Bank;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT};
use solana_sdk::vote_program::VoteState;
use solana_sdk::vote_transaction::VoteTransaction;
use std::io::Cursor;
use std::sync::Arc;

pub const DEFAULT_ACTIVE_WINDOW_NUM_SLOTS: u64 = DEFAULT_SLOTS_PER_EPOCH;

#[derive(Clone)]
pub struct LeaderSchedulerConfig {
    pub ticks_per_slot: u64,
    pub slots_per_epoch: u64,

    // The tick length of the acceptable window for determining live validators
    pub active_window_num_slots: u64,
}

// Used to toggle leader rotation in fullnode so that tests that don't
// need leader rotation don't break
impl LeaderSchedulerConfig {
    pub fn new(ticks_per_slot: u64, slots_per_epoch: u64, active_window_num_slots: u64) -> Self {
        LeaderSchedulerConfig {
            ticks_per_slot,
            slots_per_epoch,
            active_window_num_slots,
        }
    }
}

impl Default for LeaderSchedulerConfig {
    fn default() -> Self {
        Self {
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_epoch: DEFAULT_SLOTS_PER_EPOCH,
            active_window_num_slots: DEFAULT_ACTIVE_WINDOW_NUM_SLOTS,
        }
    }
}

fn sort_stakes(stakes: &mut Vec<(Pubkey, u64)>) {
    // Sort first by stake. If stakes are the same, sort by pubkey to ensure a
    // deterministic result.
    // Note: Use unstable sort, because we dedup right after to remove the equal elements.
    stakes.sort_unstable_by(|(pubkey0, stake0), (pubkey1, stake1)| {
        if stake0 == stake1 {
            pubkey0.cmp(&pubkey1)
        } else {
            stake0.cmp(&stake1)
        }
    });

    // Now that it's sorted, we can do an O(n) dedup.
    stakes.dedup();
}

// Return true of the latest vote is between the lower and upper bounds (inclusive)
fn is_active_staker(vote_state: &VoteState, lower_bound: u64, upper_bound: u64) -> bool {
    vote_state
        .votes
        .back()
        .filter(|vote| vote.slot_height >= lower_bound && vote.slot_height <= upper_bound)
        .is_some()
}

/// Return a sorted, filtered list of node_id/stake pairs.
fn get_active_stakes(
    bank: &Bank,
    active_window_num_slots: u64,
    upper_bound: u64,
) -> Vec<(Pubkey, u64)> {
    let lower_bound = upper_bound.saturating_sub(active_window_num_slots);
    let mut stakes: Vec<_> = bank
        .vote_states(|vote_state| is_active_staker(vote_state, lower_bound, upper_bound))
        .iter()
        .filter_map(|vote_state| {
            let stake = bank.get_balance(&vote_state.staker_id);
            if stake > 0 {
                Some((vote_state.node_id, stake))
            } else {
                None
            }
        })
        .collect();
    sort_stakes(&mut stakes);
    stakes
}

#[derive(Clone, Debug)]
pub struct LeaderScheduler {
    // A leader slot duration in ticks
    ticks_per_slot: u64,

    // Duration of an epoch (one or more slots) in ticks.
    // This value must be divisible by ticks_per_slot
    slots_per_epoch: u64,

    // The number of slots for which a vote qualifies a candidate for leader
    // selection
    active_window_num_slots: u64,

    // Round-robin ordering of the validators for the current epoch at epoch_schedule[0], and the
    // previous epoch at epoch_schedule[1]
    epoch_schedule: [Vec<Pubkey>; 2],

    // The epoch for epoch_schedule[0]
    current_epoch: u64,

    // The seed used to determine the round robin order of leaders
    seed: u64,
}

// The LeaderScheduler implements a schedule for leaders as follows:
//
// 1) After the first seed is generated, this signals the beginning of actual leader rotation.
// From this point on, every ticks_per_epoch PoH counts we generate the seed based
// on the PoH height, and use it to do a weighted sample from the set
// of validators based on current stake weight. This gets you the bootstrap leader A for
// the next ticks_per_slot PoH counts. On the same PoH count we generate the seed,
// we also order the validators based on their current stake weight, and starting
// from leader A, we then pick the next leader sequentially every ticks_per_slot
// PoH counts based on this fixed ordering, so the next
// ticks_per_epoch / ticks_per_slot leaders are determined.
//
// 2) When we we hit the next seed rotation PoH height, step 1) is executed again to
// calculate the leader schedule for the upcoming ticks_per_epoch PoH counts.
impl LeaderScheduler {
    pub fn new(config: &LeaderSchedulerConfig) -> Self {
        let ticks_per_slot = config.ticks_per_slot;
        let slots_per_epoch = config.slots_per_epoch;
        let active_window_num_slots = config.active_window_num_slots;

        // Enforced invariants
        assert!(ticks_per_slot > 0);
        assert!(active_window_num_slots > 0);

        Self {
            ticks_per_slot,
            slots_per_epoch,
            active_window_num_slots,
            seed: 0,
            epoch_schedule: [Vec::new(), Vec::new()],
            current_epoch: 0,
        }
    }

    // Same as new_with_bank() but allows caller to override `active_window_slot_len`.
    // Used by unit-tests.
    fn new_with_window_len(active_window_slot_len: u64, bank: &Bank) -> Self {
        let config = LeaderSchedulerConfig::new(
            bank.ticks_per_slot(),
            bank.slots_per_epoch(),
            active_window_slot_len,
        );
        let mut leader_schedule = Self::new(&config);
        leader_schedule.update_tick_height(bank.tick_height(), bank);
        leader_schedule
    }

    pub fn new_with_bank(bank: &Bank) -> Self {
        Self::new_with_window_len(DEFAULT_ACTIVE_WINDOW_NUM_SLOTS, bank)
    }

    pub fn tick_height_to_slot(&self, tick_height: u64) -> u64 {
        tick_height / self.ticks_per_slot
    }

    fn ticks_per_epoch(&self) -> u64 {
        self.slots_per_epoch * self.ticks_per_slot
    }

    fn tick_height_to_epoch(&self, tick_height: u64) -> u64 {
        tick_height / self.ticks_per_epoch()
    }

    // Returns the number of ticks remaining from the specified tick_height to
    // the end of the specified block (i.e. the end of corresponding slot)
    pub fn num_ticks_left_in_block(&self, block: u64, tick_height: u64) -> u64 {
        ((block + 1) * self.ticks_per_slot - tick_height) - 1
    }

    // Returns the number of ticks remaining from the specified tick_height to the end of the
    // slot implied by the tick_height
    pub fn num_ticks_left_in_slot(&self, tick_height: u64) -> u64 {
        self.ticks_per_slot - tick_height % self.ticks_per_slot - 1
    }

    // Inform the leader scheduler about the current tick height of the cluster.  It may generate a
    // new schedule as a side-effect.
    pub fn update_tick_height(&mut self, tick_height: u64, bank: &Bank) {
        let epoch = self.tick_height_to_epoch(tick_height);
        trace!(
            "update_tick_height: tick_height={} (epoch={})",
            tick_height,
            epoch,
        );

        if tick_height == 0 {
            // Special case: tick_height starts at 0 instead of -1, so generate_schedule() for 0
            // here before moving on to tick_height + 1
            self.generate_schedule(0, bank);
        }

        // If we're about to cross an epoch boundary generate the schedule for the next epoch
        if self.tick_height_to_epoch(tick_height + 1) == epoch + 1 {
            self.generate_schedule(tick_height + 1, bank);
        }
    }

    pub fn get_leader_for_tick(&self, tick: u64) -> Option<Pubkey> {
        self.get_leader_for_slot(self.tick_height_to_slot(tick))
    }

    // Returns the leader for the requested slot, or None if the slot is out of the schedule bounds
    pub fn get_leader_for_slot(&self, slot: u64) -> Option<Pubkey> {
        trace!("get_leader_for_slot: slot {}", slot);
        let tick_height = slot * self.ticks_per_slot;
        let epoch = self.tick_height_to_epoch(tick_height);
        trace!(
            "get_leader_for_slot: tick_height={} slot={} epoch={} (ce={})",
            tick_height,
            slot,
            epoch,
            self.current_epoch
        );

        if epoch > self.current_epoch {
            warn!(
                "get_leader_for_slot: leader unknown for epoch {}, which is larger than {}",
                epoch, self.current_epoch
            );
            None
        } else if epoch < self.current_epoch.saturating_sub(1) {
            warn!(
                "get_leader_for_slot: leader unknown for epoch {}, which is less than {}",
                epoch,
                self.current_epoch.saturating_sub(1)
            );
            None
        } else {
            let schedule = &self.epoch_schedule[(self.current_epoch - epoch) as usize];
            if schedule.is_empty() {
                panic!("leader_schedule is empty"); // Should never happen
            }

            let first_tick_in_epoch = epoch * self.ticks_per_epoch();
            let slot_index = (tick_height - first_tick_in_epoch) / self.ticks_per_slot;

            // Round robin through each node in the schedule
            Some(schedule[slot_index as usize % schedule.len()])
        }
    }

    // Updates the leader schedule to include ticks from tick_height to the first tick of the next epoch
    fn generate_schedule(&mut self, tick_height: u64, bank: &Bank) {
        let epoch = self.tick_height_to_epoch(tick_height);
        trace!(
            "generate_schedule: tick_height={} (epoch={})",
            tick_height,
            epoch
        );
        if epoch < self.current_epoch {
            // Don't support going backwards for implementation convenience
            panic!(
                "Unable to generate the schedule for epoch < current_epoch ({} < {})",
                epoch, self.current_epoch
            );
        } else if epoch > self.current_epoch + 1 {
            // Don't support skipping epochs going forwards for implementation convenience
            panic!(
                "Unable to generate the schedule for epoch > current_epoch + 1 ({} > {})",
                epoch,
                self.current_epoch + 1
            );
        }

        if epoch > self.current_epoch {
            self.epoch_schedule[1] = self.epoch_schedule[0].clone();
            self.current_epoch = epoch;
        }

        self.seed = Self::calculate_seed(tick_height);
        let slot = self.tick_height_to_slot(tick_height);
        let ranked_active_set = get_active_stakes(&bank, self.active_window_num_slots, slot);

        if ranked_active_set.is_empty() {
            info!(
                "generate_schedule: empty ranked_active_set at tick_height {}, using leader_schedule from previous epoch",
                tick_height,
            );
        } else {
            let (mut validator_rankings, total_stake) = ranked_active_set.iter().fold(
                (Vec::with_capacity(ranked_active_set.len()), 0),
                |(mut ids, total_stake), (pubkey, stake)| {
                    ids.push(*pubkey);
                    (ids, total_stake + stake)
                },
            );

            // Choose a validator to be the first slot leader in the new schedule
            let ordered_account_stake = ranked_active_set.into_iter().map(|(_, stake)| stake);
            let start_index = Self::choose_account(ordered_account_stake, self.seed, total_stake);
            validator_rankings.rotate_left(start_index);

            // If possible try to avoid having the same slot leader twice in a row, but
            // if there's only one leader to choose from then we have no other choice
            if validator_rankings.len() > 1 && tick_height > 0 {
                let last_slot_leader = self
                    .get_leader_for_slot(self.tick_height_to_slot(tick_height - 1))
                    .expect("Previous leader schedule should still exist");
                let next_slot_leader = validator_rankings[0];

                if last_slot_leader == next_slot_leader {
                    if self.slots_per_epoch == 1 {
                        // If there is only one slot per epoch, and the same leader as the last slot
                        // of the previous epoch was chosen, then pick the next leader in the
                        // rankings instead
                        validator_rankings[0] = validator_rankings[1];
                        validator_rankings.truncate(self.slots_per_epoch as usize);
                    } else {
                        // If there is more than one leader in the schedule, truncate and set the most
                        // recent leader to the back of the line. This way that node will still remain
                        // in the rotation, just at a later slot.
                        validator_rankings.truncate(self.slots_per_epoch as usize);
                        validator_rankings.rotate_left(1);
                    }
                }
            }
            self.epoch_schedule[0] = validator_rankings;
        }

        assert!(!self.epoch_schedule[0].is_empty());
        trace!(
            "generate_schedule: schedule for ticks ({}, {}): {:?} ",
            tick_height,
            tick_height + self.ticks_per_epoch(),
            self.epoch_schedule[0]
        );
    }

    fn calculate_seed(tick_height: u64) -> u64 {
        let hash = hash(&serialize(&tick_height).unwrap());
        let bytes = hash.as_ref();
        let mut rdr = Cursor::new(bytes);
        rdr.read_u64::<LittleEndian>().unwrap()
    }

    fn choose_account<I>(stakes: I, seed: u64, total_stake: u64) -> usize
    where
        I: IntoIterator<Item = u64>,
    {
        let mut total = 0;
        let mut chosen_account = 0;
        let seed = seed % total_stake;
        for (i, s) in stakes.into_iter().enumerate() {
            // We should have filtered out all accounts with zero stake in
            // rank_active_set()
            assert!(s != 0);
            total += s;
            if total > seed {
                chosen_account = i;
                break;
            }
        }

        chosen_account
    }

    #[cfg(test)]
    pub fn reset(&mut self) {
        self.current_epoch = 0;
        self.epoch_schedule = [Vec::new(), Vec::new()];
    }

    /// Force a schedule for the first epoch
    #[cfg(test)]
    pub fn set_leader_schedule(&mut self, schedule: Vec<Pubkey>) {
        assert!(!schedule.is_empty());
        self.current_epoch = 0;
        self.epoch_schedule[0] = schedule;
        self.epoch_schedule[1] = Vec::new();
    }
}

impl Default for LeaderScheduler {
    fn default() -> Self {
        let config = LeaderSchedulerConfig::default();
        Self::new(&config)
    }
}

// Create entries such the node identified by active_keypair
// will be added to the active set for leader selection:
// 1) Give the node a nonzero number of tokens,
// 2) A vote from the validator
pub fn make_active_set_entries(
    active_keypair: &Arc<Keypair>,
    token_source: &Keypair,
    stake: u64,
    slot_height_to_vote_on: u64,
    last_entry_id: &Hash,
    last_tick_id: &Hash,
    num_ending_ticks: u64,
) -> (Vec<Entry>, VotingKeypair) {
    // 1) Assume the active_keypair node has no tokens staked
    let transfer_tx = SystemTransaction::new_account(
        &token_source,
        active_keypair.pubkey(),
        stake,
        *last_tick_id,
        0,
    );
    let mut last_entry_id = *last_entry_id;
    let transfer_entry = next_entry_mut(&mut last_entry_id, 1, vec![transfer_tx]);

    // 2) Create and register a vote account for active_keypair
    let voting_keypair = VotingKeypair::new_local(active_keypair);
    let vote_account_id = voting_keypair.pubkey();

    let new_vote_account_tx =
        VoteTransaction::new_account(active_keypair, vote_account_id, *last_tick_id, 1, 1);
    let new_vote_account_entry = next_entry_mut(&mut last_entry_id, 1, vec![new_vote_account_tx]);

    // 3) Create vote entry
    let vote_tx =
        VoteTransaction::new_vote(&voting_keypair, slot_height_to_vote_on, *last_tick_id, 0);
    let vote_entry = next_entry_mut(&mut last_entry_id, 1, vec![vote_tx]);

    // 4) Create the ending empty ticks
    let mut txs = vec![transfer_entry, new_vote_account_entry, vote_entry];
    let empty_ticks = create_ticks(num_ending_ticks, last_entry_id);
    txs.extend(empty_ticks);
    (txs, voting_keypair)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::voting_keypair::tests::{new_vote_account_with_vote, push_vote};
    use hashbrown::HashSet;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::{GenesisBlock, BOOTSTRAP_LEADER_TOKENS};
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::timing::DEFAULT_SLOTS_PER_EPOCH;

    fn get_active_pubkeys(
        bank: &Bank,
        active_window_num_slots: u64,
        upper_bound: u64,
    ) -> Vec<Pubkey> {
        let stakes = get_active_stakes(bank, active_window_num_slots, upper_bound);
        stakes.into_iter().map(|x| x.0).collect()
    }

    #[test]
    fn test_active_set() {
        solana_logger::setup();

        let leader_id = Keypair::new().pubkey();
        let active_window_tick_length = 1000;
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new(&genesis_block);

        let bootstrap_ids = vec![genesis_block.bootstrap_leader_id];

        // Insert a bunch of votes at height "start_height"
        let start_height = 3;
        let num_old_ids = 20;
        let mut old_ids = vec![];
        for _ in 0..num_old_ids {
            let new_keypair = Keypair::new();
            let pk = new_keypair.pubkey();
            old_ids.push(pk);

            // Give the account some stake
            bank.transfer(5, &mint_keypair, pk, genesis_block.last_id())
                .unwrap();

            // Create a vote account and push a vote
            new_vote_account_with_vote(&new_keypair, &Keypair::new(), &bank, 1, start_height);
        }
        old_ids.sort();

        // Insert a bunch of votes at height "start_height + active_window_tick_length"
        let num_new_ids = 10;
        let mut new_ids = vec![];
        for _ in 0..num_new_ids {
            let new_keypair = Keypair::new();
            let pk = new_keypair.pubkey();
            new_ids.push(pk);
            // Give the account some stake
            bank.transfer(5, &mint_keypair, pk, genesis_block.last_id())
                .unwrap();

            // Create a vote account and push a vote
            let slot_height = start_height + active_window_tick_length + 1;
            new_vote_account_with_vote(&new_keypair, &Keypair::new(), &bank, 1, slot_height);
        }
        new_ids.sort();

        // Query for the active set at various heights
        let result = get_active_pubkeys(&bank, active_window_tick_length, 0);
        assert_eq!(result, bootstrap_ids);

        let result = get_active_pubkeys(&bank, active_window_tick_length, start_height - 1);
        assert_eq!(result, bootstrap_ids);

        let result = get_active_pubkeys(
            &bank,
            active_window_tick_length,
            active_window_tick_length + start_height - 1,
        );
        assert_eq!(result, old_ids);

        let result = get_active_pubkeys(
            &bank,
            active_window_tick_length,
            active_window_tick_length + start_height,
        );
        assert_eq!(result, old_ids);

        let result = get_active_pubkeys(
            &bank,
            active_window_tick_length,
            active_window_tick_length + start_height + 1,
        );
        assert_eq!(result, new_ids);

        let result = get_active_pubkeys(
            &bank,
            active_window_tick_length,
            2 * active_window_tick_length + start_height,
        );
        assert_eq!(result, new_ids);

        let result = get_active_pubkeys(
            &bank,
            active_window_tick_length,
            2 * active_window_tick_length + start_height + 1,
        );
        assert_eq!(result, new_ids);

        let result = get_active_pubkeys(
            &bank,
            active_window_tick_length,
            2 * active_window_tick_length + start_height + 2,
        );
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_multiple_vote() {
        let leader_keypair = Keypair::new();
        let leader_id = leader_keypair.pubkey();
        let active_window_tick_length = 1000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new(&genesis_block);

        // Bootstrap leader should be in the active set even without explicit votes
        {
            let result = get_active_pubkeys(&bank, active_window_tick_length, 0);
            assert_eq!(result, vec![leader_id]);

            let result =
                get_active_pubkeys(&bank, active_window_tick_length, active_window_tick_length);
            assert_eq!(result, vec![leader_id]);

            let result = get_active_pubkeys(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 1,
            );
            assert_eq!(result.len(), 0);
        }

        // Check that a node that votes twice in a row will get included in the active
        // window

        // Create a vote account
        let voting_keypair = Keypair::new();
        new_vote_account_with_vote(&leader_keypair, &voting_keypair, &bank, 1, 1);

        {
            let result = get_active_pubkeys(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 1,
            );
            assert_eq!(result, vec![leader_id]);

            let result = get_active_pubkeys(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 2,
            );
            assert_eq!(result.len(), 0);
        }

        // Vote at slot_height 2
        push_vote(&voting_keypair, &bank, 2);

        {
            let result = get_active_pubkeys(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 2,
            );
            assert_eq!(result, vec![leader_id]);

            let result = get_active_pubkeys(
                &bank,
                active_window_tick_length,
                active_window_tick_length + 3,
            );
            assert_eq!(result.len(), 0);
        }
    }

    fn run_scheduler_test(num_validators: u64, ticks_per_slot: u64, slots_per_epoch: u64) {
        info!(
            "run_scheduler_test({}, {}, {})",
            num_validators, ticks_per_slot, slots_per_epoch
        );
        // Allow the validators to be in the active window for the entire test
        let active_window_num_slots = slots_per_epoch;

        // Create the bank and validators, which are inserted in order of account balance
        let num_vote_account_tokens = 1;
        let (mut genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        genesis_block.ticks_per_slot = ticks_per_slot;
        genesis_block.slots_per_epoch = slots_per_epoch;

        info!("bootstrap_leader_id: {}", genesis_block.bootstrap_leader_id);

        let bank = Bank::new(&genesis_block);

        let mut validators = vec![];
        let last_id = genesis_block.last_id();
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            let voting_keypair = Keypair::new();
            validators.push(new_pubkey);
            let stake = (i + 42) as u64;
            info!("validator {}: stake={} pubkey={}", i, stake, new_pubkey);
            // Give the validator some tokens
            bank.transfer(stake, &mint_keypair, new_pubkey, last_id)
                .unwrap();

            // Vote to make the validator part of the active set for the entire test
            // (we made the active_window_num_slots large enough at the beginning of the test)
            new_vote_account_with_vote(
                &new_validator,
                &voting_keypair,
                &bank,
                num_vote_account_tokens as u64,
                slots_per_epoch,
            );
        }

        let mut leader_scheduler =
            LeaderScheduler::new_with_window_len(active_window_num_slots, &bank);

        // Generate the schedule for first epoch, bootstrap_leader will be the only leader
        leader_scheduler.generate_schedule(0, &bank);

        // The leader outside of the newly generated schedule window:
        // (0, slots_per_epoch]
        assert_eq!(
            leader_scheduler.get_leader_for_slot(0),
            Some(genesis_block.bootstrap_leader_id)
        );
        assert_eq!(leader_scheduler.get_leader_for_slot(slots_per_epoch), None);

        let ticks_per_epoch = slots_per_epoch * ticks_per_slot;
        // Generate schedule for second epoch.  This schedule won't be used but the schedule for
        // the third epoch cannot be generated without an existing schedule for the second epoch
        leader_scheduler.generate_schedule(ticks_per_epoch, &bank);

        // Generate schedule for third epoch to ensure the bootstrap leader will not be added to
        // the schedule, as the bootstrap leader did not vote in the second epoch but all other
        // validators did
        leader_scheduler.generate_schedule(2 * ticks_per_epoch, &bank);

        // For the next ticks_per_epoch entries, call get_leader_for_slot every
        // ticks_per_slot entries, and the next leader should be the next validator
        // in order of stake
        let num_slots = slots_per_epoch;
        let mut start_leader_index = None;
        for i in 0..num_slots {
            let tick_height = 2 * ticks_per_epoch + i * ticks_per_slot;
            info!("iteration {}: tick_height={}", i, tick_height);
            let slot = leader_scheduler.tick_height_to_slot(tick_height);
            let current_leader = leader_scheduler
                .get_leader_for_slot(slot)
                .expect("Expected a leader from scheduler");
            info!("current_leader={} slot={}", current_leader, slot);

            // Note: The "validators" vector is already sorted by stake, so the expected order
            // for the leader schedule can be derived by just iterating through the vector
            // in order. The only exception is for the bootstrap leader in the schedule, we need to
            // find the index into the "validators" vector where the schedule begins.
            if None == start_leader_index {
                start_leader_index = Some(
                    validators
                        .iter()
                        .position(|v| *v == current_leader)
                        .unwrap(),
                );
            }

            let expected_leader =
                validators[((start_leader_index.unwrap() as u64 + i) % num_validators) as usize];
            assert_eq!(current_leader, expected_leader);
            assert_eq!(
                slot,
                leader_scheduler.tick_height_to_slot(2 * ticks_per_epoch) + i
            );
            assert_eq!(
                slot,
                leader_scheduler.tick_height_to_slot(tick_height + ticks_per_slot - 1)
            );
            assert_eq!(
                leader_scheduler.get_leader_for_slot(slot),
                Some(current_leader)
            );
        }
    }

    #[test]
    fn test_leader_after_genesis() {
        solana_logger::setup();
        let leader_id = Keypair::new().pubkey();
        let leader_tokens = 2;
        let (genesis_block, _) = GenesisBlock::new_with_leader(5, leader_id, leader_tokens);
        let bank = Bank::new(&genesis_block);
        let leader_scheduler = LeaderScheduler::new_with_bank(&bank);
        let slot = leader_scheduler.tick_height_to_slot(bank.tick_height());
        assert_eq!(leader_scheduler.get_leader_for_slot(slot), Some(leader_id));
    }

    #[test]
    fn test_num_ticks_left_in_block() {
        let leader_scheduler = LeaderScheduler::new(&LeaderSchedulerConfig::new(10, 2, 1));

        assert_eq!(leader_scheduler.num_ticks_left_in_block(0, 0), 9);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(1, 0), 19);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(0, 1), 8);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(0, 8), 1);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(0, 9), 0);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(1, 10), 9);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(1, 11), 8);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(1, 19), 0);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(2, 20), 9);
        assert_eq!(leader_scheduler.num_ticks_left_in_block(2, 21), 8);
    }

    #[test]
    fn test_num_ticks_left_in_slot() {
        let leader_scheduler = LeaderScheduler::new(&LeaderSchedulerConfig::new(10, 2, 1));
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(0), 9);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(1), 8);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(8), 1);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(9), 0);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(10), 9);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(11), 8);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(19), 0);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(20), 9);
        assert_eq!(leader_scheduler.num_ticks_left_in_slot(21), 8);
    }

    #[test]
    fn test_seed() {
        // Check that num_seeds different seeds are generated
        let num_seeds = 1000;
        let mut old_seeds = HashSet::new();
        for i in 0..num_seeds {
            let seed = LeaderScheduler::calculate_seed(i);
            assert!(!old_seeds.contains(&seed));
            old_seeds.insert(seed);
        }
    }

    #[test]
    fn test_choose_account() {
        let tokens = vec![10, 30, 50, 5, 1];
        let total_tokens = tokens.iter().sum();
        let mut seed = tokens[0];
        assert_eq!(
            LeaderScheduler::choose_account(tokens.clone(), seed, total_tokens),
            1
        );

        seed = tokens[0] - 1;
        assert_eq!(
            LeaderScheduler::choose_account(tokens.clone(), seed, total_tokens),
            0
        );

        seed = 0;
        assert_eq!(
            LeaderScheduler::choose_account(tokens.clone(), seed, total_tokens),
            0
        );

        seed = total_tokens;
        assert_eq!(
            LeaderScheduler::choose_account(tokens.clone(), seed, total_tokens),
            0
        );

        seed = total_tokens - 1;
        assert_eq!(
            LeaderScheduler::choose_account(tokens.clone(), seed, total_tokens),
            tokens.len() - 1
        );

        seed = tokens[0..3].iter().sum();
        assert_eq!(
            LeaderScheduler::choose_account(tokens.clone(), seed, total_tokens),
            3
        );
    }

    #[test]
    fn test_scheduler_basic() {
        solana_logger::setup();
        // Test when the number of validators equals
        // ticks_per_epoch / ticks_per_slot, so each validator
        // is selected once
        let mut num_validators = 100;
        let mut ticks_per_slot = 100;

        run_scheduler_test(num_validators, ticks_per_slot, num_validators);

        // Test when there are fewer validators than
        // ticks_per_epoch / ticks_per_slot, so each validator
        // is selected multiple times
        num_validators = 3;
        ticks_per_slot = 100;
        run_scheduler_test(num_validators, ticks_per_slot, num_validators);

        // Test when there are fewer number of validators than
        // ticks_per_epoch / ticks_per_slot, so each validator
        // may not be selected
        num_validators = 10;
        ticks_per_slot = 100;
        run_scheduler_test(num_validators, ticks_per_slot, num_validators);

        // Test when ticks_per_epoch == ticks_per_slot,
        // only one validator should be selected
        num_validators = 10;
        ticks_per_slot = 2;
        run_scheduler_test(num_validators, ticks_per_slot, num_validators);
    }

    #[test]
    fn test_scheduler_active_window() {
        solana_logger::setup();

        let num_validators = 10;
        let num_vote_account_tokens = 1;

        // Make sure ticks_per_epoch is big enough so we select all the
        // validators as part of the schedule each time (we need to check the active window
        // is the cause of validators being truncated later)
        let ticks_per_slot = 100;
        let slots_per_epoch = num_validators;
        let active_window_num_slots = slots_per_epoch;

        // Create the bazzznk and validators
        let (mut genesis_block, mint_keypair) = GenesisBlock::new(
            ((((num_validators + 1) / 2) * (num_validators + 1))
                + (num_vote_account_tokens * num_validators)) as u64,
        );
        genesis_block.ticks_per_slot = ticks_per_slot;
        genesis_block.slots_per_epoch = slots_per_epoch;
        let bank = Bank::new(&genesis_block);
        let mut leader_scheduler =
            LeaderScheduler::new_with_window_len(active_window_num_slots, &bank);

        let mut validators = vec![];
        let last_id = genesis_block.last_id();
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            let voting_keypair = Keypair::new();
            validators.push(new_pubkey);
            // Give the validator some tokens
            bank.transfer(
                (i + 1 + num_vote_account_tokens) as u64,
                &mint_keypair,
                new_pubkey,
                last_id,
            )
            .unwrap();

            // Create a vote account and push a vote
            let tick_height = (i + 2) * active_window_num_slots - 1;
            new_vote_account_with_vote(&new_validator, &voting_keypair, &bank, 1, tick_height);
        }

        // Generate schedule every active_window_num_slots entries and check that
        // validators are falling out of the rotation as they fall out of the
        // active set
        trace!("bootstrap_leader_id: {}", genesis_block.bootstrap_leader_id);
        for i in 0..num_validators {
            trace!("validators[{}]: {}", i, validators[i as usize]);
        }
        assert_eq!(leader_scheduler.current_epoch, 0);
        leader_scheduler.generate_schedule(1, &bank);
        assert_eq!(leader_scheduler.current_epoch, 0);
        for i in 0..=num_validators {
            info!("i === {}", i);
            leader_scheduler
                .generate_schedule((i + 1) * ticks_per_slot * active_window_num_slots, &bank);
            assert_eq!(leader_scheduler.current_epoch, i + 1);
            if i == 0 {
                assert_eq!(
                    vec![genesis_block.bootstrap_leader_id],
                    leader_scheduler.epoch_schedule[0],
                );
            } else {
                assert_eq!(
                    vec![validators[(i - 1) as usize]],
                    leader_scheduler.epoch_schedule[0],
                );
            };
        }
    }

    #[test]
    fn test_update_tick_height() {
        solana_logger::setup();

        let ticks_per_slot = 100;
        let slots_per_epoch = 2;
        let ticks_per_epoch = ticks_per_slot * slots_per_epoch;
        let active_window_num_slots = 1;

        // Check that the generate_schedule() function is being called by the
        // update_tick_height() function at the correct entry heights.
        let (mut genesis_block, _) = GenesisBlock::new(10_000);
        genesis_block.ticks_per_slot = ticks_per_slot;
        genesis_block.slots_per_epoch = slots_per_epoch;

        let bank = Bank::new(&genesis_block);
        let mut leader_scheduler =
            LeaderScheduler::new_with_window_len(active_window_num_slots, &bank);
        info!(
            "bootstrap_leader_id: {:?}",
            genesis_block.bootstrap_leader_id
        );
        assert_eq!(bank.tick_height(), 0);

        //
        // Check various tick heights in epoch 0 up to the last tick
        //
        for tick_height in &[
            0,
            1,
            ticks_per_slot,
            ticks_per_slot + 1,
            ticks_per_epoch - 2,
        ] {
            info!("Checking tick_height {}", *tick_height);
            leader_scheduler.update_tick_height(*tick_height, &bank);
            assert_eq!(leader_scheduler.current_epoch, 0);
            // The schedule for epoch 0 is known
            assert_eq!(
                leader_scheduler.get_leader_for_slot(0),
                Some(genesis_block.bootstrap_leader_id)
            );
            assert_eq!(
                leader_scheduler.get_leader_for_slot(1),
                Some(genesis_block.bootstrap_leader_id)
            );
            // The schedule for epoch 1 is unknown
            assert_eq!(leader_scheduler.get_leader_for_slot(2), None);
        }

        //
        // Check the last tick of epoch 0, various tick heights in epoch 1 up to the last tick
        //
        for tick_height in &[
            ticks_per_epoch - 1,
            ticks_per_epoch,
            ticks_per_epoch + 1,
            ticks_per_epoch + ticks_per_slot,
            ticks_per_epoch + ticks_per_slot + 1,
            ticks_per_epoch + ticks_per_epoch - 2,
            //  ticks_per_epoch + ticks_per_epoch,
        ] {
            info!("Checking tick_height {}", *tick_height);
            leader_scheduler.update_tick_height(*tick_height, &bank);
            assert_eq!(leader_scheduler.current_epoch, 1);
            // The schedule for epoch 0 is known
            assert_eq!(
                leader_scheduler.get_leader_for_slot(0),
                Some(genesis_block.bootstrap_leader_id)
            );
            assert_eq!(
                leader_scheduler.get_leader_for_slot(1),
                Some(genesis_block.bootstrap_leader_id)
            );

            // The schedule for epoch 1 is known
            assert_eq!(
                leader_scheduler.get_leader_for_slot(2),
                Some(genesis_block.bootstrap_leader_id)
            );
            assert_eq!(
                leader_scheduler.get_leader_for_slot(3),
                Some(genesis_block.bootstrap_leader_id)
            );
            // The schedule for epoch 2 is unknown
            assert_eq!(leader_scheduler.get_leader_for_slot(6), None);
        }

        leader_scheduler.update_tick_height(ticks_per_epoch + ticks_per_epoch - 1, &bank);
        assert_eq!(leader_scheduler.current_epoch, 2);
        // The schedule for epoch 0 is unknown
        assert_eq!(leader_scheduler.get_leader_for_slot(0), None);
        assert_eq!(leader_scheduler.get_leader_for_slot(1), None);
        // The schedule for epoch 1 is known
        assert_eq!(
            leader_scheduler.get_leader_for_slot(2),
            Some(genesis_block.bootstrap_leader_id)
        );
        assert_eq!(
            leader_scheduler.get_leader_for_slot(3),
            Some(genesis_block.bootstrap_leader_id)
        );
        // The schedule for epoch 2 is known
        assert_eq!(
            leader_scheduler.get_leader_for_slot(4),
            Some(genesis_block.bootstrap_leader_id)
        );
        assert_eq!(
            leader_scheduler.get_leader_for_slot(5),
            Some(genesis_block.bootstrap_leader_id)
        );
        // The schedule for epoch 3 is unknown
        assert_eq!(leader_scheduler.get_leader_for_slot(6), None);
    }

    #[test]
    fn test_constructors() {
        // Check defaults for LeaderScheduler
        let leader_scheduler_config = LeaderSchedulerConfig::default();

        let leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        assert_eq!(leader_scheduler.ticks_per_slot, DEFAULT_TICKS_PER_SLOT);
        assert_eq!(leader_scheduler.slots_per_epoch, DEFAULT_SLOTS_PER_EPOCH);

        // Check actual arguments for LeaderScheduler
        let ticks_per_slot = 100;
        let slots_per_epoch = 2;
        let active_window_num_slots = 1;

        let leader_scheduler_config =
            LeaderSchedulerConfig::new(ticks_per_slot, slots_per_epoch, active_window_num_slots);

        let leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        assert_eq!(leader_scheduler.ticks_per_slot, ticks_per_slot);
        assert_eq!(leader_scheduler.slots_per_epoch, slots_per_epoch);
    }

    fn run_consecutive_leader_test(slots_per_epoch: u64, add_validator: bool) {
        let bootstrap_leader_keypair = Arc::new(Keypair::new());
        let bootstrap_leader_id = bootstrap_leader_keypair.pubkey();
        let ticks_per_slot = 100;
        let active_window_num_slots = slots_per_epoch;

        // Create mint and bank
        let (genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(10_000, bootstrap_leader_id, BOOTSTRAP_LEADER_TOKENS);
        let bank = Bank::new(&genesis_block);
        let last_id = genesis_block.last_id();
        let initial_vote_height = 1;

        // Create and add validator to the active set
        let validator_keypair = Arc::new(Keypair::new());
        let validator_id = validator_keypair.pubkey();
        if add_validator {
            bank.transfer(5, &mint_keypair, validator_id, last_id)
                .unwrap();

            // Create a vote account and push a vote
            new_vote_account_with_vote(
                &validator_keypair,
                &Keypair::new(),
                &bank,
                1,
                initial_vote_height,
            );
        }

        // Make sure the bootstrap leader, not the validator, is picked again on next slot
        // Depending on the seed, we make the leader stake either 2, or 3. Because the
        // validator stake is always 1, then the rankings will always be
        // [(validator, 1), (leader, leader_stake)]. Thus we just need to make sure that
        // seed % (leader_stake + 1) > 0 to make sure that the leader is picked again.
        let seed = LeaderScheduler::calculate_seed(0);
        let leader_stake = if seed % 3 == 0 { 3 } else { 2 };

        let vote_account_tokens = 1;
        bank.transfer(
            leader_stake + vote_account_tokens,
            &mint_keypair,
            bootstrap_leader_id,
            last_id,
        )
        .unwrap();

        // Create a vote account and push a vote to add the leader to the active set
        let voting_keypair = Keypair::new();
        new_vote_account_with_vote(
            &bootstrap_leader_keypair,
            &voting_keypair,
            &bank,
            vote_account_tokens as u64,
            0,
        );

        let leader_scheduler_config =
            LeaderSchedulerConfig::new(ticks_per_slot, slots_per_epoch, active_window_num_slots);
        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        leader_scheduler.generate_schedule(0, &bank);
        assert_eq!(leader_scheduler.current_epoch, 0);
        assert_eq!(leader_scheduler.epoch_schedule[0], [bootstrap_leader_id]);

        // Make sure the validator, not the leader is selected on the first slot of the
        // next epoch
        leader_scheduler.generate_schedule(ticks_per_slot * slots_per_epoch, &bank);
        assert_eq!(leader_scheduler.current_epoch, 1);
        if add_validator {
            assert_eq!(leader_scheduler.epoch_schedule[0][0], validator_id);
        } else {
            assert_eq!(leader_scheduler.epoch_schedule[0][0], bootstrap_leader_id);
        }
    }

    #[test]
    fn test_avoid_consecutive_leaders() {
        // Test when there is both a leader + validator in the active set
        run_consecutive_leader_test(1, true);
        run_consecutive_leader_test(2, true);
        run_consecutive_leader_test(10, true);

        // Test when there is only one node in the active set
        run_consecutive_leader_test(1, false);
        run_consecutive_leader_test(2, false);
        run_consecutive_leader_test(10, false);
    }

    #[test]
    fn test_sort_stakes_basic() {
        let pubkey0 = Keypair::new().pubkey();
        let pubkey1 = Keypair::new().pubkey();
        let mut stakes = vec![(pubkey0, 2), (pubkey1, 1)];
        sort_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey1, 1), (pubkey0, 2)]);
    }

    #[test]
    fn test_sort_stakes_with_dup() {
        let pubkey0 = Keypair::new().pubkey();
        let pubkey1 = Keypair::new().pubkey();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 2), (pubkey0, 1)];
        sort_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey0, 1), (pubkey1, 2)]);
    }

    #[test]
    fn test_sort_stakes_with_equal_stakes() {
        let pubkey0 = Pubkey::default();
        let pubkey1 = Keypair::new().pubkey();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 1)];
        sort_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey0, 1), (pubkey1, 1)]);
    }
}
