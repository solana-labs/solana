use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::timing::{timestamp, MAX_ENTRY_IDS};

#[derive(Debug, PartialEq, Eq, Clone)]
struct HashQueueEntry {
    timestamp: u64,
    tick_height: u64,
}

/// Low memory overhead, so can be cloned for every checkpoint
#[derive(Clone)]
pub struct HashQueue {
    /// updated whenever an id is registered, at each tick ;)
    tick_height: u64,

    /// last tick to be registered
    last_id: Option<Hash>,

    entries: HashMap<Hash, HashQueueEntry>,
}
impl Default for HashQueue {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            tick_height: 0,
            last_id: None,
        }
    }
}

impl HashQueue {
    pub fn tick_height(&self) -> u64 {
        self.tick_height
    }

    pub fn last_id(&self) -> Hash {
        self.last_id.expect("no last_id has been set")
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
    /// check if entry is valid
    #[cfg(test)]
    pub fn check_entry(&self, entry_id: Hash) -> bool {
        self.entries.get(&entry_id).is_some()
    }

    pub fn genesis_last_id(&mut self, last_id: &Hash) {
        self.entries.insert(
            *last_id,
            HashQueueEntry {
                tick_height: 0,
                timestamp: timestamp(),
            },
        );

        self.last_id = Some(*last_id);
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
            HashQueueEntry {
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
            if current_tick_height >= *tick_height
                && ((current_tick_height - tick_height) as usize) < MAX_ENTRY_IDS
            {
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

    #[cfg(test)]
    pub fn clear(&mut self) {
        self.entries = HashMap::new();
        self.tick_height = 0;
        self.last_id = None;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::hash::hash;

    #[test]
    fn test_register_tick() {
        let last_id = Hash::default();
        let mut entry_queue = HashQueue::default();
        assert!(!entry_queue.check_entry(last_id));
        entry_queue.register_tick(&last_id);
        assert!(entry_queue.check_entry(last_id));
    }
    #[test]
    fn test_reject_old_last_id() {
        let last_id = Hash::default();
        let mut entry_queue = HashQueue::default();
        for i in 0..MAX_ENTRY_IDS {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            entry_queue.register_tick(&last_id);
        }
        // Assert we're no longer able to use the oldest entry ID.
        assert!(!entry_queue.check_entry(last_id));
    }
}
