//! The `accountant` module tracks client balances, and the progress of pending
//! transactions. It offers a high-level public API that signs transactions
//! on behalf of the caller, and a private low-level API for when they have
//! already been signed and verified.

extern crate libc;

use chrono::prelude::*;
use event::Event;
use hash::Hash;
use mint::Mint;
use plan::{Payment, Plan, Witness};
use signature::{KeyPair, PublicKey, Signature};
use std::collections::hash_map::Entry::Occupied;
use std::collections::{HashMap, HashSet, VecDeque};
use std::result;
use std::sync::RwLock;
use transaction::Transaction;

const MAX_ENTRY_IDS: usize = 1024 * 4;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountingError {
    InsufficientFunds,
    InvalidTransferSignature,
}

pub type Result<T> = result::Result<T, AccountingError>;

/// Commit funds to the 'to' party.
fn apply_payment(balances: &RwLock<HashMap<PublicKey, RwLock<i64>>>, payment: &Payment) {
    if balances.read().unwrap().contains_key(&payment.to) {
        let bals = balances.read().unwrap();
        *bals[&payment.to].write().unwrap() += payment.tokens;
    } else {
        let mut bals = balances.write().unwrap();
        bals.insert(payment.to, RwLock::new(payment.tokens));
    }
}

pub struct Accountant {
    balances: RwLock<HashMap<PublicKey, RwLock<i64>>>,
    pending: RwLock<HashMap<Signature, Plan>>,
    last_ids: RwLock<VecDeque<(Hash, RwLock<HashSet<Signature>>)>>,
    time_sources: RwLock<HashSet<PublicKey>>,
    last_time: RwLock<DateTime<Utc>>,
}

impl Accountant {
    /// Create an Accountant using a deposit.
    pub fn new_from_deposit(deposit: &Payment) -> Self {
        let balances = RwLock::new(HashMap::new());
        apply_payment(&balances, deposit);
        Accountant {
            balances,
            pending: RwLock::new(HashMap::new()),
            last_ids: RwLock::new(VecDeque::new()),
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
        let acc = Self::new_from_deposit(&deposit);
        acc.register_entry_id(&mint.last_id());
        acc
    }

    fn reserve_signature(signatures: &RwLock<HashSet<Signature>>, sig: &Signature) -> bool {
        if signatures.read().unwrap().contains(sig) {
            return false;
        }
        signatures.write().unwrap().insert(*sig);
        true
    }

    fn reserve_signature_with_last_id(&self, sig: &Signature, last_id: &Hash) -> bool {
        if let Some(entry) = self.last_ids
            .read()
            .unwrap()
            .iter()
            .rev()
            .find(|x| x.0 == *last_id)
        {
            return Self::reserve_signature(&entry.1, sig);
        }
        false
    }

    /// Tell the accountant which Entry IDs exist on the ledger. This function
    /// assumes subsequent calls correspond to later entries, and will boot
    /// the oldest ones once its internal cache is full. Once boot, the
    /// accountant will reject transactions using that `last_id`.
    pub fn register_entry_id(&self, last_id: &Hash) {
        let mut last_ids = self.last_ids.write().unwrap();
        if last_ids.len() >= MAX_ENTRY_IDS {
            last_ids.pop_front();
        }
        last_ids.push_back((*last_id, RwLock::new(HashSet::new())));
    }

    /// Process a Transaction that has already been verified.
    pub fn process_verified_transaction(&self, tr: &Transaction) -> Result<()> {
        if self.get_balance(&tr.from).unwrap_or(0) < tr.data.tokens {
            return Err(AccountingError::InsufficientFunds);
        }

        if !self.reserve_signature_with_last_id(&tr.sig, &tr.data.last_id) {
            return Err(AccountingError::InvalidTransferSignature);
        }

        if let Some(x) = self.balances.read().unwrap().get(&tr.from) {
            *x.write().unwrap() -= tr.data.tokens;
        }

        let mut plan = tr.data.plan.clone();
        plan.apply_witness(&Witness::Timestamp(*self.last_time.read().unwrap()));

        if let Some(ref payment) = plan.final_payment() {
            apply_payment(&self.balances, payment);
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
                apply_payment(&self.balances, payment);
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
                apply_payment(&self.balances, payment);
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
        let bals = self.balances.read().unwrap();
        bals.get(pubkey).map(|x| *x.read().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::KeyPairUtil;
    use hash::hash;
    use bincode::serialize;

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
        assert!(acc.reserve_signature_with_last_id(&sig, &alice.last_id()));
        assert!(!acc.reserve_signature_with_last_id(&sig, &alice.last_id()));
    }

    #[test]
    fn test_max_entry_ids() {
        let alice = Mint::new(1);
        let acc = Accountant::new(&alice);
        let sig = Signature::default();
        for i in 0..MAX_ENTRY_IDS {
            let last_id = hash(&serialize(&i).unwrap()); // Unique hash
            acc.register_entry_id(&last_id);
        }
        // Assert we're no longer able to use the oldest entry ID.
        assert!(!acc.reserve_signature_with_last_id(&sig, &alice.last_id()));
    }
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use accountant::*;
    use rayon::prelude::*;
    use signature::KeyPairUtil;
    use hash::hash;
    use bincode::serialize;

    #[bench]
    fn process_verified_event_bench(bencher: &mut Bencher) {
        let mint = Mint::new(100_000_000);
        let acc = Accountant::new(&mint);
        // Create transactions between unrelated parties.
        let transactions: Vec<_> = (0..4096)
            .into_par_iter()
            .map(|i| {
                // Seed the 'from' account.
                let rando0 = KeyPair::new();
                let tr = Transaction::new(&mint.keypair(), rando0.pubkey(), 1_000, mint.last_id());
                acc.process_verified_transaction(&tr).unwrap();

                // Seed the 'to' account and a cell for its signature.
                let last_id = hash(&serialize(&i).unwrap()); // Unique hash
                acc.register_entry_id(&last_id);

                let rando1 = KeyPair::new();
                let tr = Transaction::new(&rando0, rando1.pubkey(), 1, last_id);
                acc.process_verified_transaction(&tr).unwrap();

                // Finally, return a transaction that's unique
                Transaction::new(&rando0, rando1.pubkey(), 1, last_id)
            })
            .collect();
        bencher.iter(|| {
            // Since benchmarker runs this multiple times, we need to clear the signatures.
            for (_, sigs) in acc.last_ids.read().unwrap().iter() {
                sigs.write().unwrap().clear();
            }

            transactions.par_iter().for_each(|tr| {
                acc.process_verified_transaction(tr).unwrap();
            });
        });
    }
}
