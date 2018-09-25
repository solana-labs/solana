//! The `leader_scheduler` module implements a structure and functions for tracking and
//! managing the schedule for leader rotation

use bank::Bank;
use bincode::serialize;
use byteorder::{LittleEndian, ReadBytesExt};
use hash::hash;
use solana_program_interface::pubkey::Pubkey;
use std::collections::HashMap;
use std::io::Cursor;

pub struct ActiveValidators {
    // Map from validator id to the last PoH height at which they voted,
    pub active_validators: HashMap<Pubkey, u64>,
    pub active_window_length: u64,
}

impl ActiveValidators {
    pub fn new(active_window_length_option: Option<u64>) -> Self {
        let mut active_window_length = 1000;
        if let Some(input) = active_window_length_option {
            active_window_length = input;
        }

        ActiveValidators {
            active_validators: HashMap::new(),
            active_window_length,
        }
    }

    // Finds all the active voters who have voted in the range
    // (height - active_window_length, height], and removes
    // anybody who hasn't voted in that range from the map
    pub fn get_active_set(&mut self, height: u64) -> Vec<Pubkey> {
        // Don't filter anything if height is less than the
        // size of the active window. Otherwise, calculate the acceptable
        // window and filter the active_validators

        // Note: height == 0 will only be included for all
        // height < self.active_window_length
        if height >= self.active_window_length {
            let lower_bound = height - self.active_window_length;
            self.active_validators
                .retain(|_, height| *height > lower_bound);
        }

        self.active_validators.keys().cloned().collect()
    }

    // Push a vote for a validator with id == "id" who voted at PoH height == "height"
    pub fn push_vote(&mut self, id: Pubkey, height: u64) -> () {
        let old_height = self.active_validators.entry(id).or_insert(height);
        if height > *old_height {
            *old_height = height;
        }
    }
}

pub struct LeaderSchedulerConfig {
    // The first leader who will bootstrap the network
    pub bootstrap_leader: Pubkey,

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
    #[allow(dead_code)]
    pub fn new(
        bootstrap_leader: Pubkey,
        bootstrap_height_option: Option<u64>,
        leader_rotation_interval_option: Option<u64>,
        seed_rotation_interval_option: Option<u64>,
        active_window_length_option: Option<u64>,
    ) -> Self {
        LeaderSchedulerConfig {
            bootstrap_leader,
            bootstrap_height_option,
            leader_rotation_interval_option,
            seed_rotation_interval_option,
            active_window_length_option,
        }
    }
}

pub struct LeaderScheduler {
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

    // Maintain the set of active validators
    pub active_validators: ActiveValidators,

    // Round-robin ordering for the validators
    leader_schedule: Vec<Pubkey>,

    // The last height at which the seed + schedule was generated
    last_seed_height: Option<u64>,

    // The seed used to determine the round robin order of leaders
    seed: u64,
}

// The LeaderScheduler implements a schedule for leaders as follows:
//
// 1) During the bootstrapping period of bootstrap_height PoH counts, the
// leader is hard-coded to the input bootstrap_leader.
//
// 2) After the first seed is generated, this signals the beginning of actual leader rotation.
// From this point on, every seed_rotation_interval PoH counts we generate the seed based
// on the PoH height, and use it to do a weighted sample from the set
// of validators based on current stake weight. This gets you the first leader A for
// the next leader_rotation_interval PoH counts. On the same PoH count we generate the seed,
// we also order the validators based on their current stake weight, and starting
// from leader A, we then pick the next leader sequentially every leader_rotation_interval
// PoH counts based on this fixed ordering, so the next
// seed_rotation_interval / leader_rotation_interval leaders are determined.
//
// 3) When we we hit the next seed rotation PoH height, step 2) is executed again to
// calculate the leader schedule for the upcoming seed_rotation_interval PoH counts.
impl LeaderScheduler {
    pub fn new(config: &LeaderSchedulerConfig) -> Self {
        let mut bootstrap_height = 1000;
        if let Some(input) = config.bootstrap_height_option {
            bootstrap_height = input;
        }

        let mut leader_rotation_interval = 100;
        if let Some(input) = config.leader_rotation_interval_option {
            leader_rotation_interval = input;
        }

        let mut seed_rotation_interval = 1000;
        if let Some(input) = config.seed_rotation_interval_option {
            seed_rotation_interval = input;
        }

        // Enforced invariants
        assert!(seed_rotation_interval >= leader_rotation_interval);
        assert!(bootstrap_height > 0);
        assert!(seed_rotation_interval % leader_rotation_interval == 0);

        LeaderScheduler {
            active_validators: ActiveValidators::new(config.active_window_length_option),
            leader_rotation_interval,
            seed_rotation_interval,
            leader_schedule: Vec::new(),
            last_seed_height: None,
            bootstrap_leader: config.bootstrap_leader,
            bootstrap_height,
            seed: 0,
        }
    }

