use crate::checkpoint::Checkpoint;
use crate::poh_service::NUM_TICKS_PER_SECOND;
use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Signature;
use solana_sdk::timing::timestamp;
use std::collections::VecDeque;
use std::result;

/// The number of most recent `last_id` values that the bank will track the signatures
/// of. Once the bank discards a `last_id`, it will reject any transactions that use
/// that `last_id` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `last_id` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_ENTRY_IDS: usize = NUM_TICKS_PER_SECOND * 120;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Status<T> {
    Reserved,
    Complete(T),
}

type StatusMap<T> = HashMap<Signature, Status<T>>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum StatusDequeError {
    /// The `Signature` has been seen before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `last_id`, or as a double-spend attack.
    DuplicateSignature,

    /// The bank has not seen the given `last_id` or the transaction is too old and
    /// the `last_id` has been discarded.
    LastIdNotFound,
}

pub type Result<T> = result::Result<T, StatusDequeError>;

/// a record of a tick, from register_tick
#[derive(Clone)]
struct StatusEntry<T> {
    /// when the id was registered, according to network time
    tick_height: u64,

    /// timestamp when this id was registered, used for stats/finality
    timestamp: u64,

    /// a map of signature status, used for duplicate detection
    statuses: StatusMap<T>,
}

pub struct StatusDeque<T> {
    /// A FIFO queue of `last_id` items, where each item is a set of signatures
    /// that have been processed using that `last_id`. Rejected `last_id`
    /// values are so old that the `last_id` has been pulled out of the queue.

    /// updated whenever an id is registered, at each tick ;)
    pub tick_height: u64,

    /// last tick to be registered
    pub last_id: Option<Hash>,

    /// Mapping of hashes to signature sets along with timestamp and what tick_height
    /// was when the id was added. The bank uses this data to
    /// reject transactions with signatures it's seen before and to reject
    /// transactions that are too old (nth is too small)
    entries: HashMap<Hash, StatusEntry<T>>,

    checkpoints: VecDeque<(u64, Option<Hash>, HashMap<Hash, StatusEntry<T>>)>,
}

impl<T> Default for StatusDeque<T> {
    fn default() -> Self {
        Self {
            tick_height: 0,
            last_id: None,
            entries: HashMap::new(),
            checkpoints: VecDeque::new(),
        }
    }
}

impl<T: Clone> Checkpoint for StatusDeque<T> {
    fn checkpoint(&mut self) {
        self.checkpoints
            .push_front((self.tick_height, self.last_id, self.entries.clone()));
    }
    fn rollback(&mut self) {
        let (tick_height, last_id, entries) = self.checkpoints.pop_front().unwrap();
        self.tick_height = tick_height;
        self.last_id = last_id;
        self.entries = entries;
    }
    fn purge(&mut self, depth: usize) {
        while self.depth() > depth {
            self.checkpoints.pop_back().unwrap();
        }
    }
    fn depth(&self) -> usize {
        self.checkpoints.len()
    }
}

impl<T: Clone> StatusDeque<T> {
    pub fn update_signature_status_with_last_id(
        &mut self,
        signature: &Signature,
        result: &T,
        last_id: &Hash,
    ) {
        if let Some(entry) = self.entries.get_mut(last_id) {
            entry
                .statuses
                .insert(*signature, Status::Complete(result.clone()));
        }
    }
    pub fn reserve_signature_with_last_id(
        &mut self,
        last_id: &Hash,
        sig: &Signature,
    ) -> Result<()> {
        if let Some(entry) = self.entries.get_mut(last_id) {
            if self.tick_height - entry.tick_height < MAX_ENTRY_IDS as u64 {
                return Self::reserve_signature(&mut entry.statuses, sig);
            }
        }
        Err(StatusDequeError::LastIdNotFound)
    }

    /// Store the given signature. The bank will reject any transaction with the same signature.
    fn reserve_signature(statuses: &mut StatusMap<T>, signature: &Signature) -> Result<()> {
        if let Some(_result) = statuses.get(signature) {
            return Err(StatusDequeError::DuplicateSignature);
        }
        statuses.insert(*signature, Status::Reserved);
        Ok(())
    }

    /// Forget all signatures. Useful for benchmarking.
    pub fn clear_signatures(&mut self) {
        for entry in &mut self.entries.values_mut() {
            entry.statuses.clear();
        }
    }

