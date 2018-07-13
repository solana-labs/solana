//! The `consistency_manager` manages everything related to the
//! consistency of the state in relation to the network. These tasks include:
//! 
//! 1) Entry voting and vote tracking from peers across the network
//! 2) Snapshotting state + rollback

use bank::Bank;
use entry::Entry;
use hash::Hash;
use result::Result;
use signature::{KeyPair, PublicKey, Signature};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use transaction::{Instruction, Transaction};
use transaction_sender::send_transaction;

// The interval (measured in number of entries) before submitting votes
pub const VOTE_INTERVAL: u64 = 1000;

pub struct ValidationEntry {
    entry_id: Hash,
    entry_height: u64,
    /// Set of validators who voted for this entry
    votes: HashSet<PublicKey>,
    total_staked_votes: f64,
}

impl ValidationEntry {
    pub fn new(
        entry_id: Hash,
        entry_height: u64,
    ) -> Self 
    {
        ValidationEntry{
            entry_id,
            entry_height,
            votes: HashSet::new(),
            total_staked_votes: 0.0,
        }
    }

    pub fn has_supermajority() -> bool{
        return true;
    }

    pub fn add_vote(&mut self, new_voter: PublicKey) {
        if self.votes.contains(&new_voter) {
            return;
        } else {
            self.votes.insert(new_voter);
            // TODO: Increment total_staked votes after 
            // implementing real stake tracking that handles dynamic
            // validator sets
        }
    }
}

pub struct VoteContract {
    handle_instruction: &'a Fn(Instruction) -> (),
}

impl VoteContract {
    fn new(handle_instruction: &'a Fn(Instruction) -> ()) {
        VoteContract(handle_instruction);
    }
}

// TODOs:
// 1) Add snapshotting of the bank
// 2) Implement rollback
// 3) Support for dynamic validator sets and stake tracking
pub struct ConsistencyManager {
    bank: Arc<Bank>,
    keypair: KeyPair,

    /// The number of entries successfully processed since the start of the ledger.
    /// Current assumption is this will only be touched by one thread 
    /// at a time b/c only one thread should be running process_entries() at a time.
    entry_height: u64,

    // Map from entry id to the ValidationInfo containing information about the state
    // of the network's vote on a particular entry
    entry_vote_info: HashMap<Hash, ValidationEntry>,
}

impl ConsistencyManager {
    pub fn new(
        bank: Arc<Bank>,
        keypair: KeyPair,
        entry_height: u64,
    ) -> Self 
    {
        let handle_instruction = 
        let vote_contract = VoteContract::new(

        )
        ConsistencyManager{bank, keypair, entry_height, entry_vote_info: HashMap::new()}
    }

    pub fn process_entries(&mut self, new_entries: Vec<Entry>) -> Result<()> {
        for entry in new_entries {
            let result = self.bank.process_entry(entry);
            result?;
            self.entry_height += 1;

            // Check if it's time to send a vote
            if self.entry_height % VOTE_INTERVAL == 0 {
                self.generate_and_send_vote(entry);
            }

            // TODO: Check if any rollbacks are required, clean up stale validation rounds
        }

        Ok(())
    }

    pub fn process_validation_vote(
        &mut self,
        entry_id: u64,
        entry_height: u64,
        from: PublicKey
    ) -> 
    {
        let votes_for_entry = self.entry_vote_info.entry(entry_id).or_insert(HashMap::new());
        votes_for_entry.add_vote(from);
    }

    fn generate_and_send_vote(&mut self, entry: Entry) {
        let vote = self.generate_vote(entry);
        send_transaction(vec![vote]);
    }

    fn entry_to_vote(&self, entry: Entry) -> Signature {
        let sign_data = entry.id;
        Signature::clone_from_slice(self.keypair.sign(&sign_data).as_ref())
    }

    fn generate_vote(
        &mut self,
        entry: Entry,
    ) -> Transaction
    {
        let sig = self.entry_to_vote(entry);

        // TODO: think about whether it would be better to model the vote
        // as a Protocol object rather than a Transaction at this point
        Transaction::new_validation_vote(
            &self.keypair,
            entry.id,
            sig,
            self.bank.last_id(),
        )
    }

    fn check_rollback(&mut self) {

    }
}
