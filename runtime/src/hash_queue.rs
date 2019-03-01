use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::timing::timestamp;

#[derive(Debug, PartialEq, Eq, Clone)]
struct HashQueueEntry {
    timestamp: u64,
    hash_height: u64,
}

/// Low memory overhead, so can be cloned for every checkpoint
#[derive(Clone)]
pub struct HashQueue {
    /// updated whenever an hash is registered
    hash_height: u64,

    /// last hash to be registered
    last_hash: Option<Hash>,

    entries: HashMap<Hash, HashQueueEntry>,

    max_entries: usize,
}

impl HashQueue {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::new(),
            hash_height: 0,
            last_hash: None,
            max_entries,
        }
    }

    pub fn hash_height(&self) -> u64 {
        self.hash_height
    }

    pub fn last_hash(&self) -> Hash {
        self.last_hash.expect("no hash has been set")
    }

    /// Check if the age of the entry is within the max_age
    /// return false for any entries with an age equal to or above max_age
    pub fn check_entry_age(&self, entry: Hash, max_age: usize) -> bool {
        let entry = self.entries.get(&entry);
        match entry {
            Some(entry) => self.hash_height - entry.hash_height < max_age as u64,
            _ => false,
        }
    }
    /// check if entry is valid
    #[cfg(test)]
    pub fn check_entry(&self, entry: Hash) -> bool {
        self.entries.get(&entry).is_some()
    }

    pub fn genesis_hash(&mut self, hash: &Hash) {
        self.entries.insert(
            *hash,
            HashQueueEntry {
                hash_height: 0,
                timestamp: timestamp(),
            },
        );

        self.last_hash = Some(*hash);
    }

    pub fn register_hash(&mut self, hash: &Hash) {
        self.hash_height += 1;
        let hash_height = self.hash_height;

        // this clean up can be deferred until sigs gets larger
        //  because we verify entry.nth every place we check for validity
        let max_entries = self.max_entries;
        if self.entries.len() >= max_entries {
            self.entries
                .retain(|_, entry| hash_height - entry.hash_height <= max_entries as u64);
        }

        self.entries.insert(
            *hash,
            HashQueueEntry {
                hash_height,
                timestamp: timestamp(),
            },
        );

        self.last_hash = Some(*hash);
    }

    /// Maps a hash height to a timestamp
    pub fn hash_height_to_timestamp(&self, hash_height: u64) -> Option<u64> {
        for entry in self.entries.values() {
            if entry.hash_height == hash_height {
                return Some(entry.timestamp);
            }
        }
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;
    use solana_sdk::hash::hash;
    use solana_sdk::timing::MAX_RECENT_TICK_HASHES;

    #[test]
    fn test_register_hash() {
        let last_hash = Hash::default();
        let mut entry_queue = HashQueue::new(MAX_RECENT_TICK_HASHES);
        assert!(!entry_queue.check_entry(last_hash));
        entry_queue.register_hash(&last_hash);
        assert!(entry_queue.check_entry(last_hash));
        assert_eq!(entry_queue.hash_height(), 1);
    }
    #[test]
    fn test_reject_old_last_hash() {
        let last_hash = Hash::default();
        let mut entry_queue = HashQueue::new(MAX_RECENT_TICK_HASHES);
        for i in 0..MAX_RECENT_TICK_HASHES {
            let last_hash = hash(&serialize(&i).unwrap()); // Unique hash
            entry_queue.register_hash(&last_hash);
        }
        // Assert we're no longer able to use the oldest entry ID.
        assert!(!entry_queue.check_entry(last_hash));
    }
}
