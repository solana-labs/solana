//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use hash::Hash;
use entry::Entry;
use event::Event;
use transaction::{Condition, Transaction};
use signature::{KeyPair, PublicKey, Signature};
use mint::Mint;
use historian::{reserve_signature, Historian};
use std::sync::mpsc::SendError;
use std::collections::{HashMap, HashSet};
use std::result;
use chrono::prelude::*;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountingError {
    InsufficientFunds,
    InvalidTransfer,
    InvalidTransferSignature,
    SendError,
}

pub type Result<T> = result::Result<T, AccountingError>;

pub struct Accountant {
    pub historian: Historian,
    pub balances: HashMap<PublicKey, i64>,
    pub first_id: Hash,
    pub last_id: Hash,
    pending: HashMap<Signature, Transaction<i64>>,
    time_sources: HashSet<PublicKey>,
    last_time: DateTime<Utc>,
}

impl Accountant {
    pub fn new_from_entries<I>(entries: I, ms_per_tick: Option<u64>) -> Self
    where
        I: IntoIterator<Item = Entry>,
    {
        let mut entries = entries.into_iter();

        // The first item in the log is required to be an entry with zero num_hashes,
        // which implies its id can be used as the log's seed.
        let entry0 = entries.next().unwrap();
        let start_hash = entry0.id;

        let hist = Historian::new(&start_hash, ms_per_tick);
        let mut acc = Accountant {
            historian: hist,
            balances: HashMap::new(),
            first_id: start_hash,
            last_id: start_hash,
            pending: HashMap::new(),
            time_sources: HashSet::new(),
            last_time: Utc.timestamp(0, 0),
        };

        // The second item in the log is a special transaction where the to and from
        // fields are the same. That entry should be treated as a deposit, not a
        // transfer to oneself.
        let entry1 = entries.next().unwrap();
        acc.process_verified_event(&entry1.events[0], true).unwrap();

        for entry in entries {
            for event in entry.events {
                acc.process_verified_event(&event, false).unwrap();
            }
        }
        acc
    }

    pub fn new(mint: &Mint, ms_per_tick: Option<u64>) -> Self {
        Self::new_from_entries(mint.create_entries(), ms_per_tick)
    }

    pub fn sync(self: &mut Self) -> Hash {
        while let Ok(entry) = self.historian.receiver.try_recv() {
            self.last_id = entry.id;
        }
        self.last_id
    }

    fn is_deposit(allow_deposits: bool, from: &PublicKey, to: &PublicKey) -> bool {
        allow_deposits && from == to
    }

    pub fn process_transaction(self: &mut Self, tr: Transaction<i64>) -> Result<()> {
        if !tr.verify() {
            return Err(AccountingError::InvalidTransfer);
        }

        if self.get_balance(&tr.from).unwrap_or(0) < tr.asset {
            return Err(AccountingError::InsufficientFunds);
        }

        self.process_verified_transaction(&tr, false)?;
        if let Err(SendError(_)) = self.historian.sender.send(Event::Transaction(tr)) {
            return Err(AccountingError::SendError);
        }

        Ok(())
    }

    /// Commit funds to the 'to' party.
    fn complete_transaction(self: &mut Self, tr: &Transaction<i64>) {
        if self.balances.contains_key(&tr.to) {
            if let Some(x) = self.balances.get_mut(&tr.to) {
                *x += tr.asset;
            }
        } else {
            self.balances.insert(tr.to, tr.asset);
        }
    }

    /// Return funds to the 'from' party.
    fn cancel_transaction(self: &mut Self, tr: &Transaction<i64>) {
        if let Some(x) = self.balances.get_mut(&tr.from) {
            *x += tr.asset;
        }
    }

    // TODO: Move this to transaction.rs
    fn all_satisfied(&self, conds: &[Condition]) -> bool {
        let mut satisfied = true;
        for cond in conds {
            if let &Condition::Timestamp(dt) = cond {
                if dt > self.last_time {
                    satisfied = false;
                }
            } else {
                satisfied = false;
            }
        }
        satisfied
    }

