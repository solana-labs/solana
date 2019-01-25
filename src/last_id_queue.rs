use crate::poh_service::NUM_TICKS_PER_SECOND;
use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::timing::timestamp;

/// The number of most recent `last_id` values that the bank will track the signatures
/// of. Once the bank discards a `last_id`, it will reject any transactions that use
/// that `last_id` in a transaction. Lowering this value reduces memory consumption,
/// but requires clients to update its `last_id` more frequently. Raising the value
/// lengthens the time a client must wait to be certain a missing transaction will
/// not be processed by the network.
pub const MAX_ENTRY_IDS: usize = NUM_TICKS_PER_SECOND * 120;

#[derive(Debug, PartialEq, Eq, Clone)]
struct LastIdEntry {
    timestamp: u64,
    tick_height: u64,
}

/// Low memory overhead, so can be cloned for every checkpoint
#[derive(Clone)]
pub struct LastIdQueue {
    /// updated whenever an id is registered, at each tick ;)
    pub tick_height: u64,

    /// last tick to be registered
    pub last_id: Option<Hash>,

    entries: HashMap<Hash, LastIdEntry>,
}
impl Default for LastIdQueue {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            tick_height: 0,
            last_id: None,
        }
    }
}

impl LastIdQueue {
    /// Check if the age of the entry_id is within the max_age
    /// return false for any entries with an age equal to or above max_age
    pub fn check_entry_id_age(&self, entry_id: Hash, max_age: usize) -> bool {
        let entry = self.entries.get(&entry_id);
        match entry {
            Some(entry) => self.tick_height - entry.tick_height < max_age as u64,
            _ => false,
        }
    }
    /// check if entry is valid
    pub fn check_entry(&self, entry_id: Hash) -> bool {
        self.entries.get(&entry_id).is_some()
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
            LastIdEntry {
                tick_height,
                timestamp: timestamp(),
            },
        );

        self.last_id = Some(*last_id);
    }

    /// Looks through a list of tick heights and stakes, and finds the latest
    /// tick that has achieved confirmation
    pub fn get_confirmation_timestamp(
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
    pub fn clear(&mut self) {
        self.entries = HashMap::new();
        self.tick_height = 0;
        self.last_id = None;
    }
    /// fork for LastIdQueue is a simple clone
    pub fn fork(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            tick_height: self.tick_height.clone(),
            last_id: self.last_id.clone(),
        }
    }
    /// merge for entryq is a swap
    pub fn merge_into_root(&mut self, other: Self) {
        let (entries, tick_height, last_id) = { (other.entries, other.tick_height, other.last_id) };
        self.entries = entries;
        self.tick_height = tick_height;
        self.last_id = last_id;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::hash::hash;

    #[test]
    fn test_count_valid_ids() {
        let first_id = Hash::default();
        let mut entry_queue = LastIdQueue::default();
        entry_queue.register_tick(&first_id);
        let ids: Vec<_> = (0..MAX_ENTRY_IDS)
            .map(|i| {
                let last_id = hash(&serialize(&i).unwrap()); // Unique hash
                entry_queue.register_tick(&last_id);
                last_id
            })
            .collect();
        assert_eq!(entry_queue.count_valid_ids(&[]).len(), 0);
        assert_eq!(entry_queue.count_valid_ids(&[first_id]).len(), 0);
        for (i, id) in entry_queue.count_valid_ids(&ids).iter().enumerate() {
            assert_eq!(id.0, i);
        }
    }

    #[test]
    fn test_register_tick() {
        let last_id = Hash::default();
        let mut entry_queue = LastIdQueue::default();
        assert!(!entry_queue.check_entry(last_id));
        entry_queue.register_tick(&last_id);
        assert!(entry_queue.check_entry(last_id));
    }
    #[test]
    fn test_reject_old_last_id() {
        let last_id = Hash::default();
        let mut entry_queue = LastIdQueue::default();
        for i in 0..MAX_ENTRY_IDS {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            entry_queue.register_tick(&last_id);
        }
        // Assert we're no longer able to use the oldest entry ID.
        assert!(!entry_queue.check_entry(last_id));
    }
    #[test]
    fn test_fork() {
        let last_id = Hash::default();
        let mut first = LastIdQueue::default();
        assert!(!first.check_entry(last_id));
        first.register_tick(&last_id);
        let second = first.fork();
        assert!(second.check_entry(last_id));
    }
    #[test]
    fn test_merge() {
        let last_id = Hash::default();
        let mut first = LastIdQueue::default();
        assert!(!first.check_entry(last_id));
        let mut second = first.fork();
        second.register_tick(&last_id);
        first.merge_into_root(second);
        assert!(first.check_entry(last_id));
    }
}