    pub fn push_vote(&mut self, id: Pubkey, height: u64) {
        self.active_validators.push_vote(id, height);
    }

    fn get_active_set(&mut self, height: u64) -> Vec<Pubkey> {
        self.active_validators.get_active_set(height)
    }

    pub fn update_height(&mut self, height: u64, bank: &Bank) {
        if height < self.bootstrap_height {
            return;
        }

        if height == self.bootstrap_height
            || (height - self.bootstrap_height) % self.seed_rotation_interval == 0
        {
            self.generate_schedule(height, bank);
        }
    }

    // Uses the schedule generated by the last call to generate_schedule() to return the
    // leader for a given PoH height in round-robin fashion
    pub fn get_scheduled_leader(&self, height: u64) -> Option<Pubkey> {
        // This covers cases where the schedule isn't yet generated.
        if self.last_seed_height == None {
            if height < self.bootstrap_height {
                return Some(self.bootstrap_leader);
            } else {
                // If there's been no schedule generated yet before we reach the end of the
                // bootstrapping period, then the leader is unknown
                return None;
            }
        }

        // If we have a schedule, then just check that we are within the bounds of that
        // schedule [last_seed_height, last_seed_height + seed_rotation_interval).
        // Leaders outside of this bound are undefined.
        let last_seed_height = self.last_seed_height.unwrap();
        if height >= last_seed_height + self.seed_rotation_interval || height < last_seed_height {
            return None;
        }

        // Find index into the leader_schedule that this PoH height maps to
        let index = (height - last_seed_height) / self.leader_rotation_interval;
        let validator_index = index as usize % self.leader_schedule.len();
        Some(self.leader_schedule[validator_index])
    }

