//! The `leader_scheduler` module implements a structure and functions for tracking and
//! managing the schedule for leader rotation

use crate::bank::Bank;
use crate::entry::{create_ticks, Entry};
use crate::voting_keypair::VotingKeypair;
use bincode::serialize;
use byteorder::{LittleEndian, ReadBytesExt};
use hashbrown::HashSet;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::vote_program::{self, VoteState};
use solana_sdk::vote_transaction::VoteTransaction;
use std::io::Cursor;
use std::sync::Arc;

/*
// At 10 ticks/s, 8 ticks per slot implies that leader rotation and voting will happen
// every 800 ms. A fast voting cadence ensures faster finality and convergence
pub const DEFAULT_TICKS_PER_SLOT: u64 = 8;
*/
pub const DEFAULT_TICKS_PER_SLOT: u64 = 64; // TODO: DEFAULT_TICKS_PER_SLOT = 8 causes instability in the integration tests.
pub const DEFAULT_SLOTS_PER_EPOCH: u64 = 64;
pub const DEFAULT_SEED_ROTATION_INTERVAL: u64 = DEFAULT_SLOTS_PER_EPOCH * DEFAULT_TICKS_PER_SLOT;
pub const DEFAULT_ACTIVE_WINDOW_LENGTH: u64 = DEFAULT_SEED_ROTATION_INTERVAL;

#[derive(Clone)]
pub struct LeaderSchedulerConfig {
    // The interval at which to rotate the leader, should be much less than
    // seed_rotation_interval
    pub leader_rotation_interval: u64,

    // The interval at which to generate the seed used for ranking the validators
    pub seed_rotation_interval: u64,

    // The length of the acceptable window for determining live validators
    pub active_window_length: u64,
}

// Used to toggle leader rotation in fullnode so that tests that don't
// need leader rotation don't break
impl LeaderSchedulerConfig {
    pub fn new(
        leader_rotation_interval: u64,
        seed_rotation_interval: u64,
        active_window_length: u64,
    ) -> Self {
        LeaderSchedulerConfig {
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        }
    }
}

