use crate::{consensus::VOTE_THRESHOLD_SIZE, rpc_subscriptions::RpcSubscriptions};
use solana_ledger::blockstore::Blockstore;
use solana_measure::measure::Measure;
use solana_metrics::datapoint_info;
use solana_runtime::bank::Bank;
use solana_sdk::clock::Slot;
use solana_vote_program::{vote_state::VoteState, vote_state::MAX_LOCKOUT_HISTORY};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    sync::{Arc, RwLock},
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

#[derive(Default)]
pub struct CacheSlotInfo {
    pub current_slot: Slot,
    pub node_root: Slot,
    pub largest_confirmed_root: Slot,
    pub highest_confirmed_slot: Slot,
}

pub type BlockCommitmentArray = [u64; MAX_LOCKOUT_HISTORY + 1];

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlockCommitment {
    pub commitment: BlockCommitmentArray,
}

impl BlockCommitment {
    pub fn increase_confirmation_stake(&mut self, confirmation_count: usize, stake: u64) {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.commitment[confirmation_count - 1] += stake;
    }

    pub fn get_confirmation_stake(&mut self, confirmation_count: usize) -> u64 {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        self.commitment[confirmation_count - 1]
    }

    pub fn increase_rooted_stake(&mut self, stake: u64) {
        self.commitment[MAX_LOCKOUT_HISTORY] += stake;
    }

    pub fn get_rooted_stake(&self) -> u64 {
        self.commitment[MAX_LOCKOUT_HISTORY]
    }

    #[cfg(test)]
    pub(crate) fn new(commitment: BlockCommitmentArray) -> Self {
        Self { commitment }
    }
}

pub struct BlockCommitmentCache {
    block_commitment: HashMap<Slot, BlockCommitment>,
    largest_confirmed_root: Slot,
    total_stake: u64,
    bank: Arc<Bank>,
    blockstore: Arc<Blockstore>,
    root: Slot,
    highest_confirmed_slot: Slot,
}

impl std::fmt::Debug for BlockCommitmentCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockCommitmentCache")
            .field("block_commitment", &self.block_commitment)
            .field("total_stake", &self.total_stake)
            .field(
                "bank",
                &format_args!("Bank({{current_slot: {:?}}})", self.bank.slot()),
            )
            .field("root", &self.root)
            .finish()
    }
}

impl BlockCommitmentCache {
    pub fn new(
        block_commitment: HashMap<Slot, BlockCommitment>,
        largest_confirmed_root: Slot,
        total_stake: u64,
        bank: Arc<Bank>,
        blockstore: Arc<Blockstore>,
        root: Slot,
        highest_confirmed_slot: Slot,
    ) -> Self {
        Self {
            block_commitment,
            largest_confirmed_root,
            total_stake,
            bank,
            blockstore,
            root,
            highest_confirmed_slot,
        }
    }

    pub fn default_with_blockstore(blockstore: Arc<Blockstore>) -> Self {
        Self {
            block_commitment: HashMap::default(),
            largest_confirmed_root: Slot::default(),
            total_stake: u64::default(),
            bank: Arc::new(Bank::default()),
            blockstore,
            root: Slot::default(),
            highest_confirmed_slot: Slot::default(),
        }
    }

    pub fn get_block_commitment(&self, slot: Slot) -> Option<&BlockCommitment> {
        self.block_commitment.get(&slot)
    }

    pub fn largest_confirmed_root(&self) -> Slot {
        self.largest_confirmed_root
    }

    pub fn total_stake(&self) -> u64 {
        self.total_stake
    }

    pub fn bank(&self) -> Arc<Bank> {
        self.bank.clone()
    }

    pub fn slot(&self) -> Slot {
        self.bank.slot()
    }

    pub fn root(&self) -> Slot {
        self.root
    }

    pub fn highest_confirmed_slot(&self) -> Slot {
        self.highest_confirmed_slot
    }

    fn highest_slot_with_confirmation_count(&self, confirmation_count: usize) -> Slot {
        assert!(confirmation_count > 0 && confirmation_count <= MAX_LOCKOUT_HISTORY);
        for slot in (self.root()..self.slot()).rev() {
            if let Some(count) = self.get_confirmation_count(slot) {
                if count >= confirmation_count {
                    return slot;
                }
            }
        }
        self.root
    }

    fn calculate_highest_confirmed_slot(&self) -> Slot {
        self.highest_slot_with_confirmation_count(1)
    }

