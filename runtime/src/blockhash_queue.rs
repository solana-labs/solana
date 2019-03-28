use hashbrown::HashMap;
use solana_sdk::hash::Hash;
use solana_sdk::timing::timestamp;

#[derive(Debug, PartialEq, Eq, Clone)]
struct HashAge {
    timestamp: u64,
    hash_height: u64,
}

/// Low memory overhead, so can be cloned for every checkpoint
#[derive(Clone)]
pub struct BlockhashQueue {
    /// updated whenever an hash is registered
    hash_height: u64,

    /// last hash to be registered
    last_hash: Option<Hash>,

    ages: HashMap<Hash, HashAge>,

    /// hashes older than `max_age` will be dropped from the queue
    max_age: usize,
}

impl BlockhashQueue {
    pub fn new(max_age: usize) -> Self {
        Self {
            ages: HashMap::new(),
            hash_height: 0,
            last_hash: None,
            max_age,
        }
    }

    #[allow(dead_code)]
    pub fn hash_height(&self) -> u64 {
        self.hash_height
    }

    pub fn last_hash(&self) -> Hash {
        self.last_hash.expect("no hash has been set")
    }

    /// Check if the age of the hash is within the max_age
    /// return false for any hashes with an age above max_age
    pub fn check_hash_age(&self, hash: Hash, max_age: usize) -> bool {
        let hash_age = self.ages.get(&hash);
        match hash_age {
            Some(age) => self.hash_height - age.hash_height <= max_age as u64,
            _ => false,
        }
    }
    /// check if hash is valid
    #[cfg(test)]
    pub fn check_hash(&self, hash: Hash) -> bool {
        self.ages.get(&hash).is_some()
    }

    pub fn genesis_hash(&mut self, hash: &Hash) {
        self.ages.insert(
            *hash,
            HashAge {
                hash_height: 0,
                timestamp: timestamp(),
            },
        );

        self.last_hash = Some(*hash);
    }

    fn check_age(hash_height: u64, max_age: usize, age: &HashAge) -> bool {
        hash_height - age.hash_height <= max_age as u64
    }

    pub fn register_hash(&mut self, hash: &Hash) {
        self.hash_height += 1;
        let hash_height = self.hash_height;

        // this clean up can be deferred until sigs gets larger
        //  because we verify age.nth every place we check for validity
        let max_age = self.max_age;
        if self.ages.len() >= max_age {
            self.ages
                .retain(|_, age| Self::check_age(hash_height, max_age, age));
        }
        self.ages.insert(
            *hash,
            HashAge {
                hash_height,
                timestamp: timestamp(),
            },
        );

        self.last_hash = Some(*hash);
    }

    /// Maps a hash height to a timestamp
    pub fn hash_height_to_timestamp(&self, hash_height: u64) -> Option<u64> {
        for age in self.ages.values() {
            if age.hash_height == hash_height {
                return Some(age.timestamp);
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

    #[test]
    fn test_register_hash() {
        let last_hash = Hash::default();
        let mut hash_queue = BlockhashQueue::new(100);
        assert!(!hash_queue.check_hash(last_hash));
        hash_queue.register_hash(&last_hash);
        assert!(hash_queue.check_hash(last_hash));
        assert_eq!(hash_queue.hash_height(), 1);
    }
    #[test]
    fn test_reject_old_last_hash() {
        let mut hash_queue = BlockhashQueue::new(100);
        let last_hash = hash(&serialize(&0).unwrap());
        for i in 0..102 {
            let last_hash = hash(&serialize(&i).unwrap());
            hash_queue.register_hash(&last_hash);
        }
        // Assert we're no longer able to use the oldest hash.
        assert!(!hash_queue.check_hash(last_hash));
    }
    /// test that when max age is 0, that a valid last_hash still passes the age check
    #[test]
    fn test_queue_init_blockhash() {
        let last_hash = Hash::default();
        let mut hash_queue = BlockhashQueue::new(100);
        hash_queue.register_hash(&last_hash);
        assert_eq!(last_hash, hash_queue.last_hash());
        assert!(hash_queue.check_hash_age(last_hash, 0));
    }
}
