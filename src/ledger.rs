//! The `ledger` module provides functions for parallel verification of the
//! Proof of History ledger as well as iterative read, append write, and random
//! access read to a persistent file-based ledger.

use crate::db_ledger::{DbLedger, DEFAULT_SLOT_HEIGHT};
use crate::entry::Entry;
use crate::mint::Mint;
use crate::packet::{Blob, SharedBlob, BLOB_DATA_SIZE};
use bincode::{self, serialized_size};
use chrono::prelude::Utc;
use rayon::prelude::*;
use solana_sdk::budget_transaction::BudgetTransaction;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program::Vote;
use solana_sdk::vote_transaction::VoteTransaction;
use std::fs::remove_dir_all;

// a Block is a slice of Entries
pub trait Block {
    /// Verifies the hashes and counts of a slice of transactions are all consistent.
    fn verify(&self, start_hash: &Hash) -> bool;
    fn to_shared_blobs(&self) -> Vec<SharedBlob>;
    fn to_blobs(&self) -> Vec<Blob>;
    fn votes(&self) -> Vec<(Pubkey, Vote, Hash)>;
}

impl Block for [Entry] {
    fn verify(&self, start_hash: &Hash) -> bool {
        let genesis = [Entry {
            tick_height: 0,
            num_hashes: 0,
            id: *start_hash,
            transactions: vec![],
        }];
        let entry_pairs = genesis.par_iter().chain(self).zip(self);
        entry_pairs.all(|(x0, x1)| {
            let r = x1.verify(&x0.id);
            if !r {
                warn!(
                    "entry invalid!: x0: {:?}, x1: {:?} num txs: {}",
                    x0.id,
                    x1.id,
                    x1.transactions.len()
                );
            }
            r
        })
    }

    fn to_blobs(&self) -> Vec<Blob> {
        self.iter().map(|entry| entry.to_blob()).collect()
    }

    fn to_shared_blobs(&self) -> Vec<SharedBlob> {
        self.iter().map(|entry| entry.to_shared_blob()).collect()
    }

    fn votes(&self) -> Vec<(Pubkey, Vote, Hash)> {
        self.iter()
            .flat_map(|entry| {
                entry
                    .transactions
                    .iter()
                    .flat_map(VoteTransaction::get_votes)
            })
            .collect()
    }
}

/// Creates the next entries for given transactions, outputs
/// updates start_hash to id of last Entry, sets num_hashes to 0
pub fn next_entries_mut(
    start_hash: &mut Hash,
    num_hashes: &mut u64,
    transactions: Vec<Transaction>,
) -> Vec<Entry> {
    // TODO: ?? find a number that works better than |?
    //                                               V
    if transactions.is_empty() || transactions.len() == 1 {
        vec![Entry::new_mut(start_hash, num_hashes, transactions)]
    } else {
        let mut chunk_start = 0;
        let mut entries = Vec::new();

        while chunk_start < transactions.len() {
            let mut chunk_end = transactions.len();
            let mut upper = chunk_end;
            let mut lower = chunk_start;
            let mut next = chunk_end; // be optimistic that all will fit

            // binary search for how many transactions will fit in an Entry (i.e. a BLOB)
            loop {
                debug!(
                    "chunk_end {}, upper {} lower {} next {} transactions.len() {}",
                    chunk_end,
                    upper,
                    lower,
                    next,
                    transactions.len()
                );
                if Entry::serialized_size(&transactions[chunk_start..chunk_end])
                    <= BLOB_DATA_SIZE as u64
                {
                    next = (upper + chunk_end) / 2;
                    lower = chunk_end;
                    debug!(
                        "chunk_end {} fits, maybe too well? trying {}",
                        chunk_end, next
                    );
                } else {
                    next = (lower + chunk_end) / 2;
                    upper = chunk_end;
                    debug!("chunk_end {} doesn't fit! trying {}", chunk_end, next);
                }
                // same as last time
                if next == chunk_end {
                    debug!("converged on chunk_end {}", chunk_end);
                    break;
                }
                chunk_end = next;
            }
            entries.push(Entry::new_mut(
                start_hash,
                num_hashes,
                transactions[chunk_start..chunk_end].to_vec(),
            ));
            chunk_start = chunk_end;
        }

        entries
    }
}

/// Creates the next Entries for given transactions
pub fn next_entries(
    start_hash: &Hash,
    num_hashes: u64,
    transactions: Vec<Transaction>,
) -> Vec<Entry> {
    let mut id = *start_hash;
    let mut num_hashes = num_hashes;
    next_entries_mut(&mut id, &mut num_hashes, transactions)
}

pub fn get_tmp_ledger_path(name: &str) -> String {
    use std::env;
    let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
    let keypair = Keypair::new();

    let path = format!("{}/tmp/ledger-{}-{}", out_dir, name, keypair.pubkey());

    // whack any possible collision
    let _ignored = remove_dir_all(&path);

    path
}

