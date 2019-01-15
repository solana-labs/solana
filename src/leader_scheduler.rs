//! The `leader_scheduler` module implements a structure and functions for tracking and
//! managing the schedule for leader rotation

use crate::bank::Bank;

use crate::entry::{create_ticks, Entry};
use crate::vote_signer_proxy::VoteSignerProxy;
use bincode::serialize;
use byteorder::{LittleEndian, ReadBytesExt};
use hashbrown::HashSet;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program::{self, Vote, VoteProgram};
use solana_sdk::vote_transaction::VoteTransaction;
use solana_vote_signer::rpc::LocalVoteSigner;
use std::io::Cursor;
use std::sync::Arc;

pub const BLOCK_TICK_COUNT: u64 = 4;
pub const DEFAULT_BOOTSTRAP_HEIGHT: u64 = BLOCK_TICK_COUNT * 256;
pub const DEFAULT_LEADER_ROTATION_INTERVAL: u64 = BLOCK_TICK_COUNT * 32;
pub const DEFAULT_SEED_ROTATION_INTERVAL: u64 = BLOCK_TICK_COUNT * 256;
pub const DEFAULT_ACTIVE_WINDOW_LENGTH: u64 = BLOCK_TICK_COUNT * 256;

pub struct LeaderSchedulerConfig {
    // The interval at which to rotate the leader, should be much less than
    // seed_rotation_interval
    pub leader_rotation_interval_option: Option<u64>,

    // The interval at which to generate the seed used for ranking the validators
    pub seed_rotation_interval_option: Option<u64>,

    // The last height at which the bootstrap_leader will be in power before
    // the leader rotation process begins to pick future leaders
    pub bootstrap_height_option: Option<u64>,

    // The length of the acceptable window for determining live validators
    pub active_window_length_option: Option<u64>,
}

