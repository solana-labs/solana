//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

extern crate libc;

use hash::Hash;
use entry::Entry;
use event::Event;
use plan::{Plan, Witness};
use transaction::Transaction;
use signature::{KeyPair, PublicKey, Signature};
use mint::Mint;
use historian::Historian;
use recorder::Signal;
use std::sync::mpsc::SendError;
use std::collections::{HashMap, HashSet};
use std::collections::hash_map::Entry::Occupied;
use std::result;
use chrono::prelude::*;
use self::libc::size_t;
//use std::mem::transmute;
use std::mem::size_of;

#[link(name = "cuda_verify_ed25519")]
extern {
    fn ed25519_verify_many(packets: *const u8, packet_lens: *const u32, packet_offsets: *const u32,
                           message_lens: *const u32, message_offsets: *const u32,
                           public_key_offset: u32, signature_offset: u32,
                           num_keys: size_t, out: *mut u8) -> u32;
}

#[derive(Debug, PartialEq, Eq)]
pub enum AccountingError {
    InsufficientFunds,
    InvalidTransfer,
    InvalidTransferSignature,
    SendError,
}

macro_rules! offset_of {
    ($ty:ty, $field:ident) => {
        unsafe { &(*(0 as *const $ty)).$field as *const _ as usize }
    }
}

pub type Result<T> = result::Result<T, AccountingError>;

/// Commit funds to the 'to' party.
fn complete_transaction(balances: &mut HashMap<PublicKey, i64>, plan: &Plan) {
    if let Plan::Pay(ref payment) = *plan {
        *balances.entry(payment.to).or_insert(0) += payment.tokens;
    }
}

pub struct Accountant {
    pub historian: Historian,
    pub balances: HashMap<PublicKey, i64>,
    pub first_id: Hash,
    pending: HashMap<Signature, Plan>,
    time_sources: HashSet<PublicKey>,
    last_time: DateTime<Utc>,
}

