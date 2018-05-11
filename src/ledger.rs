//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger.

use bincode::{deserialize, serialize_into};
use entry::{next_entry, Entry};
use event::Event;
use hash::Hash;
use packet;
use packet::{SharedBlob, BLOB_DATA_SIZE, BLOB_SIZE};
use rayon::prelude::*;
use std::cmp::min;
use std::collections::VecDeque;
use std::io::Cursor;
use std::mem::size_of;

pub trait Block {
    /// Verifies the hashes and counts of a slice of events are all consistent.
    fn verify(&self, start_hash: &Hash) -> bool;
}

impl Block for [Entry] {
    fn verify(&self, start_hash: &Hash) -> bool {
        let genesis = [Entry::new_tick(0, start_hash)];
        let entry_pairs = genesis.par_iter().chain(self).zip(self);
        entry_pairs.all(|(x0, x1)| x1.verify(&x0.id))
    }
}

/// Create a vector of Entries of length `event_set.len()` from `start_hash` hash, `num_hashes`, and `event_set`.
pub fn next_entries(start_hash: &Hash, num_hashes: u64, event_set: Vec<Vec<Event>>) -> Vec<Entry> {
    let mut id = *start_hash;
    let mut entries = vec![];
    for event_list in &event_set {
        let events = event_list.clone();
        let entry = next_entry(&id, num_hashes, events);
        id = entry.id;
        entries.push(entry);
    }
    entries
}

pub fn process_entry_list_into_blobs(
    list: &Vec<Entry>,
    blob_recycler: &packet::BlobRecycler,
    q: &mut VecDeque<SharedBlob>,
) {
    let mut start = 0;
    let mut end = 0;
    while start < list.len() {
        let mut entries: Vec<Vec<Entry>> = Vec::new();
        let mut total = 0;
        for i in &list[start..] {
            total += size_of::<Event>() * i.events.len();
            total += size_of::<Entry>();
            if total >= BLOB_DATA_SIZE {
                break;
            }
            end += 1;
        }
        // See if we need to split the events
        if end <= start {
            let mut event_start = 0;
            let num_events_per_blob = BLOB_DATA_SIZE / size_of::<Event>();
            let total_entry_chunks =
                (list[end].events.len() + num_events_per_blob - 1) / num_events_per_blob;
            trace!(
                "splitting events end: {} total_chunks: {}",
                end,
                total_entry_chunks
            );
            for _ in 0..total_entry_chunks {
                let event_end = min(event_start + num_events_per_blob, list[end].events.len());
                let mut entry = Entry {
                    num_hashes: list[end].num_hashes,
                    id: list[end].id,
                    events: list[end].events[event_start..event_end].to_vec(),
                };
                entries.push(vec![entry]);
                event_start = event_end;
            }
            end += 1;
        } else {
            entries.push(list[start..end].to_vec());
        }

        for entry in entries {
            let b = blob_recycler.allocate();
            let pos = {
                let mut bd = b.write().unwrap();
                let mut out = Cursor::new(bd.data_mut());
                serialize_into(&mut out, &entry).expect("failed to serialize output");
                out.position() as usize
            };
            assert!(pos < BLOB_SIZE);
            b.write().unwrap().set_size(pos);
            q.push_back(b);
        }
        start = end;
    }
}

pub fn reconstruct_entries_from_blobs(blobs: &VecDeque<SharedBlob>) -> Vec<Entry> {
    let mut entries_to_apply: Vec<Entry> = Vec::new();
    let mut last_id = Hash::default();
    for msgs in blobs {
        let blob = msgs.read().unwrap();
        let entries: Vec<Entry> = deserialize(&blob.data()[..blob.meta.size]).unwrap();
        for entry in entries {
            if entry.id == last_id {
                if let Some(last_entry) = entries_to_apply.last_mut() {
                    last_entry.events.extend(entry.events);
                }
            } else {
                last_id = entry.id;
                entries_to_apply.push(entry);
            }
        }
        //TODO respond back to leader with hash of the state
    }
    entries_to_apply
}

#[cfg(test)]
mod tests {
    use super::*;
    use entry;
    use hash::hash;
    use packet::BlobRecycler;
    use signature::{KeyPair, KeyPairUtil};
    use transaction::Transaction;

    #[test]
    fn test_verify_slice() {
        let zero = Hash::default();
        let one = hash(&zero);
        assert!(vec![][..].verify(&zero)); // base case
        assert!(vec![Entry::new_tick(0, &zero)][..].verify(&zero)); // singleton case 1
        assert!(!vec![Entry::new_tick(0, &zero)][..].verify(&one)); // singleton case 2, bad
        assert!(next_entries(&zero, 0, vec![vec![]; 2])[..].verify(&zero)); // inductive step

        let mut bad_ticks = next_entries(&zero, 0, vec![vec![]; 2]);
        bad_ticks[1].id = one;
        assert!(!bad_ticks.verify(&zero)); // inductive step, bad
    }

    #[test]
    fn test_entry_to_blobs() {
        let zero = Hash::default();
        let one = hash(&zero);
        let keypair = KeyPair::new();
        let tr0 = Event::Transaction(Transaction::new(&keypair, keypair.pubkey(), 1, one));
        let events = vec![tr0.clone(); 10000];
        let e0 = entry::create_entry(&zero, 0, events);

        let entry_list = vec![e0.clone(); 1];
        let blob_recycler = BlobRecycler::default();
        let mut blob_q = VecDeque::new();
        process_entry_list_into_blobs(&entry_list, &blob_recycler, &mut blob_q);
        let entries = reconstruct_entries_from_blobs(&blob_q);

        assert_eq!(entry_list, entries);
    }

    #[test]
    fn test_next_entries() {
        let mut id = Hash::default();
        let next_id = hash(&id);
        let keypair = KeyPair::new();
        let tr0 = Event::Transaction(Transaction::new(&keypair, keypair.pubkey(), 1, next_id));
        let events = vec![tr0.clone(); 5];
        let event_set = vec![events.clone(); 5];
        let entries0 = next_entries(&id, 0, event_set);

        assert_eq!(entries0.len(), 5);

        let mut entries1 = vec![];
        for _ in 0..5 {
            let entry = next_entry(&id, 0, events.clone());
            id = entry.id;
            entries1.push(entry);
        }
        assert_eq!(entries0, entries1);
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use ledger::*;

    #[bench]
    fn event_bench(bencher: &mut Bencher) {
        let start_hash = Hash::default();
        let entries = next_entries(&start_hash, 10_000, vec![vec![]; 8]);
        bencher.iter(|| {
            assert!(entries.verify(&start_hash));
        });
    }
}