    // Called every seed_rotation_interval entries, generates the leader schedule
    // for the range of entries: [height, height + seed_rotation_interval)
    fn generate_schedule(&mut self, height: u64, bank: &Bank) {
        let seed = Self::calculate_seed(height);
        self.seed = seed;
        let active_set = self.get_active_set(height);
        let ranked_active_set = Self::rank_active_set(bank, &active_set[..]);

        // Handle case where there are no active validators with
        // non-zero stake. In this case, use the bootstrap leader for
        // the upcoming rounds
        if ranked_active_set.is_empty() {
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
        validator_rankings.rotate_left(start_index + 1);

        // There are only seed_rotation_interval / self.leader_rotation_interval slots, so
        // we only need to keep at most that many validators in the schedule
        validator_rankings
            .truncate((self.seed_rotation_interval / self.leader_rotation_interval) as usize);
        self.leader_schedule = validator_rankings;
        self.last_seed_height = Some(height);
    }

    fn get_stake(id: &Pubkey, bank: &Bank) -> i64 {
        bank.get_balance(id)
    }

    fn rank_active_set<'a>(bank: &Bank, active: &'a [Pubkey]) -> Vec<(&'a Pubkey, u64)> {
        let mut active_accounts: Vec<(&'a Pubkey, u64)> = active
            .iter()
            .filter_map(|pk| {
                let stake = Self::get_stake(pk, bank);
                if stake > 0 {
                    Some((pk, stake as u64))
                } else {
                    None
                }
            }).collect();

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

#[cfg(test)]
mod tests {
    use bank::Bank;
    use leader_scheduler::{LeaderScheduler, LeaderSchedulerConfig};
    use mint::Mint;
    use signature::{Keypair, KeypairUtil};
    use solana_program_interface::pubkey::Pubkey;
    use std::collections::HashSet;
    use std::hash::Hash;
    use std::iter::FromIterator;

    fn to_hashset<T>(slice: &[T]) -> HashSet<&T>
    where
        T: Eq + Hash,
    {
        HashSet::from_iter(slice.iter())
    }

    fn to_hashset_owned<T>(slice: &[T]) -> HashSet<T>
    where
        T: Eq + Hash + Clone,
    {
        HashSet::from_iter(slice.iter().cloned())
    }

    fn run_scheduler_test(
        num_validators: usize,
        bootstrap_height: u64,
        leader_rotation_interval: u64,
        seed_rotation_interval: u64,
    ) {
        if num_validators == 0 {
            return;
        }

        // Allow the validators to be in the active window for the entire test
        let active_window_length = seed_rotation_interval + bootstrap_height;

        // Set up the LeaderScheduler struct
        let bootstrap_leader_id = Keypair::new().pubkey();
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            bootstrap_leader_id,
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        // Create the bank and validators, which are inserted in order of account balance
        let mint = Mint::new((((num_validators + 1) / 2) * (num_validators + 1)) as i64);
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
            validators.push(new_pubkey);
            // Vote to make the validator part of the active set for the entire test
            // (we made the active_window_length large enough at the beginning of the test)
            leader_scheduler.push_vote(new_pubkey, 1);
            bank.transfer((i + 1) as i64, &mint.keypair(), new_pubkey, last_id)
                .unwrap();
        }

        // The scheduled leader during the bootstrapping period (assuming a seed + schedule
        // haven't been generated, otherwise that schedule takes precendent) should always
        // be the bootstrap leader
        assert_eq!(
            leader_scheduler.get_scheduled_leader(0),
            Some(bootstrap_leader_id)
        );
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height - 1),
            Some(bootstrap_leader_id)
        );
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height),
            None
        );

        // Generate the schedule at the end of the bootstrapping period, should be the
        // same leader for the next leader_rotation_interval entries
        leader_scheduler.generate_schedule(bootstrap_height, &bank);

        // The leader outside of the newly generated schedule window:
        // [bootstrap_height, bootstrap_height + seed_rotation_interval]
        // should be undefined
        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height - 1),
            None,
        );

        assert_eq!(
            leader_scheduler.get_scheduled_leader(bootstrap_height + seed_rotation_interval),
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
            let begin_height = bootstrap_height + i * leader_rotation_interval;
            let current_leader = leader_scheduler
                .get_scheduled_leader(begin_height)
                .expect("Expected a leader from scheduler");

            // Note: The "validators" vector is already sorted by stake, so the expected order
            // for the leader schedule can be derived by just iterating through the vector
            // in order. The only excpetion is for the first leader in the schedule, we need to
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
            // Check that the same leader is in power for the next leader_rotation_interval entries
            assert_eq!(
                leader_scheduler.get_scheduled_leader(begin_height + leader_rotation_interval - 1),
                Some(current_leader)
            );
        }
    }

    #[test]
    fn test_active_set() {
        let leader_id = Keypair::new().pubkey();
        let active_window_length = 1000;
        let leader_scheduler_config = LeaderSchedulerConfig::new(
            leader_id,
            Some(100),
            Some(100),
            Some(100),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        // Insert a bunch of votes at height "start_height"
        let start_height = 3;
        let num_old_ids = 20;
        let mut old_ids = HashSet::new();
        for _ in 0..num_old_ids {
            let pk = Keypair::new().pubkey();
            old_ids.insert(pk);
            leader_scheduler.push_vote(pk, start_height);
        }

        // Insert a bunch of votes at height "start_height + active_window_length"
        let num_new_ids = 10;
        let mut new_ids = HashSet::new();
        for _ in 0..num_new_ids {
            let pk = Keypair::new().pubkey();
            new_ids.insert(pk);
            leader_scheduler.push_vote(pk, start_height + active_window_length);
        }

        let all_ids = old_ids.union(&new_ids).collect();

        // Queries for the active set
        let result = leader_scheduler.get_active_set(active_window_length + start_height - 1);
        assert_eq!(result.len(), num_old_ids + num_new_ids);
        let result_set = to_hashset(&result);
        assert_eq!(result_set, all_ids);

        let result = leader_scheduler.get_active_set(active_window_length + start_height);
        assert_eq!(result.len(), num_new_ids);
        let result_set = to_hashset_owned(&result);
        assert_eq!(result_set, new_ids);

        let result = leader_scheduler.get_active_set(2 * active_window_length + start_height - 1);
        assert_eq!(result.len(), num_new_ids);
        let result_set = to_hashset_owned(&result);
        assert_eq!(result_set, new_ids);

        let result = leader_scheduler.get_active_set(2 * active_window_length + start_height);
        assert_eq!(result.len(), 0);
        let result_set = to_hashset_owned(&result);
        assert!(result_set.is_empty());
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
        let mint = Mint::new((((num_validators + 1) / 2) * (num_validators + 1)) as i64);
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
                (num_validators - i) as i64,
                &mint.keypair(),
                new_pubkey,
                last_id,
            ).unwrap();
        }

        let validators_pk: Vec<Pubkey> = validators.iter().map(Keypair::pubkey).collect();
        let result = LeaderScheduler::rank_active_set(&bank, &validators_pk[..]);

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
                (num_validators - i) as i64,
                &validators[i],
                new_pubkey,
                last_id,
            ).unwrap();
        }

        let all_validators: Vec<Pubkey> = validators
            .iter()
            .chain(new_validators.iter())
            .map(Keypair::pubkey)
            .collect();
        let result = LeaderScheduler::rank_active_set(&bank, &all_validators[..]);
        assert_eq!(result.len(), new_validators.len());

        for (i, (pk, balance)) in result.into_iter().enumerate() {
            assert_eq!(balance, i as u64 + 1);
            assert_eq!(*pk, new_validators[num_validators - i - 1].pubkey());
        }

        // Break ties between validators with the same balances using public key
        let mint = Mint::new(num_validators as i64);
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

        let result = LeaderScheduler::rank_active_set(&bank, &tied_validators_pk[..]);
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
        // Set up the LeaderScheduler struct
        let bootstrap_leader_id = Keypair::new().pubkey();
        let bootstrap_height = 500;
        let leader_rotation_interval = 100;
        // Make sure seed_rotation_interval is big enough so we select all the
        // validators as part of the schedule each time (we need to check the active window
        // is the cause of validators being truncated later)
        let seed_rotation_interval = leader_rotation_interval * num_validators;
        let active_window_length = 1;

        let leader_scheduler_config = LeaderSchedulerConfig::new(
            bootstrap_leader_id,
            Some(bootstrap_height),
            Some(leader_rotation_interval),
            Some(seed_rotation_interval),
            Some(active_window_length),
        );

        let mut leader_scheduler = LeaderScheduler::new(&leader_scheduler_config);

        // Create the bank and validators
        let mint = Mint::new((((num_validators + 1) / 2) * (num_validators + 1)) as i64);
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
            validators.push(new_pubkey);
            // Vote at height i * active_window_length for validator i
            leader_scheduler.push_vote(new_pubkey, i * active_window_length);
            bank.transfer((i + 1) as i64, &mint.keypair(), new_pubkey, last_id)
                .unwrap();
        }

        // Generate schedule every active_window_length entries and check that
        // validators are falling out of the rotation as they fall out of the
        // active set
        for i in 0..num_validators {
            leader_scheduler.generate_schedule(i * active_window_length, &bank);
            let result = &leader_scheduler.leader_schedule;
            assert_eq!(num_validators - i, result.len() as u64);
            let expected_set = to_hashset(&validators[i as usize..]);
            let result_set = to_hashset(&result[..]);
            assert_eq!(expected_set, result_set);
        }
    }
}