pub fn create_tmp_ledger_with_mint(name: &str, mint: &Mint) -> String {
    let path = get_tmp_ledger_path(name);
    DbLedger::destroy(&path).expect("Expected successful database destruction");
    let db_ledger = DbLedger::open(&path).unwrap();
    db_ledger
        .write_entries(DEFAULT_SLOT_HEIGHT, 0, &mint.create_entries())
        .unwrap();

    path
}

pub fn create_tmp_genesis(
    name: &str,
    num: u64,
    bootstrap_leader_id: Pubkey,
    bootstrap_leader_tokens: u64,
) -> (Mint, String) {
    let mint = Mint::new_with_leader(num, bootstrap_leader_id, bootstrap_leader_tokens);
    let path = create_tmp_ledger_with_mint(name, &mint);

    (mint, path)
}

pub fn create_ticks(num_ticks: usize, mut hash: Hash) -> Vec<Entry> {
    let mut ticks = Vec::with_capacity(num_ticks as usize);
    for _ in 0..num_ticks as u64 {
        let new_tick = Entry::new(&hash, 0, 1, vec![]);
        hash = new_tick.id;
        ticks.push(new_tick);
    }

    ticks
}

pub fn create_tmp_sample_ledger(
    name: &str,
    num_tokens: u64,
    num_ending_ticks: usize,
    bootstrap_leader_id: Pubkey,
    bootstrap_leader_tokens: u64,
) -> (Mint, String, Vec<Entry>) {
    let mint = Mint::new_with_leader(num_tokens, bootstrap_leader_id, bootstrap_leader_tokens);
    let path = get_tmp_ledger_path(name);

    // Create the entries
    let mut genesis = mint.create_entries();
    let ticks = create_ticks(num_ending_ticks, mint.last_id());
    genesis.extend(ticks);

    DbLedger::destroy(&path).expect("Expected successful database destruction");
    let db_ledger = DbLedger::open(&path).unwrap();
    db_ledger
        .write_entries(DEFAULT_SLOT_HEIGHT, 0, &genesis)
        .unwrap();

    (mint, path, genesis)
}

pub fn tmp_copy_ledger(from: &str, name: &str) -> String {
    let tostr = get_tmp_ledger_path(name);

    let db_ledger = DbLedger::open(from).unwrap();
    let ledger_entries = db_ledger.read_ledger().unwrap();

    DbLedger::destroy(&tostr).expect("Expected successful database destruction");
    let db_ledger = DbLedger::open(&tostr).unwrap();
    db_ledger
        .write_entries(DEFAULT_SLOT_HEIGHT, 0, ledger_entries)
        .unwrap();

    tostr
}

pub fn make_tiny_test_entries(num: usize) -> Vec<Entry> {
    let zero = Hash::default();
    let one = hash(&zero.as_ref());
    let keypair = Keypair::new();

    let mut id = one;
    let mut num_hashes = 0;
    (0..num)
        .map(|_| {
            Entry::new_mut(
                &mut id,
                &mut num_hashes,
                vec![Transaction::budget_new_timestamp(
                    &keypair,
                    keypair.pubkey(),
                    keypair.pubkey(),
                    Utc::now(),
                    one,
                )],
            )
        })
        .collect()
}

pub fn make_large_test_entries(num_entries: usize) -> Vec<Entry> {
    let zero = Hash::default();
    let one = hash(&zero.as_ref());
    let keypair = Keypair::new();

    let tx = Transaction::budget_new_timestamp(
        &keypair,
        keypair.pubkey(),
        keypair.pubkey(),
        Utc::now(),
        one,
    );

    let serialized_size = serialized_size(&vec![&tx]).unwrap();
    let num_txs = BLOB_DATA_SIZE / serialized_size as usize;
    let txs = vec![tx; num_txs];
    let entry = next_entries(&one, 1, txs)[0].clone();
    vec![entry; num_entries]
}