    /// Check if the age of the entry_id is within the max_age
    /// return false for any entries with an age equal to or above max_age
    pub fn check_entry_id_age(&self, entry_id: Hash, max_age: usize) -> bool {
        let entry = self.entries.get(&entry_id);

        match entry {
            Some(entry) => self.tick_height - entry.tick_height < max_age as u64,
            _ => false,
        }
    }
    /// Tell the bank which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// bank will reject transactions using that `last_id`.
    pub fn register_tick(&mut self, last_id: &Hash) {
        self.tick_height += 1;
        let tick_height = self.tick_height;

        // this clean up can be deferred until sigs gets larger
        //  because we verify entry.nth every place we check for validity
        if self.entries.len() >= MAX_ENTRY_IDS as usize {
            self.entries
                .retain(|_, entry| tick_height - entry.tick_height <= MAX_ENTRY_IDS as u64);
        }

        self.entries.insert(
            *last_id,
            StatusEntry {
                tick_height,
                timestamp: timestamp(),
                statuses: HashMap::new(),
            },
        );

        self.last_id = Some(*last_id);
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved finality
    pub fn get_finality_timestamp(
        &self,
        ticks_and_stakes: &mut [(u64, u64)],
        supermajority_stake: u64,
    ) -> Option<u64> {
        // Sort by tick height
        ticks_and_stakes.sort_by(|a, b| a.0.cmp(&b.0));
        let current_tick_height = self.tick_height;
        let mut total = 0;
        for (tick_height, stake) in ticks_and_stakes.iter() {
            if ((current_tick_height - tick_height) as usize) < MAX_ENTRY_IDS {
                total += stake;
                if total > supermajority_stake {
                    return self.tick_height_to_timestamp(*tick_height);
                }
            }
        }
        None
    }

    /// Maps a tick height to a timestamp
    fn tick_height_to_timestamp(&self, tick_height: u64) -> Option<u64> {
        for entry in self.entries.values() {
            if entry.tick_height == tick_height {
                return Some(entry.timestamp);
            }
        }
        None
    }

    /// Look through the last_ids and find all the valid ids
    /// This is batched to avoid holding the lock for a significant amount of time
    ///
    /// Return a vec of tuple of (valid index, timestamp)
    /// index is into the passed ids slice to avoid copying hashes
    pub fn count_valid_ids(&self, ids: &[Hash]) -> Vec<(usize, u64)> {
        let mut ret = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if let Some(entry) = self.entries.get(id) {
                if self.tick_height - entry.tick_height < MAX_ENTRY_IDS as u64 {
                    ret.push((i, entry.timestamp));
                }
            }
        }
        ret
    }
    pub fn get_signature_status(&self, signature: &Signature) -> Option<Status<T>> {
        for entry in self.entries.values() {
            if let Some(res) = entry.statuses.get(signature) {
                return Some(res.clone());
            }
        }
        None
    }
    pub fn has_signature(&self, signature: &Signature) -> bool {
        self.get_signature_status(signature).is_some()
    }

    pub fn get_signature(&self, last_id: &Hash, signature: &Signature) -> Option<Status<T>> {
        self.entries
            .get(last_id)
            .and_then(|entry| entry.statuses.get(signature).cloned())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::hash::hash;
    #[test]
    fn test_duplicate_transaction_signature() {
        let sig = Default::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&last_id);
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &sig),
            Ok(())
        );
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &sig),
            Err(StatusDequeError::DuplicateSignature)
        );
    }

    #[test]
    fn test_duplicate_transaction_signature_checkpoint() {
        let sig = Default::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&last_id);
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &sig),
            Ok(())
        );
        status_deque.checkpoint();
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &sig),
            Err(StatusDequeError::DuplicateSignature)
        );
    }

    #[test]
    fn test_count_valid_ids() {
        let first_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&first_id);
        let ids: Vec<_> = (0..MAX_ENTRY_IDS)
            .map(|i| {
                let last_id = hash(&serialize(&i).unwrap()); // Unique hash
                status_deque.register_tick(&last_id);
                last_id
            })
            .collect();
        assert_eq!(status_deque.count_valid_ids(&[]).len(), 0);
        assert_eq!(status_deque.count_valid_ids(&[first_id]).len(), 0);
        for (i, id) in status_deque.count_valid_ids(&ids).iter().enumerate() {
            assert_eq!(id.0, i);
        }
    }

    #[test]
    fn test_clear_signatures() {
        let signature = Signature::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&last_id);
        status_deque
            .reserve_signature_with_last_id(&last_id, &signature)
            .unwrap();
        status_deque.clear_signatures();
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &signature),
            Ok(())
        );
    }

    #[test]
    fn test_clear_signatures_checkpoint() {
        let signature = Signature::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&last_id);
        status_deque
            .reserve_signature_with_last_id(&last_id, &signature)
            .unwrap();
        status_deque.checkpoint();
        status_deque.clear_signatures();
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &signature),
            Ok(())
        );
    }

    #[test]
    fn test_get_signature_status() {
        let signature = Signature::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&last_id);
        status_deque
            .reserve_signature_with_last_id(&last_id, &signature)
            .expect("reserve signature");
        assert_eq!(
            status_deque.get_signature_status(&signature),
            Some(Status::Reserved)
        );
    }

    #[test]
    fn test_register_tick() {
        let signature = Signature::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &signature),
            Err(StatusDequeError::LastIdNotFound)
        );
        status_deque.register_tick(&last_id);
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &signature),
            Ok(())
        );
    }

    #[test]
    fn test_has_signature() {
        let signature = Signature::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        status_deque.register_tick(&last_id);
        status_deque
            .reserve_signature_with_last_id(&last_id, &signature)
            .expect("reserve signature");
        assert!(status_deque.has_signature(&signature));
    }

    #[test]
    fn test_reject_old_last_id() {
        let signature = Signature::default();
        let last_id = Default::default();
        let mut status_deque: StatusDeque<()> = StatusDeque::default();
        for i in 0..MAX_ENTRY_IDS {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            status_deque.register_tick(&last_id);
        }
        // Assert we're no longer able to use the oldest entry ID.
        assert_eq!(
            status_deque.reserve_signature_with_last_id(&last_id, &signature),
            Err(StatusDequeError::LastIdNotFound)
        );
    }
}