impl Accountant {
    pub fn new_from_entries<I>(entries: I, ms_per_tick: Option<u64>) -> Self
    where
        I: IntoIterator<Item = Entry>,
    {
        let mut entries = entries.into_iter();

        // The first item in the ledger is required to be an entry with zero num_hashes,
        // which implies its id can be used as the ledger's seed.
        let entry0 = entries.next().unwrap();
        let start_hash = entry0.id;

        let hist = Historian::new(&start_hash, ms_per_tick);
        let mut acc = Accountant {
            historian: hist,
            balances: HashMap::new(),
            first_id: start_hash,
            pending: HashMap::new(),
            time_sources: HashSet::new(),
            last_time: Utc.timestamp(0, 0),
        };

        // The second item in the ledger is a special transaction where the to and from
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

    fn is_deposit(allow_deposits: bool, from: &PublicKey, plan: &Plan) -> bool {
        if let Plan::Pay(ref payment) = *plan {
            allow_deposits && *from == payment.to
        } else {
            false
        }
    }

    pub fn process_packets(self: &mut Self, trs: &[Transaction]) {

        println!("Starting verify num packets: {}", trs.len());
        if trs.len() == 0 {
            return;
        }

        let len = trs.len();
        let mut packet_lens : Vec<u32> = Vec::with_capacity(len);
        let mut packet_offsets : Vec<u32> = Vec::with_capacity(len);
        let mut message_offsets : Vec<u32> = Vec::with_capacity(len);
        let mut message_lens : Vec<u32> = Vec::with_capacity(len);
        let signature_offset: u32 = offset_of!(Transaction, sig) as u32;
        let public_key_offset: u32 = offset_of!(Transaction, from) as u32;
        let num_keys: size_t = trs.len();
        let mut out: Vec<u8> = Vec::with_capacity(len);
        for (i, _tr) in trs.iter().enumerate() {
            packet_lens[i] = size_of::<Transaction>() as u32;
            packet_offsets[i] = (i * size_of::<Transaction>()) as u32;
            message_offsets[i] = offset_of!(Transaction, plan) as u32;
            message_lens[i] = (offset_of!(Transaction, sig) - offset_of!(Transaction, plan)) as u32;
        }
        println!("Starting verify num packets: {}", trs.len());
        unsafe {
            let res = ed25519_verify_many(trs.as_ptr() as *const u8,//std::mem::transmute::<&[Transaction], &[u8]>(trs),
                                          packet_lens.as_ptr(), packet_offsets.as_ptr(),
                                          message_lens.as_ptr(), message_offsets.as_ptr(),
                                          public_key_offset, signature_offset, num_keys,
                                          out.as_mut_ptr());
            if res != 0 {
            }
        }
        println!("done verify");
    }

    pub fn process_transaction(self: &mut Self, tr: Transaction) -> Result<()> {
        if !tr.verify() {
            return Err(AccountingError::InvalidTransfer);
        }

        if self.get_balance(&tr.from).unwrap_or(0) < tr.tokens {
            return Err(AccountingError::InsufficientFunds);
        }

        self.process_verified_transaction(&tr, false)?;
        if let Err(SendError(_)) = self.historian
            .sender
            .send(Signal::Event(Event::Transaction(tr)))
        {
            return Err(AccountingError::SendError);
        }

        Ok(())
    }

    fn process_verified_transaction(
        self: &mut Self,
        tr: &Transaction,
        allow_deposits: bool,
    ) -> Result<()> {
        if !self.historian.reserve_signature(&tr.sig) {
            return Err(AccountingError::InvalidTransferSignature);
        }

        if !Self::is_deposit(allow_deposits, &tr.from, &tr.plan) {
            if let Some(x) = self.balances.get_mut(&tr.from) {
                *x -= tr.tokens;
            }
        }

        let mut plan = tr.plan.clone();
        plan.apply_witness(&Witness::Timestamp(self.last_time));

        if plan.is_complete() {
            complete_transaction(&mut self.balances, &plan);
        } else {
            self.pending.insert(tr.sig, plan);
        }

        Ok(())
    }

    fn process_verified_sig(&mut self, from: PublicKey, tx_sig: Signature) -> Result<()> {
        if let Occupied(mut e) = self.pending.entry(tx_sig) {
            e.get_mut().apply_witness(&Witness::Signature(from));
            if e.get().is_complete() {
                complete_transaction(&mut self.balances, e.get());
                e.remove_entry();
            }
        };

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

        // Check to see if any timelocked transactions can be completed.
        let mut completed = vec![];
        for (key, plan) in &mut self.pending {
            plan.apply_witness(&Witness::Timestamp(self.last_time));
            if plan.is_complete() {
                complete_transaction(&mut self.balances, plan);
                completed.push(key.clone());
            }
        }

        for key in completed {
            self.pending.remove(&key);
        }

        Ok(())
    }

    fn process_verified_event(self: &mut Self, event: &Event, allow_deposits: bool) -> Result<()> {
        match *event {
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
        last_id: Hash,
    ) -> Result<Signature> {
        let tr = Transaction::new(keypair, to, n, last_id);
        let sig = tr.sig;
        self.process_transaction(tr).map(|_| sig)
    }

    pub fn transfer_on_date(
        self: &mut Self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Result<Signature> {
        let tr = Transaction::new_on_date(keypair, to, dt, n, last_id);
        let sig = tr.sig;
        self.process_transaction(tr).map(|_| sig)
    }

    pub fn get_balance(self: &Self, pubkey: &PublicKey) -> Option<i64> {
        self.balances.get(pubkey).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::KeyPairUtil;
    use recorder::ExitReason;

    #[test]
    fn test_accountant() {
        let alice = Mint::new(10_000);
        let bob_pubkey = KeyPair::new().pubkey();
        let mut acc = Accountant::new(&alice, Some(2));
        acc.transfer(1_000, &alice.keypair(), bob_pubkey, alice.seed())
            .unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);

        acc.transfer(500, &alice.keypair(), bob_pubkey, alice.seed())
            .unwrap();
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
        acc.transfer(1_000, &alice.keypair(), bob_pubkey, alice.seed())
            .unwrap();
        assert_eq!(
            acc.transfer(10_001, &alice.keypair(), bob_pubkey, alice.seed()),
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
    fn test_overspend_attack() {
        let alice = Mint::new(1);
        let mut acc = Accountant::new(&alice, None);
        let bob_pubkey = KeyPair::new().pubkey();
        let mut tr = Transaction::new(&alice.keypair(), bob_pubkey, 1, alice.seed());
        if let Plan::Pay(ref mut payment) = tr.plan {
            payment.tokens = 2; // <-- attack!
        }
        assert_eq!(
            acc.process_transaction(tr.clone()),
            Err(AccountingError::InvalidTransfer)
        );

        // Also, ensure all branchs of the plan spend all tokens
        if let Plan::Pay(ref mut payment) = tr.plan {
            payment.tokens = 0; // <-- whoops!
        }
        assert_eq!(
            acc.process_transaction(tr.clone()),
            Err(AccountingError::InvalidTransfer)
        );
    }

    #[test]
    fn test_transfer_to_newb() {
        let alice = Mint::new(10_000);
        let mut acc = Accountant::new(&alice, Some(2));
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        acc.transfer(500, &alice_keypair, bob_pubkey, alice.seed())
            .unwrap();
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
        acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt, alice.seed())
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
        acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt, alice.seed())
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
        let sig = acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt, alice.seed())
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
