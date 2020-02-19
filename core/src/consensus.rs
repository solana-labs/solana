use chrono::prelude::*;
use solana_ledger::{bank_forks::BankForks, blockstore::Blockstore, blockstore_db};
use solana_metrics::datapoint_debug;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    hash::Hash,
    pubkey::Pubkey,
    signature::{KeypairUtil, Signature},
};
use solana_vote_program::vote_state::{
    BlockTimestamp, Lockout, Vote, VoteState, MAX_LOCKOUT_HISTORY, TIMESTAMP_SLOT_INTERVAL,
};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufReader, Write},
    path::PathBuf,
    sync::Arc,
    time::Instant,
};
use thiserror::Error;

pub const TOWER_SNAPSHOT_NAME: &str = "tower";
const TOWER_TEMP_SNAPSHOT_NAME: &str = "tower.tmp";
pub const VOTE_THRESHOLD_DEPTH: usize = 8;
pub const VOTE_THRESHOLD_SIZE: f64 = 2f64 / 3f64;

#[derive(Default, Debug, Clone)]
pub struct StakeLockout {
    lockout: u64,
    stake: u64,
}

impl StakeLockout {
    pub fn new(lockout: u64, stake: u64) -> Self {
        Self { lockout, stake }
    }
    pub fn lockout(&self) -> u64 {
        self.lockout
    }
    pub fn stake(&self) -> u64 {
        self.stake
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Tower {
    pub node_pubkey: Pubkey,
    threshold_depth: usize,
    threshold_size: f64,
    lockouts: VoteState,
    last_vote: Vote,
    last_timestamp: BlockTimestamp,
}

impl Default for Tower {
    fn default() -> Self {
        Self {
            node_pubkey: Pubkey::default(),
            threshold_depth: VOTE_THRESHOLD_DEPTH,
            threshold_size: VOTE_THRESHOLD_SIZE,
            lockouts: VoteState::default(),
            last_vote: Vote::default(),
            last_timestamp: BlockTimestamp::default(),
        }
    }
}

impl Tower {
    pub fn new(node_pubkey: &Pubkey, vote_account_pubkey: &Pubkey, bank_forks: &BankForks) -> Self {
        let mut tower = Self::new_with_key(node_pubkey);

        tower.initialize_lockouts_from_bank_forks(&bank_forks, vote_account_pubkey);

        tower
    }

    pub fn new_with_key(node_pubkey: &Pubkey) -> Self {
        Self {
            node_pubkey: *node_pubkey,
            ..Tower::default()
        }
    }

    #[cfg(test)]
    pub fn new_for_tests(threshold_depth: usize, threshold_size: f64) -> Self {
        Self {
            threshold_depth,
            threshold_size,
            ..Tower::default()
        }
    }

    pub fn collect_vote_lockouts<F>(
        &self,
        bank_slot: u64,
        vote_accounts: F,
        ancestors: &HashMap<Slot, HashSet<u64>>,
    ) -> (HashMap<Slot, StakeLockout>, u64, u128)
    where
        F: Iterator<Item = (Pubkey, (u64, Account))>,
    {
        let mut stake_lockouts = HashMap::new();
        let mut total_stake = 0;
        let mut total_weight = 0;
        for (key, (lamports, account)) in vote_accounts {
            if lamports == 0 {
                continue;
            }
            trace!("{} {} with stake {}", self.node_pubkey, key, lamports);
            let vote_state = VoteState::from(&account);
            if vote_state.is_none() {
                datapoint_warn!(
                    "tower_warn",
                    (
                        "warn",
                        format!("Unable to get vote_state from account {}", key),
                        String
                    ),
                );
                continue;
            }
            let mut vote_state = vote_state.unwrap();

            if key == self.node_pubkey || vote_state.node_pubkey == self.node_pubkey {
                debug!("vote state {:?}", vote_state);
                debug!(
                    "observed slot {}",
                    vote_state.nth_recent_vote(0).map(|v| v.slot).unwrap_or(0) as i64
                );
                debug!("observed root {}", vote_state.root_slot.unwrap_or(0) as i64);
                datapoint_debug!(
                    "tower-observed",
                    (
                        "slot",
                        vote_state.nth_recent_vote(0).map(|v| v.slot).unwrap_or(0),
                        i64
                    ),
                    ("root", vote_state.root_slot.unwrap_or(0), i64)
                );
            }
            let start_root = vote_state.root_slot;

            vote_state.process_slot_vote_unchecked(bank_slot);

            for vote in &vote_state.votes {
                total_weight += vote.lockout() as u128 * lamports as u128;
                Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
            }

            if start_root != vote_state.root_slot {
                if let Some(root) = start_root {
                    let vote = Lockout {
                        confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                        slot: root,
                    };
                    trace!("ROOT: {}", vote.slot);
                    total_weight += vote.lockout() as u128 * lamports as u128;
                    Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
                }
            }
            if let Some(root) = vote_state.root_slot {
                let vote = Lockout {
                    confirmation_count: MAX_LOCKOUT_HISTORY as u32,
                    slot: root,
                };
                total_weight += vote.lockout() as u128 * lamports as u128;
                Self::update_ancestor_lockouts(&mut stake_lockouts, &vote, ancestors);
            }

            // The last vote in the vote stack is a simulated vote on bank_slot, which
            // we added to the vote stack earlier in this function by calling process_vote().
            // We don't want to update the ancestors stakes of this vote b/c it does not
            // represent an actual vote by the validator.

            // Note: It should not be possible for any vote state in this bank to have
            // a vote for a slot >= bank_slot, so we are guaranteed that the last vote in
            // this vote stack is the simulated vote, so this fetch should be sufficient
            // to find the last unsimulated vote.
            assert_eq!(
                vote_state.nth_recent_vote(0).map(|l| l.slot),
                Some(bank_slot)
            );
            if let Some(vote) = vote_state.nth_recent_vote(1) {
                // Update all the parents of this last vote with the stake of this vote account
                Self::update_ancestor_stakes(&mut stake_lockouts, vote.slot, lamports, ancestors);
            }
            total_stake += lamports;
        }
        (stake_lockouts, total_stake, total_weight)
    }

    pub fn is_slot_confirmed(
        &self,
        slot: u64,
        lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
    ) -> bool {
        lockouts
            .get(&slot)
            .map(|lockout| (lockout.stake as f64 / total_staked as f64) > self.threshold_size)
            .unwrap_or(false)
    }

    fn new_vote(
        local_vote_state: &VoteState,
        slot: u64,
        hash: Hash,
        last_bank_slot: Option<Slot>,
    ) -> (Vote, usize) {
        let mut local_vote_state = local_vote_state.clone();
        let vote = Vote::new(vec![slot], hash);
        local_vote_state.process_vote_unchecked(&vote);
        let slots = if let Some(last_bank_slot) = last_bank_slot {
            local_vote_state
                .votes
                .iter()
                .map(|v| v.slot)
                .skip_while(|s| *s <= last_bank_slot)
                .collect()
        } else {
            local_vote_state.votes.iter().map(|v| v.slot).collect()
        };
        trace!(
            "new vote with {:?} {:?} {:?}",
            last_bank_slot,
            slots,
            local_vote_state.votes
        );
        (Vote::new(slots, hash), local_vote_state.votes.len() - 1)
    }

    fn last_bank_vote(bank: &Bank, vote_account_pubkey: &Pubkey) -> Option<Slot> {
        let vote_account = bank.vote_accounts().get(vote_account_pubkey)?.1.clone();
        let bank_vote_state = VoteState::deserialize(&vote_account.data).ok()?;
        bank_vote_state.votes.iter().map(|v| v.slot).last()
    }

    pub fn new_vote_from_bank(&self, bank: &Bank, vote_account_pubkey: &Pubkey) -> (Vote, usize) {
        let last_vote = Self::last_bank_vote(bank, vote_account_pubkey);
        Self::new_vote(&self.lockouts, bank.slot(), bank.hash(), last_vote)
    }

    pub fn record_bank_vote(&mut self, vote: Vote) -> Option<Slot> {
        let slot = *vote.slots.last().unwrap_or(&0);
        trace!("{} record_vote for {}", self.node_pubkey, slot);
        let root_slot = self.lockouts.root_slot;
        self.lockouts.process_vote_unchecked(&vote);
        self.last_vote = vote;

        datapoint_debug!(
            "tower-vote",
            ("latest", slot, i64),
            ("root", self.lockouts.root_slot.unwrap_or(0), i64)
        );
        if root_slot != self.lockouts.root_slot {
            Some(self.lockouts.root_slot.unwrap())
        } else {
            None
        }
    }

    pub fn record_vote(&mut self, slot: Slot, hash: Hash) -> Option<Slot> {
        let vote = Vote::new(vec![slot], hash);
        self.record_bank_vote(vote)
    }

    pub fn last_vote(&self) -> Vote {
        self.last_vote.clone()
    }

    pub fn last_vote_and_timestamp(&mut self) -> Vote {
        let mut last_vote = self.last_vote();
        let current_slot = last_vote.slots.iter().max().unwrap_or(&0);
        last_vote.timestamp = self.maybe_timestamp(*current_slot);
        last_vote
    }

    pub fn root(&self) -> Option<Slot> {
        self.lockouts.root_slot
    }

    // a slot is not recent if it's older than the newest vote we have
    pub fn is_recent(&self, slot: u64) -> bool {
        if let Some(last_vote) = self.lockouts.votes.back() {
            if slot <= last_vote.slot {
                return false;
            }
        }
        true
    }

    pub fn has_voted(&self, slot: u64) -> bool {
        for vote in &self.lockouts.votes {
            if vote.slot == slot {
                return true;
            }
        }
        false
    }

    pub fn is_locked_out(&self, slot: Slot, ancestors: &HashMap<Slot, HashSet<Slot>>) -> bool {
        assert!(ancestors.contains_key(&slot));

        if !self.is_recent(slot) {
            return true;
        }

        let mut lockouts = self.lockouts.clone();
        lockouts.process_slot_vote_unchecked(slot);
        for vote in &lockouts.votes {
            if vote.slot == slot {
                continue;
            }
            if !ancestors[&slot].contains(&vote.slot) {
                return true;
            }
        }
        if let Some(root_slot) = lockouts.root_slot {
            // This case should never happen because bank forks purges all
            // non-descendants of the root every time root is set
            if slot != root_slot {
                assert!(ancestors[&slot].contains(&root_slot));
            }
        }

        false
    }
    pub fn check_vote_stake_threshold(
        &self,
        slot: u64,
        stake_lockouts: &HashMap<u64, StakeLockout>,
        total_staked: u64,
    ) -> bool {
        let mut lockouts = self.lockouts.clone();
        lockouts.process_slot_vote_unchecked(slot);
        let vote = lockouts.nth_recent_vote(self.threshold_depth);
        if let Some(vote) = vote {
            if let Some(fork_stake) = stake_lockouts.get(&vote.slot) {
                let lockout = fork_stake.stake as f64 / total_staked as f64;
                trace!(
                    "fork_stake slot: {} lockout: {} fork_stake: {} total_stake: {}",
                    slot,
                    lockout,
                    fork_stake.stake,
                    total_staked
                );
                if vote.confirmation_count as usize > self.threshold_depth {
                    for old_vote in &self.lockouts.votes {
                        if old_vote.slot == vote.slot
                            && old_vote.confirmation_count == vote.confirmation_count
                        {
                            return true;
                        }
                    }
                }
                lockout > self.threshold_size
            } else {
                false
            }
        } else {
            true
        }
    }

    /// Update lockouts for all the ancestors
    fn update_ancestor_lockouts(
        stake_lockouts: &mut HashMap<Slot, StakeLockout>,
        vote: &Lockout,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) {
        // If there's no ancestors, that means this slot must be from before the current root,
        // in which case the lockouts won't be calculated in bank_weight anyways, so ignore
        // this slot
        let vote_slot_ancestors = ancestors.get(&vote.slot);
        if vote_slot_ancestors.is_none() {
            return;
        }
        let mut slot_with_ancestors = vec![vote.slot];
        slot_with_ancestors.extend(vote_slot_ancestors.unwrap());
        for slot in slot_with_ancestors {
            let entry = &mut stake_lockouts.entry(slot).or_default();
            entry.lockout += vote.lockout();
        }
    }

    /// Update stake for all the ancestors.
    /// Note, stake is the same for all the ancestor.
    fn update_ancestor_stakes(
        stake_lockouts: &mut HashMap<Slot, StakeLockout>,
        slot: Slot,
        lamports: u64,
        ancestors: &HashMap<Slot, HashSet<Slot>>,
    ) {
        // If there's no ancestors, that means this slot must be from before the current root,
        // in which case the lockouts won't be calculated in bank_weight anyways, so ignore
        // this slot
        let vote_slot_ancestors = ancestors.get(&slot);
        if vote_slot_ancestors.is_none() {
            return;
        }
        let mut slot_with_ancestors = vec![slot];
        slot_with_ancestors.extend(vote_slot_ancestors.unwrap());
        for slot in slot_with_ancestors {
            let entry = &mut stake_lockouts.entry(slot).or_default();
            entry.stake += lamports;
        }
    }

    fn bank_weight(&self, bank: &Bank, ancestors: &HashMap<Slot, HashSet<Slot>>) -> u128 {
        let (_, _, bank_weight) =
            self.collect_vote_lockouts(bank.slot(), bank.vote_accounts().into_iter(), ancestors);
        bank_weight
    }

    fn find_heaviest_bank(&self, bank_forks: &BankForks) -> Option<Arc<Bank>> {
        let ancestors = bank_forks.ancestors();
        let mut bank_weights: Vec<_> = bank_forks
            .frozen_banks()
            .values()
            .map(|b| {
                (
                    self.bank_weight(b, &ancestors),
                    b.parents().len(),
                    b.clone(),
                )
            })
            .collect();
        bank_weights.sort_by_key(|b| (b.0, b.1));
        bank_weights.pop().map(|b| b.2)
    }

    fn initialize_lockouts_from_bank_forks(
        &mut self,
        bank_forks: &BankForks,
        vote_account_pubkey: &Pubkey,
    ) {
        if let Some(bank) = self.find_heaviest_bank(bank_forks) {
            let root = bank_forks.root();
            if let Some((_stake, vote_account)) = bank.vote_accounts().get(vote_account_pubkey) {
                let mut vote_state = VoteState::deserialize(&vote_account.data)
                    .expect("vote_account isn't a VoteState?");
                vote_state.root_slot = Some(root);
                vote_state.votes.retain(|v| v.slot > root);
                trace!(
                    "{} lockouts initialized to {:?}",
                    self.node_pubkey,
                    vote_state
                );

                assert_eq!(
                    vote_state.node_pubkey, self.node_pubkey,
                    "vote account's node_pubkey doesn't match",
                );
                self.lockouts = vote_state;
            }
        }
    }

    fn maybe_timestamp(&mut self, current_slot: Slot) -> Option<UnixTimestamp> {
        if self.last_timestamp.slot == 0
            || self.last_timestamp.slot < (current_slot - (current_slot % TIMESTAMP_SLOT_INTERVAL))
        {
            let timestamp = Utc::now().timestamp();
            self.last_timestamp = BlockTimestamp {
                slot: current_slot,
                timestamp,
            };
            Some(timestamp)
        } else {
            None
        }
    }

    pub fn save_to_file<T: KeypairUtil>(&self, path: &PathBuf, keypair: &T) -> Result<()> {
        let timer = Instant::now();
        let saveable_tower = SavedTower::new(self, keypair)?;
        fs::create_dir_all(path)?;
        let mut snapshot_file = File::create(path.join(TOWER_TEMP_SNAPSHOT_NAME))?;
        bincode::serialize_into(&mut snapshot_file, &saveable_tower)?;
        snapshot_file.flush()?;
        fs::rename(
            path.join(TOWER_TEMP_SNAPSHOT_NAME),
            path.join(TOWER_SNAPSHOT_NAME),
        )?;
        let snapshot_time = timer.elapsed().as_millis() as usize;
        inc_new_counter_info!("tower_snapshot_duration_ms", snapshot_time);
        Ok(())
    }

    pub fn reload_from_file(
        path: &PathBuf,
        my_pubkey: &Pubkey,
        vote_account: &Pubkey,
    ) -> Result<Self> {
        info!("Looking for Tower snapshot at {:?}", path);
        // try to reload tower from disk, otherwise reconstruct one
        let tower_file = File::open(path.join(TOWER_SNAPSHOT_NAME))?;
        let mut stream = BufReader::new(tower_file);
        let tower: SavedTower = bincode::deserialize_from(&mut stream)?;
        if !tower.verify(vote_account) {
            error!("Signature on tower in invalid");
            return Err(TowerError::InvalidSignature);
        }
        let tower = tower.deserialize()?;
        // check that the tower actually belongs to this node
        if &tower.node_pubkey != my_pubkey {
            error!(
                "Wrong tower state found. My pubkey {:?} but found tower for {:?}",
                my_pubkey, tower.node_pubkey
            );
            return Err(TowerError::WrongPubkey);
        }
        info!("Restoring tower from saved state.");
        Ok(tower)
    }
}

#[derive(Default, Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct SavedTower {
    signature: Signature,
    data: Vec<u8>,
}

impl SavedTower {
    pub fn new<T: KeypairUtil>(tower: &Tower, keypair: &T) -> Result<Self> {
        let data = bincode::serialize(tower)?;
        let signature = keypair.sign_message(&data);
        Ok(Self { data, signature })
    }