// Used to toggle leader rotation in fullnode so that tests that don't
// need leader rotation don't break
impl LeaderSchedulerConfig {
    pub fn new(
        bootstrap_height_option: Option<u64>,
        leader_rotation_interval_option: Option<u64>,
        seed_rotation_interval_option: Option<u64>,
        active_window_length_option: Option<u64>,
    ) -> Self {
        LeaderSchedulerConfig {
            bootstrap_height_option,
            leader_rotation_interval_option,
            seed_rotation_interval_option,
            active_window_length_option,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LeaderScheduler {
    // Set to true if we want the default implementation of the LeaderScheduler,
    // where ony the bootstrap leader is used
    pub use_only_bootstrap_leader: bool,

    // The interval at which to rotate the leader, should be much less than
    // seed_rotation_interval
    pub leader_rotation_interval: u64,

    // The interval at which to generate the seed used for ranking the validators
    pub seed_rotation_interval: u64,

    // The first leader who will bootstrap the network
    pub bootstrap_leader: Pubkey,

    // The last height at which the bootstrap_leader will be in power before
    // the leader rotation process begins to pick future leaders
    pub bootstrap_height: u64,

    // The last height at which the seed + schedule was generated
    pub last_seed_height: Option<u64>,

    // The length of time in ticks for which a vote qualifies a candidate for leader
    // selection
    pub active_window_length: u64,

    // Round-robin ordering for the validators
    leader_schedule: Vec<Pubkey>,

    // The seed used to determine the round robin order of leaders
    seed: u64,
}

// The LeaderScheduler implements a schedule for leaders as follows:
//
// 1) During the bootstrapping period of bootstrap_height PoH counts, the
// leader is hard-coded to the bootstrap_leader that is read from the genesis block.
//
// 2) After the first seed is generated, this signals the beginning of actual leader rotation.
// From this point on, every seed_rotation_interval PoH counts we generate the seed based
// on the PoH height, and use it to do a weighted sample from the set
// of validators based on current stake weight. This gets you the bootstrap leader A for
// the next leader_rotation_interval PoH counts. On the same PoH count we generate the seed,
// we also order the validators based on their current stake weight, and starting
// from leader A, we then pick the next leader sequentially every leader_rotation_interval
// PoH counts based on this fixed ordering, so the next
// seed_rotation_interval / leader_rotation_interval leaders are determined.
//
// 3) When we we hit the next seed rotation PoH height, step 2) is executed again to
// calculate the leader schedule for the upcoming seed_rotation_interval PoH counts.
impl LeaderScheduler {
    pub fn from_bootstrap_leader(bootstrap_leader: Pubkey) -> Self {
        let config = LeaderSchedulerConfig::new(None, None, None, None);
        let mut leader_scheduler = LeaderScheduler::new(&config);
        leader_scheduler.use_only_bootstrap_leader = true;
        leader_scheduler.bootstrap_leader = bootstrap_leader;
        leader_scheduler
    }

    pub fn new(config: &LeaderSchedulerConfig) -> Self {
        let mut bootstrap_height = DEFAULT_BOOTSTRAP_HEIGHT;
        if let Some(input) = config.bootstrap_height_option {
            bootstrap_height = input;
        }

        let mut leader_rotation_interval = DEFAULT_LEADER_ROTATION_INTERVAL;
        if let Some(input) = config.leader_rotation_interval_option {
            leader_rotation_interval = input;
        }

        let mut seed_rotation_interval = DEFAULT_SEED_ROTATION_INTERVAL;
        if let Some(input) = config.seed_rotation_interval_option {
            seed_rotation_interval = input;
        }

        let mut active_window_length = DEFAULT_ACTIVE_WINDOW_LENGTH;
        if let Some(input) = config.active_window_length_option {
            active_window_length = input;
        }

        // Enforced invariants
        assert!(seed_rotation_interval >= leader_rotation_interval);
        assert!(bootstrap_height > 0);
        assert!(seed_rotation_interval % leader_rotation_interval == 0);

        LeaderScheduler {
            use_only_bootstrap_leader: false,
            leader_rotation_interval,
            seed_rotation_interval,
            leader_schedule: Vec::new(),
            last_seed_height: None,
            bootstrap_leader: Pubkey::default(),
            bootstrap_height,
            active_window_length,
            seed: 0,
        }
    }

    // Returns true if the given height is the first tick of a slot
    pub fn is_first_slot_tick(&self, height: u64) -> bool {
        if self.use_only_bootstrap_leader {
            return false;
        }

        if height < self.bootstrap_height {
            return false;
        }

        (height - self.bootstrap_height) % self.leader_rotation_interval == 1
    }

    // Returns the number of ticks until the last tick the current slot
    pub fn num_ticks_left_in_slot(&self, height: u64) -> Option<u64> {
        if self.use_only_bootstrap_leader {
            return None;
        }

        if height <= self.bootstrap_height {
            Some(self.bootstrap_height - height)
        } else if (height - self.bootstrap_height) % self.leader_rotation_interval == 0 {
            Some(0)
        } else {
            Some(
                self.leader_rotation_interval
                    - ((height - self.bootstrap_height) % self.leader_rotation_interval),
            )
        }
    }

    // Let Leader X be the leader at the input tick height. This function returns the
    // the PoH height at which Leader X's slot ends.
    pub fn max_height_for_leader(&self, height: u64) -> Option<u64> {
        if self.use_only_bootstrap_leader || self.get_scheduled_leader(height).is_none() {
            return None;
        }

        let result = {
            if height <= self.bootstrap_height || self.leader_schedule.len() > 1 {
                // Two cases to consider:
                //
                // 1) If height is less than the bootstrap height, then the current leader's
                // slot ends when PoH height = bootstrap_height
                //
                // 2) Otherwise, if height >= bootstrap height, then we have generated a schedule.
                // If this leader is not the only one in the schedule, then they will
                // only be leader until the end of this slot (someone else is then guaranteed
                // to take over)
                //
                // Both above cases are calculated by the function:
                // num_ticks_left_in_slot() + height
                self.num_ticks_left_in_slot(height).expect(
                    "Should return some value when not using default implementation
                     of LeaderScheduler",
                ) + height
            } else {
                // If the height is greater than bootstrap_height and this leader is
                // the only leader in the schedule, then that leader will be in power
                // for every slot until the next epoch, which is seed_rotation_interval
                // PoH counts from the beginning of the last epoch.
                self.last_seed_height.expect(
                    "If height >= bootstrap height, then we expect
                    a seed has been generated",
                ) + self.seed_rotation_interval
            }
        };

        Some(result)
    }

    pub fn reset(&mut self) {
        self.last_seed_height = None;
    }

    pub fn update_height(&mut self, height: u64, bank: &Bank) {
        if self.use_only_bootstrap_leader {
            return;
        }

        if height < self.bootstrap_height {
            return;
        }

        if let Some(last_seed_height) = self.last_seed_height {
            if height <= last_seed_height {
                return;
            }
        }

        if (height - self.bootstrap_height) % self.seed_rotation_interval == 0 {
            self.generate_schedule(height, bank);
        }
    }

    // Uses the schedule generated by the last call to generate_schedule() to return the
    // leader for a given PoH height in round-robin fashion
    pub fn get_scheduled_leader(&self, height: u64) -> Option<(Pubkey, u64)> {
        if self.use_only_bootstrap_leader {
            return Some((self.bootstrap_leader, 0));
        }

        // This covers cases where the schedule isn't yet generated.
        if self.last_seed_height == None {
            if height <= self.bootstrap_height {
                return Some((self.bootstrap_leader, 0));
            } else {
                // If there's been no schedule generated yet before we reach the end of the
                // bootstrapping period, then the leader is unknown
                return None;
            }
        }

        // If we have a schedule, then just check that we are within the bounds of that
        // schedule (last_seed_height, last_seed_height + seed_rotation_interval].
        // Leaders outside of this bound are undefined.
        let last_seed_height = self.last_seed_height.unwrap();

        if height > last_seed_height + self.seed_rotation_interval || height <= last_seed_height {
            return None;
        }

        // Find index into the leader_schedule that this PoH height maps to. Note by the time
        // we reach here, last_seed_height is not None, which implies:
        //
        // last_seed_height >= self.bootstrap_height
        //
        // Also we know from the recent range check that height > last_seed_height, so
        // height - self.bootstrap_height >= 1, so the below logic is safe.
        let leader_slot = (height - self.bootstrap_height - 1) / self.leader_rotation_interval + 1;
        let index = (height - last_seed_height - 1) / self.leader_rotation_interval;
        let validator_index = index as usize % self.leader_schedule.len();
        Some((self.leader_schedule[validator_index], leader_slot))
    }

    pub fn get_leader_for_slot(&self, slot_height: u64) -> Option<Pubkey> {
        let tick_height = self.slot_height_to_first_tick_height(slot_height);
        self.get_scheduled_leader(tick_height).map(|(id, _)| id)
    }

    #[cfg(test)]
    pub fn set_leader_schedule(&mut self, schedule: Vec<Pubkey>) {
        self.leader_schedule = schedule;
    }

    // Maps the nth slot (where n == slot_height) to the tick height of
    // the first tick for that slot
    fn slot_height_to_first_tick_height(&self, slot_height: u64) -> u64 {
        if slot_height == 0 {
            0
        } else {
            (slot_height - 1) * self.leader_rotation_interval + self.bootstrap_height + 1
        }
    }

    // TODO: We use a HashSet for now because a single validator could potentially register
    // multiple vote account. Once that is no longer possible (see the TODO in vote_program.rs,
    // process_transaction(), case VoteInstruction::RegisterAccount), we can use a vector.
    fn get_active_set(&mut self, height: u64, bank: &Bank) -> HashSet<Pubkey> {
        let upper_bound = height;
        let lower_bound = height.saturating_sub(self.active_window_length);

        {
            let accounts = bank.accounts.accounts_db.read().unwrap();

            // TODO: iterate through checkpoints, too
            accounts
                .accounts
                .values()
                .filter_map(|account| {
                    if vote_program::check_id(&account.owner) {
                        if let Ok(vote_state) = VoteProgram::deserialize(&account.userdata) {
                            return vote_state
                                .votes
                                .back()
                                .filter(|vote| {
                                    vote.tick_height > lower_bound
                                        && vote.tick_height <= upper_bound
                                })
                                .map(|_| vote_state.node_id);
                        }
                    }

                    None
                })
                .collect()
        }
    }

    // Called every seed_rotation_interval entries, generates the leader schedule
    // for the range of entries: (height, height + seed_rotation_interval]
    fn generate_schedule(&mut self, height: u64, bank: &Bank) {
        assert!(height >= self.bootstrap_height);
        assert!((height - self.bootstrap_height) % self.seed_rotation_interval == 0);
        let seed = Self::calculate_seed(height);
        self.seed = seed;
        let active_set = self.get_active_set(height, &bank);
        let ranked_active_set = Self::rank_active_set(bank, active_set.iter());

        // Handle case where there are no active validators with
        // non-zero stake. In this case, use the bootstrap leader for
        // the upcoming rounds
        if ranked_active_set.is_empty() {
            self.last_seed_height = Some(height);
            self.leader_schedule = vec![self.bootstrap_leader];
            self.last_seed_height = Some(height);
            return;
        }

        let (mut validator_rankings, total_stake) = ranked_active_set.iter().fold(
            (Vec::with_capacity(ranked_active_set.len()), 0),
            |(mut ids, total_stake), (pk, stake)| {
                ids.push(**pk);
                (ids, total_stake + stake)
            },
        );

        // Choose the validator that will be the first to be the leader in this new
        // schedule
        let ordered_account_stake = ranked_active_set.into_iter().map(|(_, stake)| stake);
        let start_index = Self::choose_account(ordered_account_stake, self.seed, total_stake);
        validator_rankings.rotate_left(start_index);

        // There are only seed_rotation_interval / self.leader_rotation_interval slots, so
        // we only need to keep at most that many validators in the schedule
        let slots_per_epoch = self.seed_rotation_interval / self.leader_rotation_interval;

        // If possible, try to avoid having the same leader twice in a row, but
        // if there's only one leader to choose from, then we have no other choice
        if validator_rankings.len() > 1 {
            let (old_epoch_last_leader, _) = self
                .get_scheduled_leader(height - 1)
                .expect("Previous leader schedule should still exist");
            let new_epoch_start_leader = validator_rankings[0];

            if old_epoch_last_leader == new_epoch_start_leader {
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

        self.leader_schedule = validator_rankings;
        self.last_seed_height = Some(height);
    }

    fn rank_active_set<'a, I>(bank: &Bank, active: I) -> Vec<(&'a Pubkey, u64)>
    where
        I: Iterator<Item = &'a Pubkey>,
    {
        let mut active_accounts: Vec<(&'a Pubkey, u64)> = active
            .filter_map(|pk| {
                let stake = bank.get_stake(pk);
                if stake > 0 {
                    Some((pk, stake as u64))
                } else {
                    None
                }
            })
            .collect();

        active_accounts.sort_by(
            |(pk1, t1), (pk2, t2)| {
                if t1 == t2 {
                    pk1.cmp(&pk2)
                } else {
                    t1.cmp(&t2)
                }
            },
        );
        active_accounts
    }

    fn calculate_seed(height: u64) -> u64 {
        let hash = hash(&serialize(&height).unwrap());
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
}

impl Default for LeaderScheduler {
    // Create a dummy leader scheduler
    fn default() -> Self {
        let id = Pubkey::default();
        Self::from_bootstrap_leader(id)
    }
}

// Create two entries so that the node with keypair == active_keypair
// is in the active set for leader selection:
// 1) Give the node a nonzero number of tokens,
// 2) A vote from the validator
pub fn make_active_set_entries(
    active_keypair: &Arc<Keypair>,
    token_source: &Keypair,
    last_entry_id: &Hash,
    last_tick_id: &Hash,
    num_ending_ticks: usize,
) -> (Vec<Entry>, VoteSignerProxy) {
    // 1) Create transfer token entry
    let transfer_tx =
        Transaction::system_new(&token_source, active_keypair.pubkey(), 3, *last_tick_id);
    let transfer_entry = Entry::new(last_entry_id, 0, 1, vec![transfer_tx]);
    let mut last_entry_id = transfer_entry.id;

    // 2) Create and register the vote account
    let vote_signer = VoteSignerProxy::new(active_keypair, Box::new(LocalVoteSigner::default()));
    let vote_account_id: Pubkey = vote_signer.vote_account;

    let new_vote_account_tx =
        Transaction::vote_account_new(active_keypair, vote_account_id, *last_tick_id, 1, 1);
    let new_vote_account_entry = Entry::new(&last_entry_id, 0, 1, vec![new_vote_account_tx]);
    last_entry_id = new_vote_account_entry.id;

    // 3) Create vote entry
    let vote = Vote { tick_height: 1 };
    let tx = Transaction::vote_new(&vote_account_id, vote, *last_tick_id, 0);
    let msg = tx.get_sign_data();
    let sig = Signature::new(&active_keypair.sign(&msg).as_ref());
    let vote_tx = Transaction {
        signatures: vec![sig],
        account_keys: tx.account_keys,
        last_id: tx.last_id,
        fee: tx.fee,
        program_ids: tx.program_ids,
        instructions: tx.instructions,
    };
    let vote_entry = Entry::new(&last_entry_id, 0, 1, vec![vote_tx]);
    last_entry_id = vote_entry.id;

    // 4) Create the ending empty ticks
    let mut txs = vec![transfer_entry, new_vote_account_entry, vote_entry];
    let empty_ticks = create_ticks(num_ending_ticks, last_entry_id);
    txs.extend(empty_ticks);
    (txs, vote_signer)
}

#[cfg(test)]
mod tests {
    use crate::bank::Bank;
    use crate::leader_scheduler::{
        LeaderScheduler, LeaderSchedulerConfig, DEFAULT_BOOTSTRAP_HEIGHT,
        DEFAULT_LEADER_ROTATION_INTERVAL, DEFAULT_SEED_ROTATION_INTERVAL,
    };
    use crate::mint::Mint;
    use crate::vote_signer_proxy::VoteSignerProxy;
    use hashbrown::HashSet;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_vote_signer::rpc::LocalVoteSigner;
    use std::hash::Hash as StdHash;
    use std::iter::FromIterator;
    use std::sync::Arc;

    fn to_hashset_owned<T>(slice: &[T]) -> HashSet<T>
    where
        T: Eq + StdHash + Clone,
    {
        HashSet::from_iter(slice.iter().cloned())
    }

    fn push_vote(vote_signer: &VoteSignerProxy, bank: &Bank, height: u64, last_id: Hash) {
        let new_vote_tx = vote_signer.new_signed_vote_transaction(&last_id, height);

        bank.process_transaction(&new_vote_tx).unwrap();
    }

    fn run_scheduler_test(
        num_validators: usize,
        bootstrap_height: u64,
        leader_rotation_interval: u64,
        seed_rotation_interval: u64,
    ) {
        // Allow the validators to be in the active window for the entire test
        let active_window_length = seed_rotation_interval + bootstrap_height;

        // Set up the LeaderScheduler struct
        let bootstrap_leader_id = Keypair::new().pubkey();
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = bootstrap_leader_id;

        // Create the bank and validators, which are inserted in order of account balance
        let num_vote_account_tokens = 1;
        let mint = Mint::new(
            (((num_validators + 1) / 2) * (num_validators + 1)
                + num_vote_account_tokens * num_validators) as u64,
        );
        let bank = Bank::new(&mint);
        let mut validators = vec![];
        let last_id = mint
            .create_entries()
            .last()
            .expect("Mint should not create empty genesis entries")
            .id;
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            let vote_signer = VoteSignerProxy::new(
                &Arc::new(new_validator),
                Box::new(LocalVoteSigner::default()),
            );
            validators.push(new_pubkey);
            // Give the validator some tokens
            bank.transfer(
                (i + 1 + num_vote_account_tokens) as u64,
                &mint.keypair(),
                new_pubkey,
                last_id,
            )
            .unwrap();

            // Create a vote account
            vote_signer
                .new_vote_account(&bank, num_vote_account_tokens as u64, mint.last_id())
                .unwrap();
            // Vote to make the validator part of the active set for the entire test
            // (we made the active_window_length large enough at the beginning of the test)
            push_vote(&vote_signer, &bank, 1, mint.last_id());
        }

        // The scheduled leader during the bootstrapping period (assuming a seed + schedule
        // haven't been generated, otherwise that schedule takes precendent) should always
        // be the bootstrap leader
        assert_eq!(
            leader_scheduler.get_scheduled_leader(0),
            Some((bootstrap_leader_id, 0))
        );
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height - 1),
            Some((bootstrap_leader_id, 0))
        );
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height),
            Some((bootstrap_leader_id, 0))
        );
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height + 1),
            None
        );