    fn process_verified_transaction(
        self: &mut Self,
        tr: &Transaction<i64>,
        allow_deposits: bool,
    ) -> Result<()> {
        if !reserve_signature(&mut self.historian.signatures, &tr.sig) {
            return Err(AccountingError::InvalidTransferSignature);
        }

        if !tr.unless_any.is_empty() {
            // TODO: Check to see if the transaction is expired.
        }

        if !Self::is_deposit(allow_deposits, &tr.from, &tr.to) {
            if let Some(x) = self.balances.get_mut(&tr.from) {
                *x -= tr.asset;
            }
        }

        if !self.all_satisfied(&tr.if_all) {
            self.pending.insert(tr.sig, tr.clone());
            return Ok(());
        }

        self.complete_transaction(tr);
        Ok(())
    }

    fn process_verified_sig(&mut self, from: PublicKey, tx_sig: Signature) -> Result<()> {
        let mut cancel = false;
        if let Some(tr) = self.pending.get(&tx_sig) {
            // Cancel:
            // if Signature(from) is in unless_any, return funds to tx.from, and remove the tx from this map.

            // TODO: Use find().
            for cond in &tr.unless_any {
                if let Condition::Signature(pubkey) = *cond {
                    if from == pubkey {
                        cancel = true;
                        break;
                    }
                }
            }
        }

        if cancel {
            if let Some(tr) = self.pending.remove(&tx_sig) {
                self.cancel_transaction(&tr);
            }
        }

        // Process Multisig:
        // otherwise, if "Signature(from) is in if_all, remove it. If that causes that list
        // to be empty, add the asset to to, and remove the tx from this map.
        Ok(())
    }

    fn process_verified_timestamp(&mut self, from: PublicKey, dt: DateTime<Utc>) -> Result<()> {
        // If this is the first timestamp we've seen, it probably came from the genesis block,
        // so we'll trust it.
        if self.last_time == Utc.timestamp(0, 0) {
            self.time_sources.insert(from);
        }

        if self.time_sources.contains(&from) {
            if dt > self.last_time {
                self.last_time = dt;
            }
        } else {
            return Ok(());
        }
        // TODO: Lookup pending Transaction waiting on time, signed by a whitelisted PublicKey.

        // Expire:
        // if a Timestamp after this DateTime is in unless_any, return funds to tx.from,
        // and remove the tx from this map.

        // Check to see if any timelocked transactions can be completed.
        let mut completed = vec![];
        for (key, tr) in &self.pending {
            for cond in &tr.if_all {
                if let Condition::Timestamp(dt) = *cond {
                    if self.last_time >= dt {
                        if tr.if_all.len() == 1 {
                            completed.push(*key);
                        }
                    }
                }
            }
            // TODO: Add this in once we start removing constraints
            //if tr.if_all.is_empty() {
            //    // TODO: Remove tr from pending
            //    self.complete_transaction(tr);
            //}
        }

        for key in completed {
            if let Some(tr) = self.pending.remove(&key) {
                self.complete_transaction(&tr);
            }
        }

        Ok(())
    }

    fn process_verified_event(self: &mut Self, event: &Event, allow_deposits: bool) -> Result<()> {
        match *event {
            Event::Tick => Ok(()),
            Event::Transaction(ref tr) => self.process_verified_transaction(tr, allow_deposits),
            Event::Signature { from, tx_sig, .. } => self.process_verified_sig(from, tx_sig),
            Event::Timestamp { from, dt, .. } => self.process_verified_timestamp(from, dt),
        }
    }

    pub fn transfer(
        self: &mut Self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
    ) -> Result<Signature> {
        let tr = Transaction::new(keypair, to, n, self.last_id);
        let sig = tr.sig;
        self.process_transaction(tr).map(|_| sig)
    }

