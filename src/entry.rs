use hash::{extend_and_hash, hash, Hash};
use event::Event;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Entry {
    pub num_hashes: u64,
    pub id: Hash,
    pub event: Event,
}

impl Entry {
    /// Creates a Entry from the number of hashes 'num_hashes' since the previous event
    /// and that resulting 'id'.
    pub fn new_tick(num_hashes: u64, id: &Hash) -> Self {
        Entry {
            num_hashes,
            id: *id,
            event: Event::Tick,
        }
    }

    /// Verifies self.id is the result of hashing a 'start_hash' 'self.num_hashes' times.
    /// If the event is not a Tick, then hash that as well.
    pub fn verify(&self, start_hash: &Hash) -> bool {
        if !self.event.verify() {
            return false;
        }
        self.id == next_hash(start_hash, self.num_hashes, &self.event)
    }
}

/// Creates the hash 'num_hashes' after start_hash. If the event contains
/// signature, the final hash will be a hash of both the previous ID and
/// the signature.
pub fn next_hash(start_hash: &Hash, num_hashes: u64, event: &Event) -> Hash {
    let mut id = *start_hash;
    let sig = event.get_signature();
    let start_index = if sig.is_some() { 1 } else { 0 };
    for _ in start_index..num_hashes {
        id = hash(&id);
    }
    if let Some(sig) = sig {
        id = extend_and_hash(&id, &sig);
    }
    id
}

/// Creates the next Entry 'num_hashes' after 'start_hash'.
pub fn create_entry(start_hash: &Hash, cur_hashes: u64, event: Event) -> Entry {
    let sig = event.get_signature();
    let num_hashes = cur_hashes + if sig.is_some() { 1 } else { 0 };
    let id = next_hash(start_hash, 0, &event);
    Entry {
        num_hashes,
        id,
        event,
    }
}

/// Creates the next Tick Entry 'num_hashes' after 'start_hash'.
pub fn create_entry_mut(start_hash: &mut Hash, cur_hashes: &mut u64, event: Event) -> Entry {
    let entry = create_entry(start_hash, *cur_hashes, event);
    *start_hash = entry.id;
    *cur_hashes = 0;
    entry
}

/// Creates the next Tick Entry 'num_hashes' after 'start_hash'.
pub fn next_tick(start_hash: &Hash, num_hashes: u64) -> Entry {
    let event = Event::Tick;
    Entry {
        num_hashes,
        id: next_hash(start_hash, num_hashes, &event),
        event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_verify() {
        let zero = Hash::default();
        let one = hash(&zero);
        assert!(Entry::new_tick(0, &zero).verify(&zero)); // base case
        assert!(!Entry::new_tick(0, &zero).verify(&one)); // base case, bad
        assert!(next_tick(&zero, 1).verify(&zero)); // inductive step
        assert!(!next_tick(&zero, 1).verify(&one)); // inductive step, bad
    }

    #[test]
    fn test_next_tick() {
        let zero = Hash::default();
        assert_eq!(next_tick(&zero, 1).num_hashes, 1)
    }
}
