//! The `accountant` module tracks client balances, and the progress of pending
//! transactions. It offers a high-level public API that signs transactions
//! on behalf of the caller, and a private low-level API for when they have
//! already been signed and verified.

use chrono::prelude::*;
use event::Event;
use hash::Hash;
use mint::Mint;
use plan::{Payment, Plan, Witness};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::hash_map::Entry::Occupied;
use std::collections::{HashMap, HashSet};
use std::result;
use std::sync::RwLock;
use transaction::Transaction;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountingError {
    InsufficientFunds,
    InvalidTransferSignature,
}

pub type Result<T> = result::Result<T, AccountingError>;

/// Commit funds to the 'to' party.
fn apply_payment(balances: &mut HashMap<PublicKey, i64>, payment: &Payment) {
    *balances.entry(payment.to).or_insert(0) += payment.tokens;
}

pub struct Accountant {
    balances: RwLock<HashMap<PublicKey, i64>>,
    pending: RwLock<HashMap<Signature, Plan>>,
    signatures: RwLock<HashSet<Signature>>,
    time_sources: RwLock<HashSet<PublicKey>>,
    last_time: RwLock<DateTime<Utc>>,
}

impl Accountant {
    /// Create an Accountant using a deposit.
    pub fn new_from_deposit(deposit: &Payment) -> Self {
        let mut balances = HashMap::new();
        apply_payment(&mut balances, deposit);
        Accountant {
            balances: RwLock::new(balances),
            pending: RwLock::new(HashMap::new()),
            signatures: RwLock::new(HashSet::new()),
            time_sources: RwLock::new(HashSet::new()),
            last_time: RwLock::new(Utc.timestamp(0, 0)),
        }
    }

    /// Create an Accountant with only a Mint. Typically used by unit tests.
    pub fn new(mint: &Mint) -> Self {
        let deposit = Payment {
            to: mint.pubkey(),
            tokens: mint.tokens,
        };
        Self::new_from_deposit(&deposit)
    }

    fn reserve_signature(&self, sig: &Signature) -> bool {
        if self.signatures.read().unwrap().contains(sig) {
            return false;
        }
        self.signatures.write().unwrap().insert(*sig);
        true
    }

    /// Process a Transaction that has already been verified.
    pub fn process_verified_transaction(&self, tr: &Transaction) -> Result<()> {
        if self.get_balance(&tr.from).unwrap_or(0) < tr.tokens {
            return Err(AccountingError::InsufficientFunds);
        }

        if !self.reserve_signature(&tr.sig) {
            return Err(AccountingError::InvalidTransferSignature);
        }

        if let Some(x) = self.balances.write().unwrap().get_mut(&tr.from) {
            *x -= tr.tokens;
        }

        let mut plan = tr.plan.clone();
        plan.apply_witness(&Witness::Timestamp(*self.last_time.read().unwrap()));

        if let Some(ref payment) = plan.final_payment() {
            apply_payment(&mut self.balances.write().unwrap(), payment);
        } else {
            self.pending.write().unwrap().insert(tr.sig, plan);
        }

        Ok(())
    }

    /// Process a Witness Signature that has already been verified.
    fn process_verified_sig(&self, from: PublicKey, tx_sig: Signature) -> Result<()> {
        if let Occupied(mut e) = self.pending.write().unwrap().entry(tx_sig) {
            e.get_mut().apply_witness(&Witness::Signature(from));
            if let Some(ref payment) = e.get().final_payment() {
                apply_payment(&mut self.balances.write().unwrap(), payment);
                e.remove_entry();
            }
        };

        Ok(())
    }

    /// Process a Witness Timestamp that has already been verified.
    fn process_verified_timestamp(&self, from: PublicKey, dt: DateTime<Utc>) -> Result<()> {
        // If this is the first timestamp we've seen, it probably came from the genesis block,
        // so we'll trust it.
        if *self.last_time.read().unwrap() == Utc.timestamp(0, 0) {
            self.time_sources.write().unwrap().insert(from);
        }

        if self.time_sources.read().unwrap().contains(&from) {
            if dt > *self.last_time.read().unwrap() {
                *self.last_time.write().unwrap() = dt;
            }
        } else {
            return Ok(());
        }

        // Check to see if any timelocked transactions can be completed.
        let mut completed = vec![];

        // Hold 'pending' write lock until the end of this function. Otherwise another thread can
        // double-spend if it enters before the modified plan is removed from 'pending'.
        let mut pending = self.pending.write().unwrap();
        for (key, plan) in pending.iter_mut() {
            plan.apply_witness(&Witness::Timestamp(*self.last_time.read().unwrap()));
            if let Some(ref payment) = plan.final_payment() {
                apply_payment(&mut self.balances.write().unwrap(), payment);
                completed.push(key.clone());
            }
        }

        for key in completed {
            pending.remove(&key);
        }

        Ok(())
    }