impl Default for LeaderSchedulerConfig {
    fn default() -> Self {
        Self {
            leader_rotation_interval: DEFAULT_TICKS_PER_SLOT,
            seed_rotation_interval: DEFAULT_SEED_ROTATION_INTERVAL,
            active_window_length: DEFAULT_ACTIVE_WINDOW_LENGTH,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LeaderScheduler {
    // A leader slot duration in ticks
    pub leader_rotation_interval: u64,

    // Duration of an epoch (one or more slots) in ticks.
    // This value must be divisible by leader_rotation_interval
    pub seed_rotation_interval: u64,

    // The length of time in ticks for which a vote qualifies a candidate for leader
    // selection
    pub active_window_length: u64,

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
// From this point on, every seed_rotation_interval PoH counts we generate the seed based
// on the PoH height, and use it to do a weighted sample from the set
// of validators based on current stake weight. This gets you the bootstrap leader A for
// the next leader_rotation_interval PoH counts. On the same PoH count we generate the seed,
// we also order the validators based on their current stake weight, and starting
// from leader A, we then pick the next leader sequentially every leader_rotation_interval
// PoH counts based on this fixed ordering, so the next
// seed_rotation_interval / leader_rotation_interval leaders are determined.
//
// 2) When we we hit the next seed rotation PoH height, step 1) is executed again to
// calculate the leader schedule for the upcoming seed_rotation_interval PoH counts.
impl LeaderScheduler {
    pub fn new(config: &LeaderSchedulerConfig) -> Self {
        let leader_rotation_interval = config.leader_rotation_interval;
        let seed_rotation_interval = config.seed_rotation_interval;
        let active_window_length = config.active_window_length;

        // Enforced invariants
        assert!(seed_rotation_interval >= leader_rotation_interval);
        assert!(seed_rotation_interval % leader_rotation_interval == 0);

        LeaderScheduler {
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
            seed: 0,
            epoch_schedule: [Vec::new(), Vec::new()],
            current_epoch: 0,
        }
    }

    pub fn tick_height_to_slot(&self, tick_height: u64) -> u64 {
        tick_height / self.leader_rotation_interval
    }

    fn tick_height_to_epoch(&self, tick_height: u64) -> u64 {
        tick_height / self.seed_rotation_interval
    }

    // Returns the number of ticks remaining from the specified tick_height to the end of the
    // current slot
    pub fn num_ticks_left_in_slot(&self, tick_height: u64) -> u64 {
        self.leader_rotation_interval - tick_height % self.leader_rotation_interval - 1
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
        if epoch < self.current_epoch {
            return;
        }

        // Leader schedule is computed at tick 0 (for bootstrap) and then on the second tick 1 for
        // the current slot, so that generally the schedule applies to the range [slot N tick 1,
        // slot N+1 tick 0). The schedule is shifted right 1 tick from the slot rotation interval so that
        // the next leader is always known *before* a rotation occurs
        if tick_height == 0 || tick_height % self.seed_rotation_interval == 1 {
            self.generate_schedule(tick_height, bank);
        }
    }

    // Returns the leader for the requested slot, or None if the slot is out of the schedule bounds
    pub fn get_leader_for_slot(&self, slot: u64) -> Option<Pubkey> {
        trace!("get_leader_for_slot: slot {}", slot);
        let tick_height = slot * self.leader_rotation_interval;
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

            let first_tick_in_epoch = epoch * self.seed_rotation_interval;
            let slot_index = (tick_height - first_tick_in_epoch) / self.leader_rotation_interval;

            // Round robin through each node in the schedule
            Some(schedule[slot_index as usize % schedule.len()])
        }
    }

    // TODO: We use a HashSet for now because a single validator could potentially register
    // multiple vote account. Once that is no longer possible (see the TODO in vote_program.rs,
    // process_transaction(), case VoteInstruction::RegisterAccount), we can use a vector.
    fn get_active_set(&mut self, tick_height: u64, bank: &Bank) -> HashSet<Pubkey> {
        let upper_bound = tick_height;
        let lower_bound = tick_height.saturating_sub(self.active_window_length);
        trace!(
            "get_active_set: vote bounds ({}, {})",
            lower_bound,
            upper_bound
        );

        {
            let accounts = bank.accounts.accounts_db.read().unwrap();

            // TODO: iterate through checkpoints, too
            accounts
                .accounts
                .values()
                .filter_map(|account| {
                    if vote_program::check_id(&account.owner) {
                        if let Ok(vote_state) = VoteState::deserialize(&account.userdata) {
                            trace!("get_active_set: account vote_state: {:?}", vote_state);
                            return vote_state
                                .votes
                                .back()
                                .filter(|vote| {
                                    vote.tick_height >= lower_bound
                                        && vote.tick_height <= upper_bound
                                })
                                .map(|_| vote_state.staker_id);
                        }
                    }

                    None
                })
                .collect()
        }
    }

    // Updates the leader schedule to include ticks from tick_height to the first tick of the next epoch
    fn generate_schedule(&mut self, tick_height: u64, bank: &Bank) {
        let epoch = if tick_height == 0 {
            0
        } else {
            self.tick_height_to_epoch(tick_height) + 1
        };
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
        let active_set = self.get_active_set(tick_height, &bank);
        let ranked_active_set = Self::rank_active_set(bank, active_set.iter());

        if ranked_active_set.is_empty() {
            info!(
                "generate_schedule: empty ranked_active_set at tick_height {}, using leader_schedule from previous epoch",
                tick_height,
            );
        } else {
            let (mut validator_rankings, total_stake) = ranked_active_set.iter().fold(
                (Vec::with_capacity(ranked_active_set.len()), 0),
                |(mut ids, total_stake), (pubkey, stake)| {
                    ids.push(**pubkey);
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
                    let slots_per_epoch =
                        self.seed_rotation_interval / self.leader_rotation_interval;
                    if slots_per_epoch == 1 {
                        // If there is only one slot per epoch, and the same leader as the last slot
                        // of the previous epoch was chosen, then pick the next leader in the
                        // rankings instead
                        validator_rankings[0] = validator_rankings[1];
                    } else {
                        // If there is more than one leader in the schedule, truncate and set the most
                        // recent leader to the back of the line. This way that node will still remain
                        // in the rotation, just at a later slot.
                        validator_rankings.truncate(slots_per_epoch as usize);
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
            tick_height + self.seed_rotation_interval,
            self.epoch_schedule[0]
        );
    }

    fn rank_active_set<'a, I>(bank: &Bank, active: I) -> Vec<(&'a Pubkey, u64)>
    where
        I: Iterator<Item = &'a Pubkey>,
    {
        let mut active_accounts: Vec<(&'a Pubkey, u64)> = active
            .filter_map(|pubkey| {
                let stake = bank.get_balance(pubkey);
                if stake > 0 {
                    Some((pubkey, stake as u64))
                } else {
                    None
                }
            })
            .collect();

        active_accounts.sort_by(|(pubkey1, stake1), (pubkey2, stake2)| {
            if stake1 == stake2 {
                pubkey1.cmp(&pubkey2)
            } else {
                stake1.cmp(&stake2)
            }
        });
        active_accounts
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
        let config = Default::default();
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
    tick_height_to_vote_on: u64,
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
    let transfer_entry = Entry::new(last_entry_id, 0, 1, vec![transfer_tx]);
    let mut last_entry_id = transfer_entry.id;

    // 2) Create and register a vote account for active_keypair
    let voting_keypair = VotingKeypair::new_local(active_keypair);
    let vote_account_id = voting_keypair.pubkey();

    let new_vote_account_tx =
        VoteTransaction::new_account(active_keypair, vote_account_id, *last_tick_id, 1, 1);
    let new_vote_account_entry = Entry::new(&last_entry_id, 0, 1, vec![new_vote_account_tx]);
    last_entry_id = new_vote_account_entry.id;

    // 3) Create vote entry
    let vote_tx =
        VoteTransaction::new_vote(&voting_keypair, tick_height_to_vote_on, *last_tick_id, 0);
    let vote_entry = Entry::new(&last_entry_id, 0, 1, vec![vote_tx]);
    last_entry_id = vote_entry.id;

    // 4) Create the ending empty ticks
    let mut txs = vec![transfer_entry, new_vote_account_entry, vote_entry];
    let empty_ticks = create_ticks(num_ending_ticks, last_entry_id);
    txs.extend(empty_ticks);
    (txs, voting_keypair)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::genesis_block::{GenesisBlock, BOOTSTRAP_LEADER_TOKENS};
    use hashbrown::HashSet;
    use std::hash::Hash as StdHash;
    use std::iter::FromIterator;

    fn to_hashset_owned<T>(slice: &[T]) -> HashSet<T>
    where
        T: Eq + StdHash + Clone,
    {
        HashSet::from_iter(slice.iter().cloned())
    }

    pub fn new_vote_account(
        from_keypair: &Keypair,
        voting_keypair: &VotingKeypair,
        bank: &Bank,
        num_tokens: u64,
        last_id: Hash,
    ) {
        let tx = VoteTransaction::new_account(
            from_keypair,
            voting_keypair.pubkey(),
            last_id,
            num_tokens,
            0,
        );
        bank.process_transaction(&tx).unwrap();
    }

    fn push_vote(voting_keypair: &VotingKeypair, bank: &Bank, tick_height: u64, last_id: Hash) {
        let new_vote_tx = VoteTransaction::new_vote(voting_keypair, tick_height, last_id, 0);
        bank.process_transaction(&new_vote_tx).unwrap();
    }

    fn run_scheduler_test(
        num_validators: usize,
        leader_rotation_interval: u64,
        seed_rotation_interval: u64,
    ) {
        info!(
            "run_scheduler_test({}, {}, {})",
            num_validators, leader_rotation_interval, seed_rotation_interval
        );
        // Allow the validators to be in the active window for the entire test
        let active_window_length = seed_rotation_interval;

        // Set up the LeaderScheduler struct
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        // Create the bank and validators, which are inserted in order of account balance
        let num_vote_account_tokens = 1;
        let (genesis_block, mint_keypair) = GenesisBlock::new(10_000);
        info!("bootstrap_leader_id: {}", genesis_block.bootstrap_leader_id);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        let mut validators = vec![];
        let last_id = genesis_block.last_id();
        for i in 0..num_validators {
            let new_validator = Arc::new(Keypair::new());
            let new_pubkey = new_validator.pubkey();
            let voting_keypair = VotingKeypair::new_local(&new_validator);
            validators.push(new_pubkey);
            let stake = (i + 42) as u64;
            info!("validator {}: stake={} pubkey={}", i, stake, new_pubkey);
            // Give the validator some tokens
            bank.transfer(stake, &mint_keypair, new_pubkey, last_id)
                .unwrap();

            // Create a vote account
            new_vote_account(
                &new_validator,
                &voting_keypair,
                &bank,
                num_vote_account_tokens as u64,
                genesis_block.last_id(),
            );

            // Vote to make the validator part of the active set for the entire test
            // (we made the active_window_length large enough at the beginning of the test)
            push_vote(
                &voting_keypair,
                &bank,
                seed_rotation_interval,
                genesis_block.last_id(),
            );
        }

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        // Generate the schedule for first epoch, bootstrap_leader will be the only leader
        leader_scheduler.generate_schedule(0, &bank);

        // The leader outside of the newly generated schedule window:
        // (0, seed_rotation_interval]
        info!("yyy");
        assert_eq!(
            leader_scheduler.get_leader_for_slot(0),
            Some(genesis_block.bootstrap_leader_id)
        );
        info!("xxxx");
        assert_eq!(
            leader_scheduler
                .get_leader_for_slot(leader_scheduler.tick_height_to_slot(seed_rotation_interval)),
            None
        );

        // Generate schedule for second epoch.  This schedule won't be used but the schedule for
        // the third epoch cannot be generated without an existing schedule for the second epoch
        leader_scheduler.generate_schedule(1, &bank);

        // Generate schedule for third epoch to ensure the bootstrap leader will not be added to
        // the schedule, as the bootstrap leader did not vote in the second epoch but all other
        // validators did
        leader_scheduler.generate_schedule(seed_rotation_interval + 1, &bank);

        // For the next seed_rotation_interval entries, call get_leader_for_slot every
        // leader_rotation_interval entries, and the next leader should be the next validator
        // in order of stake
        let num_slots = seed_rotation_interval / leader_rotation_interval;
        let mut start_leader_index = None;
        for i in 0..num_slots {
            let tick_height = 2 * seed_rotation_interval + i * leader_rotation_interval;
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
                validators[(start_leader_index.unwrap() + i as usize) % num_validators];
            assert_eq!(current_leader, expected_leader);
            assert_eq!(
                slot,
                leader_scheduler.tick_height_to_slot(2 * seed_rotation_interval) + i
            );
            assert_eq!(
                slot,
                leader_scheduler.tick_height_to_slot(tick_height + leader_rotation_interval - 1)
            );
            assert_eq!(
                leader_scheduler.get_leader_for_slot(slot),
                Some(current_leader)
            );
        }
    }

    #[test]
    fn test_num_ticks_left_in_slot() {
        let leader_scheduler = LeaderScheduler::new(&LeaderSchedulerConfig::new(10, 20, 0));

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
    fn test_active_set() {
        solana_logger::setup();

        let leader_id = Keypair::new().pubkey();
        let active_window_length = 1000;
        let leader_scheduler_config = LeaderSchedulerConfig::new(100, 100, active_window_length);
        let (genesis_block, mint_keypair) = GenesisBlock::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);

        let bootstrap_ids = to_hashset_owned(&vec![genesis_block.bootstrap_leader_id]);

        // Insert a bunch of votes at height "start_height"
        let start_height = 3;
        let num_old_ids = 20;
        let mut old_ids = HashSet::new();
        for _ in 0..num_old_ids {
            let new_keypair = Arc::new(Keypair::new());
            let pk = new_keypair.pubkey();
            old_ids.insert(pk.clone());

            // Give the account some stake
            bank.transfer(5, &mint_keypair, pk, genesis_block.last_id())
                .unwrap();

            // Create a vote account
            let voting_keypair = VotingKeypair::new_local(&new_keypair);
            new_vote_account(
                &new_keypair,
                &voting_keypair,
                &bank,
                1,
                genesis_block.last_id(),
            );

            // Push a vote for the account
            push_vote(
                &voting_keypair,
                &bank,
                start_height,
                genesis_block.last_id(),
            );
        }

        // Insert a bunch of votes at height "start_height + active_window_length"
        let num_new_ids = 10;
        let mut new_ids = HashSet::new();
        for _ in 0..num_new_ids {
            let new_keypair = Arc::new(Keypair::new());
            let pk = new_keypair.pubkey();
            new_ids.insert(pk);
            // Give the account some stake
            bank.transfer(5, &mint_keypair, pk, genesis_block.last_id())
                .unwrap();

            // Create a vote account
            let voting_keypair = VotingKeypair::new_local(&new_keypair);
            new_vote_account(
                &new_keypair,
                &voting_keypair,
                &bank,
                1,
                genesis_block.last_id(),
            );

            push_vote(
                &voting_keypair,
                &bank,
                start_height + active_window_length + 1,
                genesis_block.last_id(),
            );
        }

        // Query for the active set at various heights
        let mut leader_scheduler = bank.leader_scheduler.write().unwrap();

        let result = leader_scheduler.get_active_set(0, &bank);
        assert_eq!(result, bootstrap_ids);

        let result = leader_scheduler.get_active_set(start_height - 1, &bank);
        assert_eq!(result, bootstrap_ids);

        let result =
            leader_scheduler.get_active_set(active_window_length + start_height - 1, &bank);
        assert_eq!(result, old_ids);

        let result = leader_scheduler.get_active_set(active_window_length + start_height, &bank);
        assert_eq!(result, old_ids);

        let result =
            leader_scheduler.get_active_set(active_window_length + start_height + 1, &bank);
        assert_eq!(result, new_ids);

        let result =
            leader_scheduler.get_active_set(2 * active_window_length + start_height, &bank);
        assert_eq!(result, new_ids);

        let result =
            leader_scheduler.get_active_set(2 * active_window_length + start_height + 1, &bank);
        assert_eq!(result, new_ids);

        let result =
            leader_scheduler.get_active_set(2 * active_window_length + start_height + 2, &bank);
        assert!(result.is_empty());
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
    fn test_rank_active_set() {
        let num_validators: usize = 101;
        // Give genesis_block sum(1..num_validators) tokens
        let (genesis_block, mint_keypair) = GenesisBlock::new(
            BOOTSTRAP_LEADER_TOKENS + (((num_validators + 1) / 2) * (num_validators + 1)) as u64,
        );
        let bank = Bank::new(&genesis_block);
        let mut validators = vec![];
        let last_id = genesis_block.last_id();
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            validators.push(new_validator);
            bank.transfer(
                (num_validators - i) as u64,
                &mint_keypair,
                new_pubkey,
                last_id,
            )
            .unwrap();
        }

        let validators_pubkey: Vec<Pubkey> = validators.iter().map(Keypair::pubkey).collect();
        let result = LeaderScheduler::rank_active_set(&bank, validators_pubkey.iter());

        assert_eq!(result.len(), validators.len());

        // Expect the result to be the reverse of the list we passed into the rank_active_set()
        for (i, (pubkey, stake)) in result.into_iter().enumerate() {
            assert_eq!(stake, i as u64 + 1);
            assert_eq!(*pubkey, validators[num_validators - i - 1].pubkey());
        }

        // Transfer all the tokens to a new set of validators, old validators should now
        // have balance of zero and get filtered out of the rankings
        let mut new_validators = vec![];
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            new_validators.push(new_validator);
            bank.transfer(
                (num_validators - i) as u64,
                &validators[i],
                new_pubkey,
                last_id,
            )
            .unwrap();
        }

        let all_validators: Vec<Pubkey> = validators
            .iter()
            .chain(new_validators.iter())
            .map(Keypair::pubkey)
            .collect();
        let result = LeaderScheduler::rank_active_set(&bank, all_validators.iter());
        assert_eq!(result.len(), new_validators.len());

        for (i, (pubkey, balance)) in result.into_iter().enumerate() {
            assert_eq!(balance, i as u64 + 1);
            assert_eq!(*pubkey, new_validators[num_validators - i - 1].pubkey());
        }

        // Break ties between validators with the same balances using public key
        let (genesis_block, mint_keypair) =
            GenesisBlock::new(BOOTSTRAP_LEADER_TOKENS + (num_validators + 1) as u64);
        let bank = Bank::new(&genesis_block);
        let mut tied_validators_pk = vec![];
        let last_id = genesis_block.last_id();

        for _i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            tied_validators_pk.push(new_pubkey);
            assert!(bank.get_balance(&mint_keypair.pubkey()) > 1);
            bank.transfer(1, &mint_keypair, new_pubkey, last_id)
                .unwrap();
        }

        let result = LeaderScheduler::rank_active_set(&bank, tied_validators_pk.iter());
        let mut sorted: Vec<&Pubkey> = tied_validators_pk.iter().map(|x| x).collect();
        sorted.sort_by(|pk1, pk2| pk1.cmp(pk2));
        assert_eq!(result.len(), tied_validators_pk.len());
        for (i, (pk, s)) in result.into_iter().enumerate() {
            assert_eq!(s, 1);
            assert_eq!(*pk, *sorted[i]);
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
        // seed_rotation_interval / leader_rotation_interval, so each validator
        // is selected once
        let mut num_validators = 100;
        let mut leader_rotation_interval = 100;
        let mut seed_rotation_interval = leader_rotation_interval * num_validators as u64;

        run_scheduler_test(
            num_validators,
            leader_rotation_interval,
            seed_rotation_interval,
        );

        // Test when there are fewer validators than
        // seed_rotation_interval / leader_rotation_interval, so each validator
        // is selected multiple times
        num_validators = 3;
        leader_rotation_interval = 100;
        seed_rotation_interval = 1000;
        run_scheduler_test(
            num_validators,
            leader_rotation_interval,
            seed_rotation_interval,
        );

        // Test when there are fewer number of validators than
        // seed_rotation_interval / leader_rotation_interval, so each validator
        // may not be selected
        num_validators = 10;
        leader_rotation_interval = 100;
        seed_rotation_interval = 200;
        run_scheduler_test(
            num_validators,
            leader_rotation_interval,
            seed_rotation_interval,
        );

        // Test when seed_rotation_interval == leader_rotation_interval,
        // only one validator should be selected
        num_validators = 10;
        leader_rotation_interval = 2;
        seed_rotation_interval = 2;
        run_scheduler_test(
            num_validators,
            leader_rotation_interval,
            seed_rotation_interval,
        );
    }

    #[test]
    fn test_scheduler_active_window() {
        solana_logger::setup();

        let num_validators = 10;
        let num_vote_account_tokens = 1;

        // Make sure seed_rotation_interval is big enough so we select all the
        // validators as part of the schedule each time (we need to check the active window
        // is the cause of validators being truncated later)
        let leader_rotation_interval = 100;
        let seed_rotation_interval = leader_rotation_interval * num_validators;
        let active_window_length = seed_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        // Create the bank and validators
        let (genesis_block, mint_keypair) = GenesisBlock::new(
            BOOTSTRAP_LEADER_TOKENS
                + ((((num_validators + 1) / 2) * (num_validators + 1))
                    + (num_vote_account_tokens * num_validators)) as u64,
        );
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        let mut validators = vec![];
        let last_id = genesis_block.last_id();
        for i in 0..num_validators {
            let new_validator = Arc::new(Keypair::new());
            let new_pubkey = new_validator.pubkey();
            let voting_keypair = VotingKeypair::new_local(&new_validator);
            validators.push(new_pubkey);
            // Give the validator some tokens
            bank.transfer(
                (i + 1 + num_vote_account_tokens) as u64,
                &mint_keypair,
                new_pubkey,
                last_id,
            )
            .unwrap();

            // Create a vote account
            new_vote_account(
                &new_validator,
                &voting_keypair,
                &bank,
                num_vote_account_tokens as u64,
                genesis_block.last_id(),
            );

            push_vote(
                &voting_keypair,
                &bank,
                (i + 2) * active_window_length - 1,
                genesis_block.last_id(),
            );
        }

        // Generate schedule every active_window_length entries and check that
        // validators are falling out of the rotation as they fall out of the
        // active set
        let mut leader_scheduler = bank.leader_scheduler.write().unwrap();
        trace!("bootstrap_leader_id: {}", genesis_block.bootstrap_leader_id);
        for i in 0..num_validators {
            trace!("validators[{}]: {}", i, validators[i as usize]);
        }
        assert_eq!(leader_scheduler.current_epoch, 0);
        leader_scheduler.generate_schedule(1, &bank);
        assert_eq!(leader_scheduler.current_epoch, 1);
        for i in 0..=num_validators {
            info!("i === {}", i);
            leader_scheduler.generate_schedule((i + 1) * active_window_length, &bank);
            assert_eq!(leader_scheduler.current_epoch, i + 2);
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
    fn test_multiple_vote() {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_id = leader_keypair.pubkey();
        let active_window_length = 1000;
        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(10000, leader_id, 500);
        let leader_scheduler_config = LeaderSchedulerConfig::new(100, 100, active_window_length);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);

        // Bootstrap leader should be in the active set even without explicit votes
        {
            let mut leader_scheduler = bank.leader_scheduler.write().unwrap();
            let result = leader_scheduler.get_active_set(0, &bank);
            assert_eq!(result, to_hashset_owned(&vec![leader_id]));

            let result = leader_scheduler.get_active_set(active_window_length, &bank);
            assert_eq!(result, to_hashset_owned(&vec![leader_id]));

            let result = leader_scheduler.get_active_set(active_window_length + 1, &bank);
            assert!(result.is_empty());
        }

        // Check that a node that votes twice in a row will get included in the active
        // window

        let voting_keypair = VotingKeypair::new_local(&leader_keypair);
        // Create a vote account
        new_vote_account(
            &leader_keypair,
            &voting_keypair,
            &bank,
            1,
            genesis_block.last_id(),
        );

        // Vote at tick_height 1
        push_vote(&voting_keypair, &bank, 1, genesis_block.last_id());

        {
            let mut leader_scheduler = bank.leader_scheduler.write().unwrap();
            let result = leader_scheduler.get_active_set(active_window_length + 1, &bank);
            assert_eq!(result, to_hashset_owned(&vec![leader_id]));

            let result = leader_scheduler.get_active_set(active_window_length + 2, &bank);
            assert!(result.is_empty());
        }

        // Vote at tick_height 2
        push_vote(&voting_keypair, &bank, 2, genesis_block.last_id());

        {
            let mut leader_scheduler = bank.leader_scheduler.write().unwrap();
            let result = leader_scheduler.get_active_set(active_window_length + 2, &bank);
            assert_eq!(result, to_hashset_owned(&vec![leader_id]));

            let result = leader_scheduler.get_active_set(active_window_length + 3, &bank);
            assert!(result.is_empty());
        }
    }

    #[test]
    fn test_update_tick_height() {
        solana_logger::setup();

        let leader_rotation_interval = 100;
        let seed_rotation_interval = 2 * leader_rotation_interval;
        let active_window_length = 1;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        // Check that the generate_schedule() function is being called by the
        // update_tick_height() function at the correct entry heights.
        let (genesis_block, _) = GenesisBlock::new(10_000);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        let mut leader_scheduler = bank.leader_scheduler.write().unwrap();
        info!(
            "bootstrap_leader_id: {:?}",
            genesis_block.bootstrap_leader_id
        );
        assert_eq!(bank.tick_height(), 0);

        //
        // tick_height == 0 is a special case
        //
        leader_scheduler.update_tick_height(0, &bank);
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
        assert_eq!(leader_scheduler.get_leader_for_slot(2), None,);

        //
        // Check various tick heights in epoch 0, and tick 0 of epoch 1
        //
        for tick_height in &[
            1,
            leader_rotation_interval,
            leader_rotation_interval + 1,
            seed_rotation_interval - 1,
            seed_rotation_interval,
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
            assert_eq!(leader_scheduler.get_leader_for_slot(4), None);
        }

        //
        // Check various tick heights in epoch 1, and tick 0 of epoch 2
        //
        for tick_height in &[
            seed_rotation_interval + 1,
            seed_rotation_interval + leader_rotation_interval,
            seed_rotation_interval + leader_rotation_interval + 1,
            seed_rotation_interval + seed_rotation_interval - 1,
            seed_rotation_interval + seed_rotation_interval,
        ] {
            info!("Checking tick_height {}", *tick_height);
            leader_scheduler.update_tick_height(*tick_height, &bank);
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
    }

    #[test]
    fn test_constructors() {
        // Check defaults for LeaderScheduler
        let leader_scheduler_config = LeaderSchedulerConfig::default();

        let leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        assert_eq!(
            leader_scheduler.leader_rotation_interval,
            DEFAULT_TICKS_PER_SLOT
        );
        assert_eq!(
            leader_scheduler.seed_rotation_interval,
            DEFAULT_SEED_ROTATION_INTERVAL
        );

        // Check actual arguments for LeaderScheduler
        let leader_rotation_interval = 100;
        let seed_rotation_interval = 200;
        let active_window_length = 1;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        let leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        assert_eq!(
            leader_scheduler.leader_rotation_interval,
            leader_rotation_interval
        );
        assert_eq!(
            leader_scheduler.seed_rotation_interval,
            seed_rotation_interval
        );
    }

    fn run_consecutive_leader_test(num_slots_per_epoch: u64, add_validator: bool) {
        let bootstrap_leader_keypair = Arc::new(Keypair::new());
        let bootstrap_leader_id = bootstrap_leader_keypair.pubkey();
        let leader_rotation_interval = 100;
        let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
        let active_window_length = seed_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_rotation_interval,
            seed_rotation_interval,
            active_window_length,
        );

        // Create mint and bank
        let (genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(10_000, bootstrap_leader_id, BOOTSTRAP_LEADER_TOKENS);
        let bank = Bank::new_with_leader_scheduler_config(&genesis_block, &leader_scheduler_config);
        let last_id = genesis_block.last_id();
        let initial_vote_height = 1;

        // Create and add validator to the active set
        let validator_keypair = Arc::new(Keypair::new());
        let validator_id = validator_keypair.pubkey();
        if add_validator {
            bank.transfer(5, &mint_keypair, validator_id, last_id)
                .unwrap();
            // Create a vote account
            let voting_keypair = VotingKeypair::new_local(&validator_keypair);
            new_vote_account(
                &validator_keypair,
                &voting_keypair,
                &bank,
                1,
                genesis_block.last_id(),
            );

            push_vote(
                &voting_keypair,
                &bank,
                initial_vote_height,
                genesis_block.last_id(),
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

        // Create a vote account
        let voting_keypair = VotingKeypair::new_local(&bootstrap_leader_keypair);
        new_vote_account(
            &bootstrap_leader_keypair,
            &voting_keypair,
            &bank,
            vote_account_tokens as u64,
            genesis_block.last_id(),
        );

        // Add leader to the active set
        push_vote(
            &voting_keypair,
            &bank,
            initial_vote_height,
            genesis_block.last_id(),
        );

        let mut leader_scheduler = LeaderScheduler::default();
        leader_scheduler.generate_schedule(0, &bank);
        assert_eq!(leader_scheduler.current_epoch, 0);
        assert_eq!(leader_scheduler.epoch_schedule[0], [bootstrap_leader_id]);

        // Make sure the validator, not the leader is selected on the first slot of the
        // next epoch
        leader_scheduler.generate_schedule(1, &bank);
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
}