    pub fn verify(&self, pubkey: &Pubkey) -> bool {
        self.signature.verify(pubkey.as_ref(), &self.data)
    }

    pub fn deserialize(&self) -> Result<Tower> {
        bincode::deserialize(&self.data).map_err(|e| e.into())
    }
}

pub type Result<T> = std::result::Result<T, TowerError>;

pub fn reconcile_blockstore_roots_with_tower(
    tower: &Tower,
    blockstore: &Blockstore,
) -> blockstore_db::Result<()> {
    if let Some(troot) = tower.root() {
        let broot = blockstore.last_root();
        if broot < troot {
            let new_roots: Vec<_> = blockstore
                .slot_meta_iterator(broot + 1)?
                .map(|(slot, _)| slot)
                .take_while(|slot| *slot <= troot)
                .collect();
            return blockstore.set_roots(&new_roots);
        }
    }
    Ok(())
}

#[derive(Error, Debug)]
pub enum TowerError {
    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),

    #[error("Serialization Error: {0}")]
    SerializeError(#[from] Box<bincode::ErrorKind>),

    #[error("The signature on the saved tower is invalid")]
    InvalidSignature,

    #[error("The tower pubkey does not match the validator pubkey")]
    WrongPubkey,
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::replay_stage::{ForkProgress, ReplayStage};
    use matches::assert_matches;
    use solana_ledger::{
        bank_forks::BankForks,
        blockstore::{make_slot_entries, Blockstore},
        get_tmp_ledger_path,
    };
    use solana_runtime::{
        bank::Bank,
        genesis_utils::{
            create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
        },
    };
    use solana_sdk::{
        clock::Slot,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, KeypairUtil},
        transaction::Transaction,
    };
    use solana_vote_program::{vote_instruction, vote_state::Vote};
    use std::{
        collections::{HashMap, VecDeque},
        fs::{remove_file, OpenOptions},
        io::{Read, Seek, SeekFrom},
        sync::RwLock,
        thread::sleep,
        time::Duration,
    };
    use tempfile::TempDir;
    use trees::{tr, Node, Tree};