    /// Process an Transaction or Witness that has already been verified.
    pub fn process_verified_event(&self, event: &Event) -> Result<()> {
        match *event {
            Event::Transaction(ref tr) => self.process_verified_transaction(tr),
            Event::Signature { from, tx_sig, .. } => self.process_verified_sig(from, tx_sig),
            Event::Timestamp { from, dt, .. } => self.process_verified_timestamp(from, dt),
        }
    }

    /// Create, sign, and process a Transaction from `keypair` to `to` of
    /// `n` tokens where `last_id` is the last Entry ID observed by the client.
    pub fn transfer(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        last_id: Hash,
    ) -> Result<Signature> {
        let tr = Transaction::new(keypair, to, n, last_id);
        let sig = tr.sig;
        self.process_verified_transaction(&tr).map(|_| sig)
    }

    /// Create, sign, and process a postdated Transaction from `keypair`
    /// to `to` of `n` tokens on `dt` where `last_id` is the last Entry ID
    /// observed by the client.
    pub fn transfer_on_date(
        &self,
        n: i64,
        keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Result<Signature> {
        let tr = Transaction::new_on_date(keypair, to, dt, n, last_id);
        let sig = tr.sig;
        self.process_verified_transaction(&tr).map(|_| sig)
    }

    pub fn get_balance(&self, pubkey: &PublicKey) -> Option<i64> {
        self.balances.read().unwrap().get(pubkey).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::KeyPairUtil;

    #[test]
    fn test_accountant() {
        let alice = Mint::new(10_000);
        let bob_pubkey = KeyPair::new().pubkey();
        let acc = Accountant::new(&alice);
        acc.transfer(1_000, &alice.keypair(), bob_pubkey, alice.last_id())
            .unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);

        acc.transfer(500, &alice.keypair(), bob_pubkey, alice.last_id())
            .unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);
    }

    #[test]
    fn test_invalid_transfer() {
        let alice = Mint::new(11_000);
        let acc = Accountant::new(&alice);
        let bob_pubkey = KeyPair::new().pubkey();
        acc.transfer(1_000, &alice.keypair(), bob_pubkey, alice.last_id())
            .unwrap();
        assert_eq!(
            acc.transfer(10_001, &alice.keypair(), bob_pubkey, alice.last_id()),
            Err(AccountingError::InsufficientFunds)
        );

        let alice_pubkey = alice.keypair().pubkey();
        assert_eq!(acc.get_balance(&alice_pubkey).unwrap(), 10_000);
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);
    }

    #[test]
    fn test_transfer_to_newb() {
        let alice = Mint::new(10_000);
        let acc = Accountant::new(&alice);
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        acc.transfer(500, &alice_keypair, bob_pubkey, alice.last_id())
            .unwrap();
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 500);
    }

    #[test]
    fn test_transfer_on_date() {
        let alice = Mint::new(1);
        let acc = Accountant::new(&alice);
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt, alice.last_id())
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
        let acc = Accountant::new(&alice);
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        acc.process_verified_timestamp(alice.pubkey(), dt).unwrap();

        // It's now past now, so this transfer should be processed immediately.
        acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt, alice.last_id())
            .unwrap();

        assert_eq!(acc.get_balance(&alice.pubkey()), Some(0));
        assert_eq!(acc.get_balance(&bob_pubkey), Some(1));
    }

    #[test]
    fn test_cancel_transfer() {
        let alice = Mint::new(1);
        let acc = Accountant::new(&alice);
        let alice_keypair = alice.keypair();
        let bob_pubkey = KeyPair::new().pubkey();
        let dt = Utc::now();
        let sig = acc.transfer_on_date(1, &alice_keypair, bob_pubkey, dt, alice.last_id())
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

    #[test]
    fn test_duplicate_event_signature() {
        let alice = Mint::new(1);
        let acc = Accountant::new(&alice);
        let sig = Signature::default();
        assert!(acc.reserve_signature(&sig));
        assert!(!acc.reserve_signature(&sig));
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use accountant::*;
    use rayon::prelude::*;
    use signature::KeyPairUtil;

    #[bench]
    fn process_verified_event_bench(bencher: &mut Bencher) {
        let alice = Mint::new(100_000_000);
        let alice_keypair = alice.keypair();
        let last_id = Hash::default();
        let transactions: Vec<_> = (0..4096)
            .into_par_iter()
            .map(|_| {
                let rando_pubkey = KeyPair::new().pubkey();
                Transaction::new(&alice_keypair, rando_pubkey, 1, last_id)
            })
            .collect();
        bencher.iter(|| {
            let acc = Accountant::new(&alice);
            transactions.par_iter().for_each(move |tr| {
                acc.process_verified_transaction(tr).unwrap();
            });
        });
    }
}