    pub fn get_confirmation_count(&self, slot: Slot) -> Option<usize> {
        self.get_lockout_count(slot, VOTE_THRESHOLD_SIZE)
    }

    // Returns the lowest level at which at least `minimum_stake_percentage` of the total epoch
    // stake is locked out
    fn get_lockout_count(&self, slot: Slot, minimum_stake_percentage: f64) -> Option<usize> {
        self.get_block_commitment(slot).map(|block_commitment| {
            let iterator = block_commitment.commitment.iter().enumerate().rev();
            let mut sum = 0;
            for (i, stake) in iterator {
                sum += stake;
                if (sum as f64 / self.total_stake as f64) > minimum_stake_percentage {
                    return i + 1;
                }
            }
            0
        })
    }

    pub fn is_confirmed_rooted(&self, slot: Slot) -> bool {
        slot <= self.largest_confirmed_root()
            && (self.blockstore.is_root(slot) || self.bank.status_cache_ancestors().contains(&slot))
    }

    #[cfg(test)]
    pub fn new_for_tests_with_blockstore(blockstore: Arc<Blockstore>) -> Self {
        let mut block_commitment: HashMap<Slot, BlockCommitment> = HashMap::new();
        block_commitment.insert(0, BlockCommitment::default());
        Self {
            block_commitment,
            blockstore,
            total_stake: 42,
            largest_confirmed_root: Slot::default(),
            bank: Arc::new(Bank::default()),
            root: Slot::default(),
            highest_confirmed_slot: Slot::default(),
        }
    }

    #[cfg(test)]
    pub fn new_for_tests_with_blockstore_bank(
        blockstore: Arc<Blockstore>,
        bank: Arc<Bank>,
        root: Slot,
    ) -> Self {
        let mut block_commitment: HashMap<Slot, BlockCommitment> = HashMap::new();
        block_commitment.insert(0, BlockCommitment::default());
        Self {
            block_commitment,
            blockstore,
            total_stake: 42,
            largest_confirmed_root: root,
            bank,
            root,
            highest_confirmed_slot: root,
        }
    }

    pub(crate) fn set_largest_confirmed_root(&mut self, root: Slot) {
        self.largest_confirmed_root = root;
    }
}

pub struct CommitmentAggregationData {
    bank: Arc<Bank>,
    root: Slot,
    total_staked: u64,
}

impl CommitmentAggregationData {
    pub fn new(bank: Arc<Bank>, root: Slot, total_staked: u64) -> Self {
        Self {
            bank,
            root,
            total_staked,
        }
    }
}

fn get_largest_confirmed_root(mut rooted_stake: Vec<(Slot, u64)>, total_stake: u64) -> Slot {
    rooted_stake.sort_by(|a, b| a.0.cmp(&b.0).reverse());
    let mut stake_sum = 0;
    for (root, stake) in rooted_stake {
        stake_sum += stake;
        if (stake_sum as f64 / total_stake as f64) > VOTE_THRESHOLD_SIZE {
            return root;
        }
    }
    0
}

pub struct AggregateCommitmentService {
    t_commitment: JoinHandle<()>,
}

impl AggregateCommitmentService {
    pub fn new(
        exit: &Arc<AtomicBool>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        subscriptions: Arc<RpcSubscriptions>,
    ) -> (Sender<CommitmentAggregationData>, Self) {
        let (sender, receiver): (
            Sender<CommitmentAggregationData>,
            Receiver<CommitmentAggregationData>,
        ) = channel();
        let exit_ = exit.clone();
        (
            sender,
            Self {
                t_commitment: Builder::new()
                    .name("solana-aggregate-stake-lockouts".to_string())
                    .spawn(move || loop {
                        if exit_.load(Ordering::Relaxed) {
                            break;
                        }

                        if let Err(RecvTimeoutError::Disconnected) =
                            Self::run(&receiver, &block_commitment_cache, &subscriptions, &exit_)
                        {
                            break;
                        }
                    })
                    .unwrap(),
            },
        )
    }

    fn run(
        receiver: &Receiver<CommitmentAggregationData>,
        block_commitment_cache: &RwLock<BlockCommitmentCache>,
        subscriptions: &Arc<RpcSubscriptions>,
        exit: &Arc<AtomicBool>,
    ) -> Result<(), RecvTimeoutError> {
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }

            let mut aggregation_data = receiver.recv_timeout(Duration::from_secs(1))?;

            while let Ok(new_data) = receiver.try_recv() {
                aggregation_data = new_data;
            }