#[cfg(test)]
pub fn make_consecutive_blobs(
    id: &Pubkey,
    num_blobs_to_make: u64,
    start_height: u64,
    start_hash: Hash,
    addr: &std::net::SocketAddr,
) -> Vec<SharedBlob> {
    let entries = create_ticks(num_blobs_to_make as usize, start_hash);

    let blobs = entries.to_shared_blobs();
    let mut index = start_height;
    for blob in &blobs {
        let mut blob = blob.write().unwrap();
        blob.set_index(index).unwrap();
        blob.set_id(id).unwrap();
        blob.meta.set_addr(addr);
        index += 1;
    }
    blobs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{next_entry, reconstruct_entries_from_blobs, Entry};
    use crate::packet::{to_blobs, BLOB_DATA_SIZE, PACKET_DATA_SIZE};
    use bincode::serialized_size;
    use solana_sdk::hash::hash;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::transaction::Transaction;
    use solana_sdk::vote_program::Vote;
    use std;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn test_verify_slice() {
        solana_logger::setup();
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        assert!(vec![][..].verify(&zero)); // base case
        assert!(vec![Entry::new_tick(0, 0, &zero)][..].verify(&zero)); // singleton case 1
        assert!(!vec![Entry::new_tick(0, 0, &zero)][..].verify(&one)); // singleton case 2, bad
        assert!(vec![next_entry(&zero, 0, vec![]); 2][..].verify(&zero)); // inductive step

        let mut bad_ticks = vec![next_entry(&zero, 0, vec![]); 2];
        bad_ticks[1].id = one;
        assert!(!bad_ticks.verify(&zero)); // inductive step, bad
    }

    fn make_test_entries() -> Vec<Entry> {
        let zero = Hash::default();
        let one = hash(&zero.as_ref());
        let keypair = Keypair::new();
        let vote_account = Keypair::new();
        let tx = Transaction::vote_new(&vote_account.pubkey(), Vote { tick_height: 1 }, one, 1);
        let msg = tx.get_sign_data();
        let sig = Signature::new(&vote_account.sign(&msg).as_ref());
        let tx0 = Transaction {
            signatures: vec![sig],
            account_keys: tx.account_keys,
            last_id: tx.last_id,
            fee: tx.fee,
            program_ids: tx.program_ids,
            instructions: tx.instructions,
        };
        let tx1 = Transaction::budget_new_timestamp(
            &keypair,
            keypair.pubkey(),
            keypair.pubkey(),
            Utc::now(),
            one,
        );
        //
        // TODO: this magic number and the mix of transaction types
        //       is designed to fill up a Blob more or less exactly,
        //       to get near enough the the threshold that
        //       deserialization falls over if it uses the wrong size()
        //       parameter to index into blob.data()
        //
        // magic numbers -----------------+
        //                                |
        //                                V
        let mut transactions = vec![tx0; 362];
        transactions.extend(vec![tx1; 100]);
        next_entries(&zero, 0, transactions)
    }

    #[test]
    fn test_entries_to_shared_blobs() {
        solana_logger::setup();
        let entries = make_test_entries();

        let blob_q = entries.to_blobs();

        assert_eq!(reconstruct_entries_from_blobs(blob_q).unwrap().0, entries);
    }

    #[test]
    fn test_bad_blobs_attack() {
        solana_logger::setup();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8000);
        let blobs_q = to_blobs(vec![(0, addr)]).unwrap(); // <-- attack!
        assert!(reconstruct_entries_from_blobs(blobs_q).is_err());
    }

    #[test]
    fn test_next_entries() {
        solana_logger::setup();
        let id = Hash::default();
        let next_id = hash(&id.as_ref());
        let keypair = Keypair::new();
        let vote_account = Keypair::new();
        let tx = Transaction::vote_new(&vote_account.pubkey(), Vote { tick_height: 1 }, next_id, 2);
        let msg = tx.get_sign_data();
        let sig = Signature::new(&vote_account.sign(&msg).as_ref());
        let tx_small = Transaction {
            signatures: vec![sig],
            account_keys: tx.account_keys,
            last_id: tx.last_id,
            fee: tx.fee,
            program_ids: tx.program_ids,
            instructions: tx.instructions,
        };
        let tx_large = Transaction::budget_new(&keypair, keypair.pubkey(), 1, next_id);

        let tx_small_size = serialized_size(&tx_small).unwrap() as usize;
        let tx_large_size = serialized_size(&tx_large).unwrap() as usize;
        let entry_size = serialized_size(&Entry {
            tick_height: 0,
            num_hashes: 0,
            id: Hash::default(),
            transactions: vec![],
        })
        .unwrap() as usize;
        assert!(tx_small_size < tx_large_size);
        assert!(tx_large_size < PACKET_DATA_SIZE);

        let threshold = (BLOB_DATA_SIZE - entry_size) / tx_small_size;

        // verify no split
        let transactions = vec![tx_small.clone(); threshold];
        let entries0 = next_entries(&id, 0, transactions.clone());
        assert_eq!(entries0.len(), 1);
        assert!(entries0.verify(&id));

        // verify the split with uniform transactions
        let transactions = vec![tx_small.clone(); threshold * 2];
        let entries0 = next_entries(&id, 0, transactions.clone());
        assert_eq!(entries0.len(), 2);
        assert!(entries0.verify(&id));

        // verify the split with small transactions followed by large
        // transactions
        let mut transactions = vec![tx_small.clone(); BLOB_DATA_SIZE / tx_small_size];
        let large_transactions = vec![tx_large.clone(); BLOB_DATA_SIZE / tx_large_size];

        transactions.extend(large_transactions);

        let entries0 = next_entries(&id, 0, transactions.clone());
        assert!(entries0.len() >= 2);
        assert!(entries0.verify(&id));
    }

}