    pub fn transfer_on_date(
        self: &mut Self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
    ) -> Result<Signature> {
        let tr = Transaction::new_on_date(keypair, to, dt, n, self.last_id);
        let sig = tr.sig;
        self.process_transaction(tr).map(|_| sig)
    }

    pub fn get_balance(self: &Self, pubkey: &PublicKey) -> Option<i64> {
        self.balances.get(pubkey).map(|x| *x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::KeyPairUtil;
    use logger::ExitReason;

    #[test]
    fn test_accountant() {
        let alice = Mint::new(10_000);
        let bob_pubkey = KeyPair::new().pubkey();
        let mut acc = Accountant::new(&alice, Some(2));
        acc.transfer(1_000, &alice.keypair(), bob_pubkey).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);

        acc.transfer(500, &alice.keypair(), bob_pubkey).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap(),
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_invalid_transfer() {
        let alice = Mint::new(11_000);
        let mut acc = Accountant::new(&alice, Some(2));
        let bob_pubkey = KeyPair::new().pubkey();
        acc.transfer(1_000, &alice.keypair(), bob_pubkey).unwrap();
        assert_eq!(
            acc.transfer(10_001, &alice.keypair(), bob_pubkey),
            Err(AccountingError::InsufficientFunds)
        );

        let alice_pubkey = alice.keypair().pubkey();
        assert_eq!(acc.get_balance(&alice_pubkey).unwrap(), 10_000);
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap(),
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_transfer_to_newb() {
        let alice = Mint::new(10_000);
        let mut acc = Accountant::new(&alice, Some(2));
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 500);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap(),
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_transfer_on_date() {
        let alice = Mint::new(1);
        let mut acc = Accountant::new(&alice, Some(2));
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt)
            .unwrap();

        // Alice's balance will be zero because all funds are locked up.
        assert_eq!(acc.get_balance(&alice.pubkey()), Some(0));

        // Bob's balance will be None because the funds have not been
        // sent.
        assert_eq!(acc.get_balance(&bob_pubkey), None);

        // Now, acknowledge the time in the condition occurred and
        // that bob's funds are now available.
        acc.process_verified_timestamp(alice.pubkey(), dt).unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey), Some(1));

        acc.process_verified_timestamp(alice.pubkey(), dt).unwrap(); // <-- Attack! Attempt to process completed transaction.
        assert_ne!(acc.get_balance(&bob_pubkey), Some(2));
    }

    #[test]
    fn test_transfer_after_date() {
        let alice = Mint::new(1);
        let mut acc = Accountant::new(&alice, Some(2));
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        acc.process_verified_timestamp(alice.pubkey(), dt).unwrap();

        // It's now past now, so this transfer should be processed immediately.
        acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt)
            .unwrap();

        assert_eq!(acc.get_balance(&alice.pubkey()), Some(0));
        assert_eq!(acc.get_balance(&bob_pubkey), Some(1));
    }

    #[test]
    fn test_cancel_transfer() {
        let alice = Mint::new(1);
        let mut acc = Accountant::new(&alice, Some(2));
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        let sig = acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt)
            .unwrap();

        // Alice's balance will be zero because all funds are locked up.
        assert_eq!(acc.get_balance(&alice.pubkey()), Some(0));

        // Bob's balance will be None because the funds have not been
        // sent.
        assert_eq!(acc.get_balance(&bob_pubkey), None);

        // Now, cancel the trancaction. Alice gets her funds back, Bob never sees them.
        acc.process_verified_sig(alice.pubkey(), sig).unwrap();
        assert_eq!(acc.get_balance(&alice.pubkey()), Some(1));
        assert_eq!(acc.get_balance(&bob_pubkey), None);

        acc.process_verified_sig(alice.pubkey(), sig).unwrap(); // <-- Attack! Attempt to cancel completed transaction.
        assert_ne!(acc.get_balance(&alice.pubkey()), Some(2));
    }
}