            let ancestors = aggregation_data.bank.status_cache_ancestors();
            if ancestors.is_empty() {
                continue;
            }

            let mut aggregate_commitment_time = Measure::start("aggregate-commitment-ms");
            let (block_commitment, rooted_stake) =
                Self::aggregate_commitment(&ancestors, &aggregation_data.bank);

            let largest_confirmed_root =
                get_largest_confirmed_root(rooted_stake, aggregation_data.total_staked);

            let mut new_block_commitment = BlockCommitmentCache::new(
                block_commitment,
                largest_confirmed_root,
                aggregation_data.total_staked,
                aggregation_data.bank,
                block_commitment_cache.read().unwrap().blockstore.clone(),
                aggregation_data.root,
                aggregation_data.root,
            );
            new_block_commitment.highest_confirmed_slot =
                new_block_commitment.calculate_highest_confirmed_slot();

            let mut w_block_commitment_cache = block_commitment_cache.write().unwrap();

            std::mem::swap(&mut *w_block_commitment_cache, &mut new_block_commitment);
            aggregate_commitment_time.stop();
            datapoint_info!(
                "block-commitment-cache",
                (
                    "aggregate-commitment-ms",
                    aggregate_commitment_time.as_ms() as i64,
                    i64
                )
            );

            subscriptions.notify_subscribers(CacheSlotInfo {
                current_slot: w_block_commitment_cache.slot(),
                node_root: w_block_commitment_cache.root(),
                largest_confirmed_root: w_block_commitment_cache.largest_confirmed_root(),
                highest_confirmed_slot: w_block_commitment_cache.highest_confirmed_slot(),
            });
        }
    }

    pub fn aggregate_commitment(
        ancestors: &[Slot],
        bank: &Bank,
    ) -> (HashMap<Slot, BlockCommitment>, Vec<(Slot, u64)>) {
        assert!(!ancestors.is_empty());

        // Check ancestors is sorted
        for a in ancestors.windows(2) {
            assert!(a[0] < a[1]);
        }

        let mut commitment = HashMap::new();
        let mut rooted_stake: Vec<(Slot, u64)> = Vec::new();
        for (_, (lamports, account)) in bank.vote_accounts().into_iter() {
            if lamports == 0 {
                continue;
            }
            let vote_state = VoteState::from(&account);
            if vote_state.is_none() {
                continue;
            }

            let vote_state = vote_state.unwrap();
            Self::aggregate_commitment_for_vote_account(
                &mut commitment,
                &mut rooted_stake,
                &vote_state,
                ancestors,
                lamports,
            );
        }

        (commitment, rooted_stake)
    }

    fn aggregate_commitment_for_vote_account(
        commitment: &mut HashMap<Slot, BlockCommitment>,
        rooted_stake: &mut Vec<(Slot, u64)>,
        vote_state: &VoteState,
        ancestors: &[Slot],
        lamports: u64,
    ) {
        assert!(!ancestors.is_empty());
        let mut ancestors_index = 0;
        if let Some(root) = vote_state.root_slot {
            for (i, a) in ancestors.iter().enumerate() {
                if *a <= root {
                    commitment
                        .entry(*a)
                        .or_insert_with(BlockCommitment::default)
                        .increase_rooted_stake(lamports);
                } else {
                    ancestors_index = i;
                    break;
                }
            }
            rooted_stake.push((root, lamports));
        }

        for vote in &vote_state.votes {
            while ancestors[ancestors_index] <= vote.slot {
                commitment
                    .entry(ancestors[ancestors_index])
                    .or_insert_with(BlockCommitment::default)
                    .increase_confirmation_stake(vote.confirmation_count as usize, lamports);
                ancestors_index += 1;

                if ancestors_index == ancestors.len() {
                    return;
                }
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.t_commitment.join()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_ledger::{
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
    };
    use solana_sdk::{genesis_config::GenesisConfig, pubkey::Pubkey};
    use solana_stake_program::stake_state;
    use solana_vote_program::vote_state::{self, VoteStateVersions};

    #[test]
    fn test_block_commitment() {
        let mut cache = BlockCommitment::default();
        assert_eq!(cache.get_confirmation_stake(1), 0);
        cache.increase_confirmation_stake(1, 10);
        assert_eq!(cache.get_confirmation_stake(1), 10);
        cache.increase_confirmation_stake(1, 20);
        assert_eq!(cache.get_confirmation_stake(1), 30);
    }

    #[test]
    fn test_get_confirmations() {
        let bank = Arc::new(Bank::default());
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        // Build BlockCommitmentCache with votes at depths 0 and 1 for 2 slots
        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(1, 5);
        cache0.increase_confirmation_stake(2, 40);

        let mut cache1 = BlockCommitment::default();
        cache1.increase_confirmation_stake(1, 40);
        cache1.increase_confirmation_stake(2, 5);

        let mut cache2 = BlockCommitment::default();
        cache2.increase_confirmation_stake(1, 20);
        cache2.increase_confirmation_stake(2, 5);

        let mut block_commitment = HashMap::new();
        block_commitment.entry(0).or_insert(cache0);
        block_commitment.entry(1).or_insert(cache1);
        block_commitment.entry(2).or_insert(cache2);
        let block_commitment_cache =
            BlockCommitmentCache::new(block_commitment, 0, 50, bank, blockstore, 0, 0);

        assert_eq!(block_commitment_cache.get_confirmation_count(0), Some(2));
        assert_eq!(block_commitment_cache.get_confirmation_count(1), Some(1));
        assert_eq!(block_commitment_cache.get_confirmation_count(2), Some(0),);
        assert_eq!(block_commitment_cache.get_confirmation_count(3), None,);
    }

    #[test]
    fn test_is_confirmed_rooted() {
        let bank = Arc::new(Bank::default());
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        blockstore.set_roots(&[0, 1]).unwrap();
        // Build BlockCommitmentCache with rooted slots
        let mut cache0 = BlockCommitment::default();
        cache0.increase_rooted_stake(50);
        let mut cache1 = BlockCommitment::default();
        cache1.increase_rooted_stake(40);
        let mut cache2 = BlockCommitment::default();
        cache2.increase_rooted_stake(20);

        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert(cache0);
        block_commitment.entry(2).or_insert(cache1);
        block_commitment.entry(3).or_insert(cache2);
        let largest_confirmed_root = 1;
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            largest_confirmed_root,
            50,
            bank,
            blockstore,
            0,
            0,
        );

        assert!(block_commitment_cache.is_confirmed_rooted(0));
        assert!(block_commitment_cache.is_confirmed_rooted(1));
        assert!(!block_commitment_cache.is_confirmed_rooted(2));
        assert!(!block_commitment_cache.is_confirmed_rooted(3));
    }

    #[test]
    fn test_get_largest_confirmed_root() {
        assert_eq!(get_largest_confirmed_root(vec![], 10), 0);
        let mut rooted_stake = vec![];
        rooted_stake.push((0, 5));
        rooted_stake.push((1, 5));
        assert_eq!(get_largest_confirmed_root(rooted_stake, 10), 0);
        let mut rooted_stake = vec![];
        rooted_stake.push((1, 5));
        rooted_stake.push((0, 10));
        rooted_stake.push((2, 5));
        rooted_stake.push((1, 4));
        assert_eq!(get_largest_confirmed_root(rooted_stake, 10), 1);
    }

    #[test]
    fn test_highest_confirmed_slot() {
        let bank = Arc::new(Bank::new(&GenesisConfig::default()));
        let bank_slot_5 = Arc::new(Bank::new_from_parent(&bank, &Pubkey::default(), 5));
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let total_stake = 50;

        // Build cache with confirmation_count 2 given total_stake
        let mut cache0 = BlockCommitment::default();
        cache0.increase_confirmation_stake(1, 5);
        cache0.increase_confirmation_stake(2, 40);

        // Build cache with confirmation_count 1 given total_stake
        let mut cache1 = BlockCommitment::default();
        cache1.increase_confirmation_stake(1, 40);
        cache1.increase_confirmation_stake(2, 5);

        // Build cache with confirmation_count 0 given total_stake
        let mut cache2 = BlockCommitment::default();
        cache2.increase_confirmation_stake(1, 20);
        cache2.increase_confirmation_stake(2, 5);

        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache0.clone()); // Slot 1, conf 2
        block_commitment.entry(2).or_insert_with(|| cache1.clone()); // Slot 2, conf 1
        block_commitment.entry(3).or_insert_with(|| cache2.clone()); // Slot 3, conf 0
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            0,
            total_stake,
            bank_slot_5.clone(),
            blockstore.clone(),
            0,
            0,
        );

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 2);

        // Build map with multiple slots at conf 1
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache1.clone()); // Slot 1, conf 1
        block_commitment.entry(2).or_insert_with(|| cache1.clone()); // Slot 2, conf 1
        block_commitment.entry(3).or_insert_with(|| cache2.clone()); // Slot 3, conf 0
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            0,
            total_stake,
            bank_slot_5.clone(),
            blockstore.clone(),
            0,
            0,
        );

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 2);

        // Build map with slot gaps
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache1.clone()); // Slot 1, conf 1
        block_commitment.entry(3).or_insert(cache1); // Slot 3, conf 1
        block_commitment.entry(5).or_insert_with(|| cache2.clone()); // Slot 5, conf 0
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            0,
            total_stake,
            bank_slot_5.clone(),
            blockstore.clone(),
            0,
            0,
        );

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 3);

        // Build map with no conf 1 slots, but one higher
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert(cache0); // Slot 1, conf 2
        block_commitment.entry(2).or_insert_with(|| cache2.clone()); // Slot 2, conf 0
        block_commitment.entry(3).or_insert_with(|| cache2.clone()); // Slot 3, conf 0
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            0,
            total_stake,
            bank_slot_5.clone(),
            blockstore.clone(),
            0,
            0,
        );

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 1);

        // Build map with no conf 1 or higher slots
        let mut block_commitment = HashMap::new();
        block_commitment.entry(1).or_insert_with(|| cache2.clone()); // Slot 1, conf 0
        block_commitment.entry(2).or_insert_with(|| cache2.clone()); // Slot 2, conf 0
        block_commitment.entry(3).or_insert(cache2); // Slot 3, conf 0
        let block_commitment_cache = BlockCommitmentCache::new(
            block_commitment,
            0,
            total_stake,
            bank_slot_5,
            blockstore,
            0,
            0,
        );

        assert_eq!(block_commitment_cache.calculate_highest_confirmed_slot(), 0);
    }

    #[test]
    fn test_aggregate_commitment_for_vote_account_1() {
        let ancestors = vec![3, 4, 5, 7, 9, 11];
        let mut commitment = HashMap::new();
        let mut rooted_stake = vec![];
        let lamports = 5;
        let mut vote_state = VoteState::default();

        let root = *ancestors.last().unwrap();
        vote_state.root_slot = Some(root);
        AggregateCommitmentService::aggregate_commitment_for_vote_account(
            &mut commitment,
            &mut rooted_stake,
            &vote_state,
            &ancestors,
            lamports,
        );

        for a in ancestors {
            let mut expected = BlockCommitment::default();
            expected.increase_rooted_stake(lamports);
            assert_eq!(*commitment.get(&a).unwrap(), expected);
        }
        assert_eq!(rooted_stake[0], (root, lamports));
    }

    #[test]
    fn test_aggregate_commitment_for_vote_account_2() {
        let ancestors = vec![3, 4, 5, 7, 9, 11];
        let mut commitment = HashMap::new();
        let mut rooted_stake = vec![];
        let lamports = 5;
        let mut vote_state = VoteState::default();

        let root = ancestors[2];
        vote_state.root_slot = Some(root);
        vote_state.process_slot_vote_unchecked(*ancestors.last().unwrap());
        AggregateCommitmentService::aggregate_commitment_for_vote_account(
            &mut commitment,
            &mut rooted_stake,
            &vote_state,
            &ancestors,
            lamports,
        );

        for a in ancestors {
            if a <= root {
                let mut expected = BlockCommitment::default();
                expected.increase_rooted_stake(lamports);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(1, lamports);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            }
        }
        assert_eq!(rooted_stake[0], (root, lamports));
    }

    #[test]
    fn test_aggregate_commitment_for_vote_account_3() {
        let ancestors = vec![3, 4, 5, 7, 9, 10, 11];
        let mut commitment = HashMap::new();
        let mut rooted_stake = vec![];
        let lamports = 5;
        let mut vote_state = VoteState::default();

        let root = ancestors[2];
        vote_state.root_slot = Some(root);
        assert!(ancestors[4] + 2 >= ancestors[6]);
        vote_state.process_slot_vote_unchecked(ancestors[4]);
        vote_state.process_slot_vote_unchecked(ancestors[6]);
        AggregateCommitmentService::aggregate_commitment_for_vote_account(
            &mut commitment,
            &mut rooted_stake,
            &vote_state,
            &ancestors,
            lamports,
        );

        for (i, a) in ancestors.iter().enumerate() {
            if *a <= root {
                let mut expected = BlockCommitment::default();
                expected.increase_rooted_stake(lamports);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else if i <= 4 {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(2, lamports);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else if i <= 6 {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(1, lamports);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            }
        }
        assert_eq!(rooted_stake[0], (root, lamports));
    }

    #[test]
    fn test_aggregate_commitment_validity() {
        let ancestors = vec![3, 4, 5, 7, 9, 10, 11];
        let GenesisConfigInfo {
            mut genesis_config, ..
        } = create_genesis_config(10_000);

        let rooted_stake_amount = 40;

        let sk1 = Pubkey::new_rand();
        let pk1 = Pubkey::new_rand();
        let mut vote_account1 = vote_state::create_account(&pk1, &Pubkey::new_rand(), 0, 100);
        let stake_account1 =
            stake_state::create_account(&sk1, &pk1, &vote_account1, &genesis_config.rent, 100);
        let sk2 = Pubkey::new_rand();
        let pk2 = Pubkey::new_rand();
        let mut vote_account2 = vote_state::create_account(&pk2, &Pubkey::new_rand(), 0, 50);
        let stake_account2 =
            stake_state::create_account(&sk2, &pk2, &vote_account2, &genesis_config.rent, 50);
        let sk3 = Pubkey::new_rand();
        let pk3 = Pubkey::new_rand();
        let mut vote_account3 = vote_state::create_account(&pk3, &Pubkey::new_rand(), 0, 1);
        let stake_account3 = stake_state::create_account(
            &sk3,
            &pk3,
            &vote_account3,
            &genesis_config.rent,
            rooted_stake_amount,
        );
        let sk4 = Pubkey::new_rand();
        let pk4 = Pubkey::new_rand();
        let mut vote_account4 = vote_state::create_account(&pk4, &Pubkey::new_rand(), 0, 1);
        let stake_account4 = stake_state::create_account(
            &sk4,
            &pk4,
            &vote_account4,
            &genesis_config.rent,
            rooted_stake_amount,
        );

        genesis_config.accounts.extend(vec![
            (pk1, vote_account1.clone()),
            (sk1, stake_account1),
            (pk2, vote_account2.clone()),
            (sk2, stake_account2),
            (pk3, vote_account3.clone()),
            (sk3, stake_account3),
            (pk4, vote_account4.clone()),
            (sk4, stake_account4),
        ]);

        // Create bank
        let bank = Arc::new(Bank::new(&genesis_config));

        let mut vote_state1 = VoteState::from(&vote_account1).unwrap();
        vote_state1.process_slot_vote_unchecked(3);
        vote_state1.process_slot_vote_unchecked(5);
        let versioned = VoteStateVersions::Current(Box::new(vote_state1));
        VoteState::to(&versioned, &mut vote_account1).unwrap();
        bank.store_account(&pk1, &vote_account1);

        let mut vote_state2 = VoteState::from(&vote_account2).unwrap();
        vote_state2.process_slot_vote_unchecked(9);
        vote_state2.process_slot_vote_unchecked(10);
        let versioned = VoteStateVersions::Current(Box::new(vote_state2));
        VoteState::to(&versioned, &mut vote_account2).unwrap();
        bank.store_account(&pk2, &vote_account2);

        let mut vote_state3 = VoteState::from(&vote_account3).unwrap();
        vote_state3.root_slot = Some(1);
        let versioned = VoteStateVersions::Current(Box::new(vote_state3));
        VoteState::to(&versioned, &mut vote_account3).unwrap();
        bank.store_account(&pk3, &vote_account3);

        let mut vote_state4 = VoteState::from(&vote_account4).unwrap();
        vote_state4.root_slot = Some(2);
        let versioned = VoteStateVersions::Current(Box::new(vote_state4));
        VoteState::to(&versioned, &mut vote_account4).unwrap();
        bank.store_account(&pk4, &vote_account4);

        let (commitment, rooted_stake) =
            AggregateCommitmentService::aggregate_commitment(&ancestors, &bank);

        for a in ancestors {
            if a <= 3 {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(2, 150);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else if a <= 5 {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(1, 100);
                expected.increase_confirmation_stake(2, 50);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else if a <= 9 {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(2, 50);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else if a <= 10 {
                let mut expected = BlockCommitment::default();
                expected.increase_confirmation_stake(1, 50);
                assert_eq!(*commitment.get(&a).unwrap(), expected);
            } else {
                assert!(commitment.get(&a).is_none());
            }
        }
        assert_eq!(rooted_stake.len(), 2);
        assert_eq!(get_largest_confirmed_root(rooted_stake, 100), 1)
    }
}