        // Generate the schedule at the end of the bootstrapping period, should be the
        // same leader for the next leader_rotation_interval entries
        leader_scheduler.generate_schedule(bootstrap_height, &bank);

        // The leader outside of the newly generated schedule window:
        // (bootstrap_height, bootstrap_height + seed_rotation_interval]
        // should be undefined
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height),
            None,
        );

        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height + seed_rotation_interval + 1),
            None,
        );

        // For the next seed_rotation_interval entries, call get_scheduled_leader every
        // leader_rotation_interval entries, and the next leader should be the next validator
        // in order of stake

        // Note: seed_rotation_interval must be divisible by leader_rotation_interval, enforced
        // by the LeaderScheduler constructor
        let num_rounds = seed_rotation_interval / leader_rotation_interval;
        let mut start_leader_index = None;
        for i in 0..num_rounds {
            let begin_height = bootstrap_height + i * leader_rotation_interval + 1;
            let (current_leader, slot) = leader_scheduler
                .get_scheduled_leader(begin_height)
                .expect("Expected a leader from scheduler");

            // Note: The "validators" vector is already sorted by stake, so the expected order
            // for the leader schedule can be derived by just iterating through the vector
            // in order. The only excpetion is for the bootstrap leader in the schedule, we need to
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
            assert_eq!(slot, i + 1);
            // Check that the same leader is in power for the next leader_rotation_interval - 1 entries
            assert_eq!(
                leader_scheduler.get_scheduled_leader(begin_height + leader_rotation_interval - 1),
                Some((current_leader, slot))
            );
        }
    }

    #[test]
    fn test_active_set() {
        let leader_id = Keypair::new().pubkey();
        let active_window_length = 1000;
        let mint = Mint::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new(&mint);

        let leader_scheduler_config =
            LeaderSchedulerConfig::new(Some(100), Some(100), Some(100), Some(active_window_length));

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = leader_id;

        // Insert a bunch of votes at height "start_height"
        let start_height = 3;
        let num_old_ids = 20;
        let mut old_ids = HashSet::new();
        for _ in 0..num_old_ids {
            let new_keypair = Keypair::new();
            let pk = new_keypair.pubkey();
            old_ids.insert(pk.clone());

            // Give the account some stake
            bank.transfer(5, &mint.keypair(), pk, mint.last_id())
                .unwrap();

            // Create a vote account
            let vote_signer =
                VoteSignerProxy::new(&Arc::new(new_keypair), Box::new(LocalVoteSigner::default()));
            vote_signer
                .new_vote_account(&bank, 1 as u64, mint.last_id())
                .unwrap();

            // Push a vote for the account
            push_vote(&vote_signer, &bank, start_height, mint.last_id());
        }

        // Insert a bunch of votes at height "start_height + active_window_length"
        let num_new_ids = 10;
        let mut new_ids = HashSet::new();
        for _ in 0..num_new_ids {
            let new_keypair = Keypair::new();
            let pk = new_keypair.pubkey();
            new_ids.insert(pk);
            // Give the account some stake
            bank.transfer(5, &mint.keypair(), pk, mint.last_id())
                .unwrap();

            // Create a vote account
            let vote_signer =
                VoteSignerProxy::new(&Arc::new(new_keypair), Box::new(LocalVoteSigner::default()));
            vote_signer
                .new_vote_account(&bank, 1 as u64, mint.last_id())
                .unwrap();

            push_vote(
                &vote_signer,
                &bank,
                start_height + active_window_length,
                mint.last_id(),
            );
        }

        // Queries for the active set
        let result =
            leader_scheduler.get_active_set(active_window_length + start_height - 1, &bank);
        assert_eq!(result, old_ids);

        let result = leader_scheduler.get_active_set(active_window_length + start_height, &bank);
        assert_eq!(result, new_ids);

        let result =
            leader_scheduler.get_active_set(2 * active_window_length + start_height - 1, &bank);
        assert_eq!(result, new_ids);

        let result =
            leader_scheduler.get_active_set(2 * active_window_length + start_height, &bank);
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
        // Give mint sum(1..num_validators) tokens
        let mint = Mint::new((((num_validators + 1) / 2) * (num_validators + 1)) as u64);
        let bank = Bank::new(&mint);
        let mut validators = vec![];
        let last_id = mint
            .create_entries()
            .last()
            .expect("Mint should not create empty genesis entries")
            .id;
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            validators.push(new_validator);
            bank.transfer(
                (num_validators - i) as u64,
                &mint.keypair(),
                new_pubkey,
                last_id,
            )
            .unwrap();
        }

        let validators_pk: Vec<Pubkey> = validators.iter().map(Keypair::pubkey).collect();
        let result = LeaderScheduler::rank_active_set(&bank, validators_pk.iter());

        assert_eq!(result.len(), validators.len());

        // Expect the result to be the reverse of the list we passed into the rank_active_set()
        for (i, (pk, stake)) in result.into_iter().enumerate() {
            assert_eq!(stake, i as u64 + 1);
            assert_eq!(*pk, validators[num_validators - i - 1].pubkey());
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

        for (i, (pk, balance)) in result.into_iter().enumerate() {
            assert_eq!(balance, i as u64 + 1);
            assert_eq!(*pk, new_validators[num_validators - i - 1].pubkey());
        }

        // Break ties between validators with the same balances using public key
        let mint = Mint::new(num_validators as u64);
        let bank = Bank::new(&mint);
        let mut tied_validators_pk = vec![];
        let last_id = mint
            .create_entries()
            .last()
            .expect("Mint should not create empty genesis entries")
            .id;

        for _ in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            tied_validators_pk.push(new_pubkey);
            bank.transfer(1, &mint.keypair(), new_pubkey, last_id)
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
    fn test_scheduler() {
        // Test when the number of validators equals
        // seed_rotation_interval / leader_rotation_interval, so each validator
        // is selected once
        let mut num_validators = 100;
        let mut bootstrap_height = 500;
        let mut leader_rotation_interval = 100;
        let mut seed_rotation_interval = leader_rotation_interval * num_validators;
        run_scheduler_test(
            num_validators,
            bootstrap_height,
            leader_rotation_interval as u64,
            seed_rotation_interval as u64,
        );

        // Test when there are fewer validators than
        // seed_rotation_interval / leader_rotation_interval, so each validator
        // is selected multiple times
        num_validators = 3;
        bootstrap_height = 500;
        leader_rotation_interval = 100;
        seed_rotation_interval = 1000;
        run_scheduler_test(
            num_validators,
            bootstrap_height,
            leader_rotation_interval as u64,
            seed_rotation_interval as u64,
        );

        // Test when there are fewer number of validators than
        // seed_rotation_interval / leader_rotation_interval, so each validator
        // may not be selected
        num_validators = 10;
        bootstrap_height = 500;
        leader_rotation_interval = 100;
        seed_rotation_interval = 200;
        run_scheduler_test(
            num_validators,
            bootstrap_height,
            leader_rotation_interval as u64,
            seed_rotation_interval as u64,
        );

        // Test when seed_rotation_interval == leader_rotation_interval,
        // only one validator should be selected
        num_validators = 10;
        bootstrap_height = 1;
        leader_rotation_interval = 1;
        seed_rotation_interval = 1;
        run_scheduler_test(
            num_validators,
            bootstrap_height,
            leader_rotation_interval as u64,
            seed_rotation_interval as u64,
        );
    }

    #[test]
    fn test_scheduler_active_window() {
        let num_validators = 10;
        let num_vote_account_tokens = 1;
        // Set up the LeaderScheduler struct
        let bootstrap_leader_id = Keypair::new().pubkey();
        let bootstrap_height = 500;
        let leader_rotation_interval = 100;
        // Make sure seed_rotation_interval is big enough so we select all the
        // validators as part of the schedule each time (we need to check the active window
        // is the cause of validators being truncated later)
        let seed_rotation_interval = leader_rotation_interval * num_validators;
        let active_window_length = seed_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = bootstrap_leader_id;

        // Create the bank and validators
        let mint = Mint::new(
            ((((num_validators + 1) / 2) * (num_validators + 1))
                + (num_vote_account_tokens * num_validators)) as u64,
        );
        let bank = Bank::new(&mint);
        let mut validators = vec![];
        let last_id = mint
            .create_entries()
            .last()
            .expect("Mint should not create empty genesis entries")
            .id;
        for i in 0..num_validators {
            let new_validator = Keypair::new();
            let new_pubkey = new_validator.pubkey();
            let vote_signer = VoteSignerProxy::new(
                &Arc::new(new_validator),
                Box::new(LocalVoteSigner::default()),
            );
            validators.push(new_pubkey);
            // Give the validator some tokens
            bank.transfer(
                (i + 1 + num_vote_account_tokens) as u64,
                &mint.keypair(),
                new_pubkey,
                last_id,
            )
            .unwrap();

            // Create a vote account
            vote_signer
                .new_vote_account(&bank, num_vote_account_tokens as u64, mint.last_id())
                .unwrap();

            // Vote at height i * active_window_length for validator i
            push_vote(
                &vote_signer,
                &bank,
                i * active_window_length + bootstrap_height,
                mint.last_id(),
            );
        }

        // Generate schedule every active_window_length entries and check that
        // validators are falling out of the rotation as they fall out of the
        // active set
        for i in 0..=num_validators {
            leader_scheduler.generate_schedule(i * active_window_length + bootstrap_height, &bank);
            let result = &leader_scheduler.leader_schedule;
            let expected = if i == num_validators {
                bootstrap_leader_id
            } else {
                validators[i as usize]
            };

            assert_eq!(vec![expected], *result);
        }
    }

    #[test]
    fn test_multiple_vote() {
        let leader_keypair = Keypair::new();
        let leader_id = leader_keypair.pubkey();
        let active_window_length = 1000;
        let mint = Mint::new_with_leader(10000, leader_id, 500);
        let bank = Bank::new(&mint);

        let leader_scheduler_config =
            LeaderSchedulerConfig::new(Some(100), Some(100), Some(100), Some(active_window_length));

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = leader_id;

        // Check that a node that votes twice in a row will get included in the active
        // window
        let initial_vote_height = 1;

        let vote_signer = VoteSignerProxy::new(
            &Arc::new(leader_keypair),
            Box::new(LocalVoteSigner::default()),
        );
        // Create a vote account
        vote_signer
            .new_vote_account(&bank, 1 as u64, mint.last_id())
            .unwrap();

        // Vote twice
        push_vote(&vote_signer, &bank, initial_vote_height, mint.last_id());
        push_vote(&vote_signer, &bank, initial_vote_height + 1, mint.last_id());

        let result =
            leader_scheduler.get_active_set(initial_vote_height + active_window_length, &bank);
        assert_eq!(result, to_hashset_owned(&vec![leader_id]));
        let result =
            leader_scheduler.get_active_set(initial_vote_height + active_window_length + 1, &bank);
        assert!(result.is_empty());
    }

    #[test]
    fn test_update_height() {
        let bootstrap_leader_id = Keypair::new().pubkey();
        let bootstrap_height = 500;
        let leader_rotation_interval = 100;
        // Make sure seed_rotation_interval is big enough so we select all the
        // validators as part of the schedule each time (we need to check the active window
        // is the cause of validators being truncated later)
        let seed_rotation_interval = leader_rotation_interval;
        let active_window_length = 1;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = bootstrap_leader_id;

        // Check that the generate_schedule() function is being called by the
        // update_height() function at the correct entry heights.
        let bank = Bank::default();
        leader_scheduler.update_height(bootstrap_height - 1, &bank);
        assert_eq!(leader_scheduler.last_seed_height, None);
        leader_scheduler.update_height(bootstrap_height, &bank);
        assert_eq!(leader_scheduler.last_seed_height, Some(bootstrap_height));
        leader_scheduler.update_height(bootstrap_height + seed_rotation_interval - 1, &bank);
        assert_eq!(leader_scheduler.last_seed_height, Some(bootstrap_height));
        leader_scheduler.update_height(bootstrap_height + seed_rotation_interval, &bank);
        assert_eq!(
            leader_scheduler.last_seed_height,
            Some(bootstrap_height + seed_rotation_interval)
        );
    }

    #[test]
    fn test_constructors() {
        let bootstrap_leader_id = Keypair::new().pubkey();

        // Check defaults for LeaderScheduler
        let leader_scheduler_config = LeaderSchedulerConfig::new(None, None, None, None);

        let leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        assert_eq!(leader_scheduler.bootstrap_leader, Pubkey::default());

        assert_eq!(leader_scheduler.bootstrap_height, DEFAULT_BOOTSTRAP_HEIGHT);

        assert_eq!(
            leader_scheduler.leader_rotation_interval,
            DEFAULT_LEADER_ROTATION_INTERVAL
        );
        assert_eq!(
            leader_scheduler.seed_rotation_interval,
            DEFAULT_SEED_ROTATION_INTERVAL
        );

        // Check actual arguments for LeaderScheduler
        let bootstrap_height = 500;
        let leader_rotation_interval = 100;
        let seed_rotation_interval = 200;
        let active_window_length = 1;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = bootstrap_leader_id;

        assert_eq!(leader_scheduler.bootstrap_height, bootstrap_height);

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
        let bootstrap_leader_keypair = Keypair::new();
        let bootstrap_leader_id = bootstrap_leader_keypair.pubkey();
        let bootstrap_height = 500;
        let leader_rotation_interval = 100;
        let seed_rotation_interval = num_slots_per_epoch * leader_rotation_interval;
        let active_window_length = bootstrap_height + seed_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = bootstrap_leader_id;

        // Create mint and bank
        let mint = Mint::new_with_leader(10000, bootstrap_leader_id, 0);
        let bank = Bank::new(&mint);
        let last_id = mint
            .create_entries()
            .last()
            .expect("Mint should not create empty genesis entries")
            .id;
        let initial_vote_height = 1;

        // Create and add validator to the active set
        let validator_keypair = Keypair::new();
        let validator_id = validator_keypair.pubkey();
        if add_validator {
            bank.transfer(5, &mint.keypair(), validator_id, last_id)
                .unwrap();
            // Create a vote account
            let vote_signer = VoteSignerProxy::new(
                &Arc::new(validator_keypair),
                Box::new(LocalVoteSigner::default()),
            );
            vote_signer
                .new_vote_account(&bank, 1 as u64, mint.last_id())
                .unwrap();

            push_vote(&vote_signer, &bank, initial_vote_height, mint.last_id());
        }

        // Make sure the bootstrap leader, not the validator, is picked again on next slot
        // Depending on the seed, we make the leader stake either 2, or 3. Because the
        // validator stake is always 1, then the rankings will always be
        // [(validator, 1), (leader, leader_stake)]. Thus we just need to make sure that
        // seed % (leader_stake + 1) > 0 to make sure that the leader is picked again.
        let seed = LeaderScheduler::calculate_seed(bootstrap_height);
        let leader_stake = {
            if seed % 3 == 0 {
                3
            } else {
                2
            }
        };

        let vote_account_tokens = 1;
        bank.transfer(
            leader_stake + vote_account_tokens,
            &mint.keypair(),
            bootstrap_leader_id,
            last_id,
        )
        .unwrap();

        // Create a vote account
        let vote_signer = VoteSignerProxy::new(
            &Arc::new(bootstrap_leader_keypair),
            Box::new(LocalVoteSigner::default()),
        );
        vote_signer
            .new_vote_account(&bank, vote_account_tokens as u64, mint.last_id())
            .unwrap();

        // Add leader to the active set
        push_vote(&vote_signer, &bank, initial_vote_height, mint.last_id());

        leader_scheduler.generate_schedule(bootstrap_height, &bank);

        // Make sure the validator, not the leader is selected on the first slot of the
        // next epoch
        if add_validator {
            assert!(leader_scheduler.leader_schedule[0] == validator_id);
        } else {
            assert!(leader_scheduler.leader_schedule[0] == bootstrap_leader_id);
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
    fn test_max_height_for_leader() {
        let bootstrap_leader_keypair = Keypair::new();
        let bootstrap_leader_id = bootstrap_leader_keypair.pubkey();
        let bootstrap_height = 500;
        let leader_rotation_interval = 100;
        let seed_rotation_interval = 2 * leader_rotation_interval;
        let active_window_length = bootstrap_height + seed_rotation_interval;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);
        leader_scheduler.bootstrap_leader = bootstrap_leader_id;

        // Create mint and bank
        let mint = Mint::new_with_leader(10000, bootstrap_leader_id, 500);
        let bank = Bank::new(&mint);
        let last_id = mint
            .create_entries()
            .last()
            .expect("Mint should not create empty genesis entries")
            .id;
        let initial_vote_height = 1;

        // No schedule generated yet, so for all heights <= bootstrap height, the
        // max height will be bootstrap height
        assert_eq!(
            leader_scheduler.max_height_for_leader(0),
            Some(bootstrap_height)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height - 1),
            Some(bootstrap_height)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height),
            Some(bootstrap_height)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + 1),
            None
        );

        // Test when the active set == 1 node

        // Generate schedule where the bootstrap leader will be the only
        // choice because the active set is empty. Thus if the schedule
        // was generated on PoH height bootstrap_height + n * seed_rotation_interval,
        // then the same leader will be in power until PoH height
        // bootstrap_height + (n + 1) * seed_rotation_interval
        leader_scheduler.generate_schedule(bootstrap_height, &bank);
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height),
            None
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height - 1),
            None
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + 1),
            Some(bootstrap_height + seed_rotation_interval)
        );
        leader_scheduler.generate_schedule(bootstrap_height + seed_rotation_interval, &bank);
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval + 1),
            Some(bootstrap_height + 2 * seed_rotation_interval)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval),
            None
        );

        leader_scheduler.reset();

        // Now test when the active set > 1 node

        // Create and add validator to the active set
        let validator_keypair = Keypair::new();
        let validator_id = validator_keypair.pubkey();

        // Create a vote account for the validator
        bank.transfer(5, &mint.keypair(), validator_id, last_id)
            .unwrap();
        let vote_signer = VoteSignerProxy::new(
            &Arc::new(validator_keypair),
            Box::new(LocalVoteSigner::default()),
        );
        vote_signer
            .new_vote_account(&bank, 1 as u64, mint.last_id())
            .unwrap();
        push_vote(&vote_signer, &bank, initial_vote_height, mint.last_id());

        // Create a vote account for the leader
        bank.transfer(5, &mint.keypair(), bootstrap_leader_id, last_id)
            .unwrap();
        let vote_signer = VoteSignerProxy::new(
            &Arc::new(bootstrap_leader_keypair),
            Box::new(LocalVoteSigner::default()),
        );
        vote_signer
            .new_vote_account(&bank, 1 as u64, mint.last_id())
            .unwrap();

        // Add leader to the active set
        push_vote(&vote_signer, &bank, initial_vote_height, mint.last_id());

        // Generate the schedule
        leader_scheduler.generate_schedule(bootstrap_height, &bank);

        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height),
            None
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + 1),
            Some(bootstrap_height + leader_rotation_interval)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + leader_rotation_interval - 1),
            Some(bootstrap_height + leader_rotation_interval)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + leader_rotation_interval),
            Some(bootstrap_height + leader_rotation_interval)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + leader_rotation_interval + 1),
            Some(bootstrap_height + 2 * leader_rotation_interval)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval - 1),
            Some(bootstrap_height + seed_rotation_interval),
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval),
            Some(bootstrap_height + seed_rotation_interval),
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval + 1),
            None,
        );

        // Generate another schedule
        leader_scheduler.generate_schedule(bootstrap_height + seed_rotation_interval, &bank);

        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval),
            None
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + seed_rotation_interval + 1),
            Some(bootstrap_height + seed_rotation_interval + leader_rotation_interval)
        );
        assert_eq!(
            leader_scheduler.max_height_for_leader(bootstrap_height + 2 * seed_rotation_interval),
            Some(bootstrap_height + 2 * seed_rotation_interval)
        );
        assert_eq!(
            leader_scheduler
                .max_height_for_leader(bootstrap_height + 2 * seed_rotation_interval + 1),
            None
        );
    }
}
