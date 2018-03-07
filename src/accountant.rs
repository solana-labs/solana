//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use hash::Hash;
use entry::Entry;
use event::Event;
use transaction::Transaction;
use signature::{KeyPair, PublicKey, Signature};
use genesis::Genesis;
use historian::{reserve_signature, Historian};
use std::sync::mpsc::SendError;
use std::collections::HashMap;
use std::result;

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
        };

        // The second item in the log is a special transaction where the to and from
        // fields are the same. That entry should be treated as a deposit, not a
        // transfer to oneself.
        let entry1 = entries.next().unwrap();
        acc.process_verified_event(&entry1.event, true).unwrap();

        for entry in entries {
            acc.process_verified_event(&entry.event, false).unwrap();
        }
        acc
    }

    pub fn new(gen: &Genesis, ms_per_tick: Option<u64>) -> Self {
        Self::new_from_entries(gen.create_entries(), ms_per_tick)
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

    fn process_verified_transaction(
        self: &mut Self,
        tr: &Transaction<i64>,
        allow_deposits: bool,
    ) -> Result<()> {
        if !reserve_signature(&mut self.historian.signatures, &tr.sig) {
            return Err(AccountingError::InvalidTransferSignature);
        }

        if !Self::is_deposit(allow_deposits, &tr.from, &tr.to) {
            if let Some(x) = self.balances.get_mut(&tr.from) {
                *x -= tr.asset;
            }
        }

        if self.balances.contains_key(&tr.to) {
            if let Some(x) = self.balances.get_mut(&tr.to) {
                *x += tr.asset;
            }
        } else {
            self.balances.insert(tr.to, tr.asset);
        }

        Ok(())
    }

    fn process_verified_event(self: &mut Self, event: &Event, allow_deposits: bool) -> Result<()> {
        match *event {
            Event::Tick => Ok(()),
            Event::Transaction(ref tr) => self.process_verified_transaction(tr, allow_deposits),
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
        let alice = Genesis::new(10_000);
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
        let alice = Genesis::new(11_000);
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
        let alice = Genesis::new(10_000);
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
}