    pub(crate) struct VoteSimulator<'a> {
        searchable_nodes: HashMap<u64, &'a Node<u64>>,
    }

    impl<'a> VoteSimulator<'a> {
        pub(crate) fn new(forks: &'a Tree<u64>) -> Self {
            let mut searchable_nodes = HashMap::new();
            let root = forks.root();
            searchable_nodes.insert(root.data, root);
            Self { searchable_nodes }
        }

        pub(crate) fn simulate_vote(
            &mut self,
            vote_slot: Slot,
            bank_forks: &RwLock<BankForks>,
            cluster_votes: &mut HashMap<Pubkey, Vec<u64>>,
            validator_keypairs: &HashMap<Pubkey, ValidatorVoteKeypairs>,
            my_keypairs: &ValidatorVoteKeypairs,
            progress: &mut HashMap<u64, ForkProgress>,
            tower: &mut Tower,
        ) -> VoteResult {
            let node = self
                .find_node_and_update_simulation(vote_slot)
                .expect("Vote to simulate must be for a slot in the tree");

            let mut missing_nodes = VecDeque::new();
            let mut current = node;
            loop {
                let current_slot = current.data;
                if bank_forks.read().unwrap().get(current_slot).is_some()
                    || tower.root().map(|r| current_slot < r).unwrap_or(false)
                {
                    break;
                } else {
                    missing_nodes.push_front(current);
                }

                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    break;
                }
            }

            // Create any missing banks along the path
            for missing_node in missing_nodes {
                let missing_slot = missing_node.data;
                let parent = missing_node.parent().unwrap().data;
                let parent_bank = bank_forks
                    .read()
                    .unwrap()
                    .get(parent)
                    .expect("parent bank must exist")
                    .clone();
                info!("parent of {} is {}", missing_slot, parent_bank.slot(),);
                progress
                    .entry(missing_slot)
                    .or_insert_with(|| ForkProgress::new(parent_bank.last_blockhash()));

                // Create the missing bank
                let new_bank =
                    Bank::new_from_parent(&parent_bank, &Pubkey::default(), missing_slot);

                // Simulate ingesting the cluster's votes for the parent into this bank
                for (pubkey, vote) in cluster_votes.iter() {
                    if vote.contains(&parent_bank.slot()) {
                        let keypairs = validator_keypairs.get(pubkey).unwrap();
                        let node_pubkey = keypairs.node_keypair.pubkey();
                        let vote_pubkey = keypairs.vote_keypair.pubkey();
                        let last_blockhash = parent_bank.last_blockhash();
                        let votes = Vote::new(vec![parent_bank.slot()], parent_bank.hash());
                        info!("voting {} {}", parent_bank.slot(), parent_bank.hash());
                        let vote_ix = vote_instruction::vote(&vote_pubkey, &vote_pubkey, votes);
                        let mut vote_tx =
                            Transaction::new_with_payer(vec![vote_ix], Some(&node_pubkey));
                        vote_tx.partial_sign(&[&keypairs.node_keypair], last_blockhash);
                        vote_tx.partial_sign(&[&keypairs.vote_keypair], last_blockhash);
                        new_bank.process_transaction(&vote_tx).unwrap();
                    }
                }
                new_bank.freeze();
                bank_forks.write().unwrap().insert(new_bank);
            }

            // Now try to simulate the vote
            let my_pubkey = my_keypairs.node_keypair.pubkey();
            let my_vote_pubkey = my_keypairs.vote_keypair.pubkey();
            let ancestors = bank_forks.read().unwrap().ancestors();
            let mut frozen_banks: Vec<_> = bank_forks
                .read()
                .unwrap()
                .frozen_banks()
                .values()
                .cloned()
                .collect();
            ReplayStage::compute_bank_stats(
                &my_pubkey,
                &ancestors,
                &mut frozen_banks,
                tower,
                progress,
            );
            ReplayStage::select_fork(&frozen_banks, tower, progress);

            let bank = bank_forks
                .read()
                .unwrap()
                .get(vote_slot)
                .expect("Bank must have been created before vote simulation")
                .clone();
            // Make sure this slot isn't locked out or failing threshold
            let fork_progress = progress
                .get(&vote_slot)
                .expect("Slot for vote must exist in progress map");
            info!("Checking vote: {}", vote_slot);
            info!("lockouts: {:?}", fork_progress.fork_stats.stake_lockouts);
            if fork_progress.fork_stats.is_locked_out && !fork_progress.fork_stats.vote_threshold {
                return VoteResult::FailedAllChecks(vote_slot);
            } else if fork_progress.fork_stats.is_locked_out {
                return VoteResult::LockedOut(vote_slot);
            } else if !fork_progress.fork_stats.vote_threshold {
                return VoteResult::FailedThreshold(vote_slot);
            }
            let vote = tower.new_vote_from_bank(&bank, &my_vote_pubkey).0;
            if let Some(new_root) = tower.record_bank_vote(vote) {
                ReplayStage::handle_new_root(new_root, bank_forks, progress, &None);
            }

            // Mark the vote for this bank under this node's pubkey so it will be
            // integrated into any future child banks
            cluster_votes.entry(my_pubkey).or_default().push(vote_slot);
            VoteResult::Ok
        }

        // Find a node representing the given slot
        fn find_node_and_update_simulation(&mut self, slot: u64) -> Option<&'a Node<u64>> {
            let mut successful_search_node: Option<&'a Node<u64>> = None;
            let mut found_node = None;
            for search_node in self.searchable_nodes.values() {
                if let Some((target, new_searchable_nodes)) = Self::find_node(search_node, slot) {
                    successful_search_node = Some(search_node);
                    found_node = Some(target);
                    for node in new_searchable_nodes {
                        self.searchable_nodes.insert(node.data, node);
                    }
                    break;
                }
            }
            successful_search_node.map(|node| {
                self.searchable_nodes.remove(&node.data);
            });
            found_node
        }

        fn find_node(
            node: &'a Node<u64>,
            slot: u64,
        ) -> Option<(&'a Node<u64>, Vec<&'a Node<u64>>)> {
            if node.data == slot {
                Some((node, node.iter().collect()))
            } else {
                let mut search_result: Option<(&'a Node<u64>, Vec<&'a Node<u64>>)> = None;
                for child in node.iter() {
                    if let Some((_, ref mut new_searchable_nodes)) = search_result {
                        new_searchable_nodes.push(child);
                        continue;
                    }
                    search_result = Self::find_node(child, slot);
                }

                search_result
            }
        }
    }

    #[derive(PartialEq, Debug)]
    pub(crate) enum VoteResult {
        LockedOut(u64),
        FailedThreshold(u64),
        FailedAllChecks(u64),
        Ok,
    }

    // Setup BankForks with bank 0 and all the validator accounts
    pub(crate) fn initialize_state(
        validator_keypairs_map: &HashMap<Pubkey, ValidatorVoteKeypairs>,
    ) -> (BankForks, HashMap<u64, ForkProgress>) {
        let validator_keypairs: Vec<_> = validator_keypairs_map.values().collect();
        let GenesisConfigInfo {
            genesis_config,
            mint_keypair,
            voting_keypair: _,
        } = create_genesis_config_with_vote_accounts(1_000_000_000, &validator_keypairs);

        let bank0 = Bank::new(&genesis_config);

        for pubkey in validator_keypairs_map.keys() {
            bank0.transfer(10_000, &mint_keypair, pubkey).unwrap();
        }

        bank0.freeze();
        let mut progress = HashMap::new();
        progress.insert(0, ForkProgress::new(bank0.last_blockhash()));
        (BankForks::new(0, bank0), progress)
    }

    fn gen_stakes(stake_votes: &[(u64, &[u64])]) -> Vec<(Pubkey, (u64, Account))> {
        let mut stakes = vec![];
        for (lamports, votes) in stake_votes {
            let mut account = Account::default();
            account.data = vec![0; VoteState::size_of()];
            account.lamports = *lamports;
            let mut vote_state = VoteState::default();
            for slot in *votes {
                vote_state.process_slot_vote_unchecked(*slot);
            }
            vote_state
                .serialize(&mut account.data)
                .expect("serialize state");
            stakes.push((Pubkey::new_rand(), (*lamports, account)));
        }
        stakes
    }

    fn can_progress_on_fork(
        my_pubkey: &Pubkey,
        tower: &mut Tower,
        start_slot: u64,
        num_slots: u64,
        bank_forks: &RwLock<BankForks>,
        cluster_votes: &mut HashMap<Pubkey, Vec<u64>>,
        keypairs: &HashMap<Pubkey, ValidatorVoteKeypairs>,
        progress: &mut HashMap<u64, ForkProgress>,
    ) -> bool {
        // Check that within some reasonable time, validator can make a new
        // root on this fork
        let old_root = tower.root();
        let mut main_fork = tr(start_slot);
        let mut tip = main_fork.root_mut();

        for i in 1..num_slots {
            tip.push_front(tr(start_slot + i));
            tip = tip.first_mut().unwrap();
        }
        let mut voting_simulator = VoteSimulator::new(&main_fork);
        for i in 1..num_slots {
            voting_simulator.simulate_vote(
                i + start_slot,
                &bank_forks,
                cluster_votes,
                &keypairs,
                keypairs.get(&my_pubkey).unwrap(),
                progress,
                tower,
            );
            if old_root != tower.root() {
                return true;
            }
        }

        false
    }

    #[test]
    fn test_simple_votes() {
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let stake_keypair = Keypair::new();
        let node_pubkey = node_keypair.pubkey();

        let mut keypairs = HashMap::new();
        keypairs.insert(
            node_pubkey,
            ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
        );

        // Initialize BankForks
        let (bank_forks, mut progress) = initialize_state(&keypairs);
        let bank_forks = RwLock::new(bank_forks);

        // Create the tree of banks
        let forks = tr(0) / (tr(1) / (tr(2) / (tr(3) / (tr(4) / tr(5)))));

        // Set the voting behavior
        let mut voting_simulator = VoteSimulator::new(&forks);
        let votes = vec![0, 1, 2, 3, 4, 5];

        // Simulate the votes
        let mut tower = Tower::new_with_key(&node_pubkey);

        let mut cluster_votes = HashMap::new();
        for vote in votes {
            assert_eq!(
                VoteResult::Ok,
                voting_simulator.simulate_vote(
                    vote,
                    &bank_forks,
                    &mut cluster_votes,
                    &keypairs,
                    keypairs.get(&node_pubkey).unwrap(),
                    &mut progress,
                    &mut tower,
                )
            );
        }

        for i in 0..5 {
            assert_eq!(tower.lockouts.votes[i].slot as usize, i);
            assert_eq!(tower.lockouts.votes[i].confirmation_count as usize, 6 - i);
        }
    }

    #[test]
    fn test_double_partition() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let stake_keypair = Keypair::new();
        let node_pubkey = node_keypair.pubkey();
        let vote_pubkey = vote_keypair.pubkey();

        let mut keypairs = HashMap::new();
        info!("my_pubkey: {}", node_pubkey);
        keypairs.insert(
            node_pubkey,
            ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
        );

        // Create the tree of banks in a BankForks object
        let forks = tr(0)
            / (tr(1)
                / (tr(2)
                    / (tr(3)
                        / (tr(4)
                            / (tr(5)
                                / (tr(6)
                                    / (tr(7)
                                        / (tr(8)
                                            / (tr(9)
                                                // Minor fork 1
                                                / (tr(10) / (tr(11) / (tr(12) / (tr(13) / (tr(14))))))
                                                / (tr(43)
                                                    / (tr(44)
                                                        // Minor fork 2
                                                        / (tr(45) / (tr(46) / (tr(47) / (tr(48) / (tr(49) / (tr(50)))))))
                                                        / (tr(110)))))))))))));

        // Set the voting behavior
        let mut voting_simulator = VoteSimulator::new(&forks);
        let mut votes: Vec<Slot> = vec![];
        // Vote on the first minor fork
        votes.extend((0..=14).into_iter());
        // Come back to the main fork
        votes.extend((43..=44).into_iter());
        // Vote on the second minor fork
        votes.extend((45..=50).into_iter());

        let mut cluster_votes: HashMap<Pubkey, Vec<Slot>> = HashMap::new();
        let (bank_forks, mut progress) = initialize_state(&keypairs);
        let bank_forks = RwLock::new(bank_forks);

        // Simulate the votes. Should fail on trying to come back to the main fork
        // at 106 exclusively due to threshold failure
        let mut tower = Tower::new_with_key(&node_pubkey);
        for vote in &votes {
            // All these votes should be ok
            assert_eq!(
                voting_simulator.simulate_vote(
                    *vote,
                    &bank_forks,
                    &mut cluster_votes,
                    &keypairs,
                    keypairs.get(&node_pubkey).unwrap(),
                    &mut progress,
                    &mut tower,
                ),
                VoteResult::Ok
            );
        }

        // Try to come back to main fork
        let next_unlocked_slot = 110;
        assert_eq!(
            voting_simulator.simulate_vote(
                next_unlocked_slot,
                &bank_forks,
                &mut cluster_votes,
                &keypairs,
                keypairs.get(&node_pubkey).unwrap(),
                &mut progress,
                &mut tower,
            ),
            VoteResult::Ok
        );

        info!("local tower: {:#?}", tower.lockouts.votes);
        let vote_accounts = bank_forks
            .read()
            .unwrap()
            .get(next_unlocked_slot)
            .unwrap()
            .vote_accounts();
        let observed = vote_accounts.get(&vote_pubkey).unwrap();
        let state = VoteState::from(&observed.1).unwrap();
        info!("observed tower: {:#?}", state.votes);

        assert!(can_progress_on_fork(
            &node_pubkey,
            &mut tower,
            next_unlocked_slot,
            200,
            &bank_forks,
            &mut cluster_votes,
            &keypairs,
            &mut progress
        ));
    }

    #[test]
    fn test_collect_vote_lockouts_sums() {
        //two accounts voting for slot 0 with 1 token staked
        let accounts = gen_stakes(&[(1, &[0]), (1, &[0])]);
        let tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        let (staked_lockouts, total_staked, bank_weight) =
            tower.collect_vote_lockouts(1, accounts.into_iter(), &ancestors);
        assert_eq!(staked_lockouts[&0].stake, 2);
        assert_eq!(staked_lockouts[&0].lockout, 2 + 2 + 4 + 4);
        assert_eq!(total_staked, 2);

        // Each acccount has 1 vote in it. After simulating a vote in collect_vote_lockouts,
        // the account will have 2 votes, with lockout 2 + 4 = 6. So expected weight for
        // two acccounts is 2 * 6 = 12
        assert_eq!(bank_weight, 12)
    }

    #[test]
    fn test_collect_vote_lockouts_root() {
        let votes: Vec<u64> = (0..MAX_LOCKOUT_HISTORY as u64).into_iter().collect();
        //two accounts voting for slots 0..MAX_LOCKOUT_HISTORY with 1 token staked
        let accounts = gen_stakes(&[(1, &votes), (1, &votes)]);
        let mut tower = Tower::new_for_tests(0, 0.67);
        let mut ancestors = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY + 1) {
            tower.record_vote(i as u64, Hash::default());
            ancestors.insert(i as u64, (0..i as u64).into_iter().collect());
        }
        let root = Lockout {
            confirmation_count: MAX_LOCKOUT_HISTORY as u32,
            slot: 0,
        };
        let root_weight = root.lockout() as u128;
        let vote_account_expected_weight = tower
            .lockouts
            .votes
            .iter()
            .map(|v| v.lockout() as u128)
            .sum::<u128>()
            + root_weight;
        let expected_bank_weight = 2 * vote_account_expected_weight;
        assert_eq!(tower.lockouts.root_slot, Some(0));
        let (staked_lockouts, _total_staked, bank_weight) = tower.collect_vote_lockouts(
            MAX_LOCKOUT_HISTORY as u64,
            accounts.into_iter(),
            &ancestors,
        );
        for i in 0..MAX_LOCKOUT_HISTORY {
            assert_eq!(staked_lockouts[&(i as u64)].stake, 2);
        }
        // should be the sum of all the weights for root
        assert!(staked_lockouts[&0].lockout > (2 * (1 << MAX_LOCKOUT_HISTORY)));
        assert_eq!(bank_weight, expected_bank_weight);
    }

    #[test]
    fn test_check_vote_threshold_without_votes() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(tower.check_vote_stake_threshold(0, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_no_skip_lockout_with_new_root() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(4, 0.67);
        let mut stakes = HashMap::new();
        for i in 0..(MAX_LOCKOUT_HISTORY as u64 + 1) {
            stakes.insert(
                i,
                StakeLockout {
                    stake: 1,
                    lockout: 8,
                },
            );
            tower.record_vote(i, Hash::default());
        }
        assert!(!tower.check_vote_stake_threshold(MAX_LOCKOUT_HISTORY as u64 + 1, &stakes, 2));
    }

    #[test]
    fn test_is_slot_confirmed_not_enough_stake_failure() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(!tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_slot_confirmed_unknown_slot() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = HashMap::new();
        assert!(!tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_slot_confirmed_pass() {
        let tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        assert!(tower.is_slot_confirmed(0, &stakes, 2));
    }

    #[test]
    fn test_is_locked_out_empty() {
        let tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(0, HashSet::new())].into_iter().collect();
        assert!(!tower.is_locked_out(0, &ancestors));
    }

    #[test]
    fn test_is_locked_out_root_slot_child_pass() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect())]
            .into_iter()
            .collect();
        tower.lockouts.root_slot = Some(0);
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_root_slot_sibling_fail() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(2, vec![0].into_iter().collect())]
            .into_iter()
            .collect();
        tower.lockouts.root_slot = Some(0);
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(2, &ancestors));
    }

    #[test]
    fn test_check_already_voted() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        tower.record_vote(0, Hash::default());
        assert!(tower.has_voted(0));
        assert!(!tower.has_voted(1));
    }

    #[test]
    fn test_check_recent_slot() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        assert!(tower.is_recent(0));
        assert!(tower.is_recent(32));
        for i in 0..64 {
            tower.record_vote(i, Hash::default());
        }
        assert!(!tower.is_recent(0));
        assert!(!tower.is_recent(32));
        assert!(!tower.is_recent(63));
        assert!(tower.is_recent(65));
    }

    #[test]
    fn test_is_locked_out_double_vote() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect()), (0, HashSet::new())]
            .into_iter()
            .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(0, &ancestors));
    }

    #[test]
    fn test_is_locked_out_child() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![(1, vec![0].into_iter().collect())]
            .into_iter()
            .collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.is_locked_out(1, &ancestors));
    }

    #[test]
    fn test_is_locked_out_sibling() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![
            (0, HashSet::new()),
            (1, vec![0].into_iter().collect()),
            (2, vec![0].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(tower.is_locked_out(2, &ancestors));
    }

    #[test]
    fn test_is_locked_out_last_vote_expired() {
        let mut tower = Tower::new_for_tests(0, 0.67);
        let ancestors = vec![
            (0, HashSet::new()),
            (1, vec![0].into_iter().collect()),
            (4, vec![0].into_iter().collect()),
        ]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        assert!(!tower.is_locked_out(4, &ancestors));
        tower.record_vote(4, Hash::default());
        assert_eq!(tower.lockouts.votes[0].slot, 0);
        assert_eq!(tower.lockouts.votes[0].confirmation_count, 2);
        assert_eq!(tower.lockouts.votes[1].slot, 4);
        assert_eq!(tower.lockouts.votes[1].confirmation_count, 1);
    }

    #[test]
    fn test_check_vote_threshold_below_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 1,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2));
    }
    #[test]
    fn test_check_vote_threshold_above_threshold() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        assert!(tower.check_vote_stake_threshold(1, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_after_pop() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![(
            0,
            StakeLockout {
                stake: 2,
                lockout: 8,
            },
        )]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_above_threshold_no_stake() {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = HashMap::new();
        tower.record_vote(0, Hash::default());
        assert!(!tower.check_vote_stake_threshold(1, &stakes, 2));
    }

    #[test]
    fn test_check_vote_threshold_lockouts_not_updated() {
        solana_logger::setup();
        let mut tower = Tower::new_for_tests(1, 0.67);
        let stakes = vec![
            (
                0,
                StakeLockout {
                    stake: 1,
                    lockout: 8,
                },
            ),
            (
                1,
                StakeLockout {
                    stake: 2,
                    lockout: 8,
                },
            ),
        ]
        .into_iter()
        .collect();
        tower.record_vote(0, Hash::default());
        tower.record_vote(1, Hash::default());
        tower.record_vote(2, Hash::default());
        assert!(tower.check_vote_stake_threshold(6, &stakes, 2));
    }

    #[test]
    fn test_lockout_is_updated_for_entire_branch() {
        let mut stake_lockouts = HashMap::new();
        let vote = Lockout {
            slot: 2,
            confirmation_count: 1,
        };
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let mut ancestors = HashMap::new();
        ancestors.insert(2, set);
        let set: HashSet<u64> = vec![0u64].into_iter().collect();
        ancestors.insert(1, set);
        Tower::update_ancestor_lockouts(&mut stake_lockouts, &vote, &ancestors);
        assert_eq!(stake_lockouts[&0].lockout, 2);
        assert_eq!(stake_lockouts[&1].lockout, 2);
        assert_eq!(stake_lockouts[&2].lockout, 2);
    }

    #[test]
    fn test_lockout_is_updated_for_slot_or_lower() {
        let mut stake_lockouts = HashMap::new();
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let mut ancestors = HashMap::new();
        ancestors.insert(2, set);
        let set: HashSet<u64> = vec![0u64].into_iter().collect();
        ancestors.insert(1, set);
        let vote = Lockout {
            slot: 2,
            confirmation_count: 1,
        };
        Tower::update_ancestor_lockouts(&mut stake_lockouts, &vote, &ancestors);
        let vote = Lockout {
            slot: 1,
            confirmation_count: 2,
        };
        Tower::update_ancestor_lockouts(&mut stake_lockouts, &vote, &ancestors);
        assert_eq!(stake_lockouts[&0].lockout, 2 + 4);
        assert_eq!(stake_lockouts[&1].lockout, 2 + 4);
        assert_eq!(stake_lockouts[&2].lockout, 2);
    }

    #[test]
    fn test_stake_is_updated_for_entire_branch() {
        let mut stake_lockouts = HashMap::new();
        let mut account = Account::default();
        account.lamports = 1;
        let set: HashSet<u64> = vec![0u64, 1u64].into_iter().collect();
        let ancestors: HashMap<u64, HashSet<u64>> = [(2u64, set)].iter().cloned().collect();
        Tower::update_ancestor_stakes(&mut stake_lockouts, 2, account.lamports, &ancestors);
        assert_eq!(stake_lockouts[&0].stake, 1);
        assert_eq!(stake_lockouts[&1].stake, 1);
        assert_eq!(stake_lockouts[&2].stake, 1);
    }

    #[test]
    fn test_new_vote() {
        let local = VoteState::default();
        let vote = Tower::new_vote(&local, 0, Hash::default(), None);
        assert_eq!(local.votes.len(), 0);
        assert_eq!(vote.0.slots, vec![0]);
        assert_eq!(vote.1, 0);
    }

    #[test]
    fn test_new_vote_dup_vote() {
        let local = VoteState::default();
        let vote = Tower::new_vote(&local, 0, Hash::default(), Some(0));
        assert!(vote.0.slots.is_empty());
    }

    #[test]
    fn test_new_vote_next_vote() {
        let mut local = VoteState::default();
        let vote = Vote {
            slots: vec![0],
            hash: Hash::default(),
            timestamp: None,
        };
        local.process_vote_unchecked(&vote);
        assert_eq!(local.votes.len(), 1);
        let vote = Tower::new_vote(&local, 1, Hash::default(), Some(0));
        assert_eq!(vote.0.slots, vec![1]);
        assert_eq!(vote.1, 1);
    }

    #[test]
    fn test_new_vote_next_after_expired_vote() {
        let mut local = VoteState::default();
        let vote = Vote {
            slots: vec![0],
            hash: Hash::default(),
            timestamp: None,
        };
        local.process_vote_unchecked(&vote);
        assert_eq!(local.votes.len(), 1);
        let vote = Tower::new_vote(&local, 3, Hash::default(), Some(0));
        //first vote expired, so index should be 0
        assert_eq!(vote.0.slots, vec![3]);
        assert_eq!(vote.1, 0);
    }

    #[test]
    fn test_check_vote_threshold_forks() {
        // Create the ancestor relationships
        let ancestors = (0..=(VOTE_THRESHOLD_DEPTH + 1) as u64)
            .map(|slot| {
                let slot_parents: HashSet<_> = (0..slot).collect();
                (slot, slot_parents)
            })
            .collect();

        // Create votes such that
        // 1) 3/4 of the stake has voted on slot: VOTE_THRESHOLD_DEPTH - 2, lockout: 2
        // 2) 1/4 of the stake has voted on slot: VOTE_THRESHOLD_DEPTH, lockout: 2^9
        let total_stake = 4;
        let threshold_size = 0.67;
        let threshold_stake = (f64::ceil(total_stake as f64 * threshold_size)) as u64;
        let tower_votes: Vec<u64> = (0..VOTE_THRESHOLD_DEPTH as u64).collect();
        let accounts = gen_stakes(&[
            (threshold_stake, &[(VOTE_THRESHOLD_DEPTH - 2) as u64]),
            (total_stake - threshold_stake, &tower_votes[..]),
        ]);

        // Initialize tower
        let mut tower = Tower::new_for_tests(VOTE_THRESHOLD_DEPTH, threshold_size);

        // CASE 1: Record the first VOTE_THRESHOLD tower votes for fork 2. We want to
        // evaluate a vote on slot VOTE_THRESHOLD_DEPTH. The nth most recent vote should be
        // for slot 0, which is common to all account vote states, so we should pass the
        // threshold check
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64;
        for vote in &tower_votes {
            tower.record_vote(*vote, Hash::default());
        }
        let (staked_lockouts, total_staked, _) =
            tower.collect_vote_lockouts(vote_to_evaluate, accounts.clone().into_iter(), &ancestors);
        assert!(tower.check_vote_stake_threshold(vote_to_evaluate, &staked_lockouts, total_staked));

        // CASE 2: Now we want to evaluate a vote for slot VOTE_THRESHOLD_DEPTH + 1. This slot
        // will expire the vote in one of the vote accounts, so we should have insufficient
        // stake to pass the threshold
        let vote_to_evaluate = VOTE_THRESHOLD_DEPTH as u64 + 1;
        let (staked_lockouts, total_staked, _) =
            tower.collect_vote_lockouts(vote_to_evaluate, accounts.into_iter(), &ancestors);
        assert!(!tower.check_vote_stake_threshold(
            vote_to_evaluate,
            &staked_lockouts,
            total_staked
        ));
    }

    fn vote_and_check_recent(num_votes: usize) {
        let mut tower = Tower::new_for_tests(1, 0.67);
        let slots = if num_votes > 0 {
            vec![num_votes as u64 - 1]
        } else {
            vec![]
        };
        let expected = Vote::new(slots, Hash::default());
        for i in 0..num_votes {
            tower.record_vote(i as u64, Hash::default());
        }
        assert_eq!(expected, tower.last_vote())
    }

    #[test]
    fn test_recent_votes_full() {
        vote_and_check_recent(MAX_LOCKOUT_HISTORY)
    }

    #[test]
    fn test_recent_votes_empty() {
        vote_and_check_recent(0)
    }

    #[test]
    fn test_recent_votes_exact() {
        vote_and_check_recent(5)
    }

    #[test]
    fn test_maybe_timestamp() {
        let mut tower = Tower::default();
        assert!(tower.maybe_timestamp(TIMESTAMP_SLOT_INTERVAL).is_some());
        let BlockTimestamp { slot, timestamp } = tower.last_timestamp;

        assert_eq!(tower.maybe_timestamp(1), None);
        assert_eq!(tower.maybe_timestamp(slot), None);
        assert_eq!(tower.maybe_timestamp(slot + 1), None);

        sleep(Duration::from_secs(1));
        assert!(tower
            .maybe_timestamp(slot + TIMESTAMP_SLOT_INTERVAL + 1)
            .is_some());
        assert!(tower.last_timestamp.timestamp > timestamp);
    }

    fn run_test_load_tower_snapshot<F, G>(
        modify_original: F,
        modify_serialized: G,
    ) -> (Tower, Result<Tower>)
    where
        F: Fn(&mut Tower, &Pubkey) -> (),
        G: Fn(&PathBuf) -> (),
    {
        let dir = TempDir::new().unwrap();
        // Use values that will not match the default derived from BankFroks
        let mut tower = Tower::new_for_tests(10, 0.9);
        let my_keypair = Keypair::new();
        modify_original(&mut tower, &my_keypair.pubkey());
        let vote_keypair = Keypair::new();

        tower
            .save_to_file(&dir.path().to_path_buf(), &vote_keypair)
            .unwrap();
        modify_serialized(&dir.path().to_path_buf());
        let loaded = Tower::reload_from_file(
            &dir.path().to_path_buf(),
            &my_keypair.pubkey(),
            &vote_keypair.pubkey(),
        );

        (tower, loaded)
    }

    #[test]
    fn test_load_tower_snapshot_good() {
        let (tower, loaded) =
            run_test_load_tower_snapshot(|tower, pubkey| tower.node_pubkey = *pubkey, |_| ());
        assert_eq!(loaded.unwrap(), tower)
    }

    #[test]
    fn test_load_tower_snapshot_wrong_owner() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, _| tower.node_pubkey = Keypair::new().pubkey(),
            |_| (),
        );
        assert_matches!(loaded, Err(TowerError::WrongPubkey))
    }

    #[test]
    fn test_load_tower_snapshot_invalid_signature() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, pubkey| tower.node_pubkey = *pubkey,
            |path| {
                let mut file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(path.join(TOWER_SNAPSHOT_NAME))
                    .unwrap();
                let mut buf = [0u8];
                assert_eq!(file.read(&mut buf).unwrap(), 1);
                buf[0] = buf[0] + 1;
                assert_eq!(file.seek(SeekFrom::Start(0)).unwrap(), 0);
                assert_eq!(file.write(&buf).unwrap(), 1);
            },
        );
        assert_matches!(loaded, Err(TowerError::InvalidSignature))
    }

    #[test]
    fn test_load_tower_snapshot_deser_failure() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, pubkey| tower.node_pubkey = *pubkey,
            |path| {
                let path = path.join(TOWER_SNAPSHOT_NAME);
                OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(&path)
                    .expect(&format!("Failed to truncate file: {:?}", path));
            },
        );
        assert_matches!(loaded, Err(TowerError::SerializeError(_)))
    }

    #[test]
    fn test_load_tower_snapshot_missing() {
        let (_, loaded) = run_test_load_tower_snapshot(
            |tower, pubkey| tower.node_pubkey = *pubkey,
            |path| {
                remove_file(path.join(TOWER_SNAPSHOT_NAME)).unwrap();
            },
        );
        assert_matches!(loaded, Err(TowerError::IOError(_)))
    }

    fn build_tree(n: u64) -> Tree<u64> {
        _build_tree(n - 1, n - 1)
    }
    fn _build_tree(n: u64, i: u64) -> Tree<u64> {
        if i > 0 {
            tr(n - i) / _build_tree(n, i - 1)
        } else {
            tr(n)
        }
    }

    #[test]
    fn test_reconcile_blockstore_roots_with_tower() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();
            let node_keypair = Keypair::new();
            let vote_keypair = Keypair::new();
            let stake_keypair = Keypair::new();
            let node_pubkey = node_keypair.pubkey();
            let mut keypairs = HashMap::new();
            keypairs.insert(
                node_pubkey,
                ValidatorVoteKeypairs::new(node_keypair, vote_keypair, stake_keypair),
            );

            let (bank_forks, mut progress) = initialize_state(&keypairs);
            let bank_forks = Arc::new(RwLock::new(bank_forks));
            let mut tower = Tower::new_with_key(&node_pubkey);

            // Create the tree of banks in a BankForks object
            let forks = build_tree(42);

            let mut voting_simulator = VoteSimulator::new(&forks);
            let mut cluster_votes: HashMap<Pubkey, Vec<Slot>> = HashMap::new();
            let votes: Vec<Slot> = (0..42).collect();

            for slot in &votes {
                let slot = *slot;
                let (shreds, _) = make_slot_entries(slot + 1, slot, 1);
                blockstore.insert_shreds(shreds, None, false).unwrap();
                assert_eq!(
                    voting_simulator.simulate_vote(
                        slot,
                        &bank_forks,
                        &mut cluster_votes,
                        &keypairs,
                        keypairs.get(&node_pubkey).unwrap(),
                        &mut progress,
                        &mut tower,
                    ),
                    VoteResult::Ok
                );
            }
            let tower_root = tower.root().unwrap();
            let set_roots: Vec<_> = (0..(tower_root / 2)).collect();
            blockstore.set_roots(&set_roots).unwrap();
            assert!(blockstore.last_root() < tower_root);
            assert!(reconcile_blockstore_roots_with_tower(&tower, &blockstore).is_ok());
            assert_eq!(blockstore.last_root(), tower.root().unwrap());
            assert!(reconcile_blockstore_roots_with_tower(&tower, &blockstore).is_ok());
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }
}
