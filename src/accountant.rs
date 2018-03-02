//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use log::{Entry, Sha256Hash};
use event::{Event, PublicKey, Signature};
use historian::Historian;
use ring::signature::Ed25519KeyPair;
use std::sync::mpsc::SendError;
use std::collections::HashMap;
use std::result;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountingError {
    InsufficientFunds,
    InvalidEvent,
    SendError,
}

pub type Result<T> = result::Result<T, AccountingError>;

pub struct Accountant {
    pub historian: Historian<u64>,
    pub balances: HashMap<PublicKey, u64>,
    pub end_hash: Sha256Hash,
}

impl Accountant {
    pub fn new(start_hash: &Sha256Hash, ms_per_tick: Option<u64>) -> Self {
        let hist = Historian::<u64>::new(start_hash, ms_per_tick);
        Accountant {
            historian: hist,
            balances: HashMap::new(),
            end_hash: *start_hash,
        }
    }

    pub fn sync(self: &mut Self) -> Vec<Entry<u64>> {
        let mut entries = vec![];
        while let Ok(entry) = self.historian.receiver.try_recv() {
            entries.push(entry);
        }

        if let Some(last_entry) = entries.last() {
            self.end_hash = last_entry.end_hash;
        }

        entries
    }

    pub fn deposit_signed(self: &mut Self, to: PublicKey, data: u64, sig: Signature) -> Result<()> {
        let event = Event::new_claim(to, data, sig);
        if !self.historian.verify_event(&event) {
            return Err(AccountingError::InvalidEvent);
        }
        if let Err(SendError(_)) = self.historian.sender.send(event) {
            return Err(AccountingError::SendError);
        }

        if self.balances.contains_key(&to) {
            if let Some(x) = self.balances.get_mut(&to) {
                *x += data;
            }
        } else {
            self.balances.insert(to, data);
        }

        Ok(())
    }

    pub fn deposit(self: &mut Self, n: u64, keypair: &Ed25519KeyPair) -> Result<Signature> {
        use event::{get_pubkey, sign_claim_data};
        let key = get_pubkey(keypair);
        let sig = sign_claim_data(&n, keypair);
        self.deposit_signed(key, n, sig).map(|_| sig)
    }

    pub fn transfer_signed(
        self: &mut Self,
        from: PublicKey,
        to: PublicKey,
        data: u64,
        sig: Signature,
    ) -> Result<()> {
        if self.get_balance(&from).unwrap_or(0) < data {
            return Err(AccountingError::InsufficientFunds);
        }

        let event = Event::Transaction {
            from: Some(from),
            to,
            data,
            sig,
        };
        if !self.historian.verify_event(&event) {
            return Err(AccountingError::InvalidEvent);
        }
        if let Err(SendError(_)) = self.historian.sender.send(event) {
            return Err(AccountingError::SendError);
        }

        if let Some(x) = self.balances.get_mut(&from) {
            *x -= data;
        }

        if self.balances.contains_key(&to) {
            if let Some(x) = self.balances.get_mut(&to) {
                *x += data;
            }
        } else {
            self.balances.insert(to, data);
        }

        Ok(())
    }

    pub fn transfer(
        self: &mut Self,
        n: u64,
        keypair: &Ed25519KeyPair,
        to: PublicKey,
    ) -> Result<Signature> {
        use event::{get_pubkey, sign_transaction_data};
        let from = get_pubkey(keypair);
        let sig = sign_transaction_data(&n, keypair, &to);
        self.transfer_signed(from, to, n, sig).map(|_| sig)
    }

    pub fn get_balance(self: &Self, pubkey: &PublicKey) -> Option<u64> {
        self.balances.get(pubkey).map(|x| *x)
    }

    pub fn wait_on_signature(self: &mut Self, wait_sig: &Signature) {
        use std::thread::sleep;
        use std::time::Duration;
        let mut entries = self.sync();
        let mut found = false;
        while !found {
            found = entries.iter().any(|e| match e.event {
                Event::Claim { sig, .. } => sig == *wait_sig,
                Event::Transaction { sig, .. } => sig == *wait_sig,
                _ => false,
            });
            if !found {
                sleep(Duration::from_millis(30));
                entries = self.sync();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event::{generate_keypair, get_pubkey};
    use historian::ExitReason;

    #[test]
    fn test_accountant() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        let sig = acc.deposit(1_000, &bob_keypair).unwrap();
        acc.wait_on_signature(&sig);

        let bob_pubkey = get_pubkey(&bob_keypair);
        let sig = acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();
        acc.wait_on_signature(&sig);

        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_invalid_transfer() {
        use std::thread::sleep;
        use std::time::Duration;
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        let sig = acc.deposit(1_000, &bob_keypair).unwrap();
        acc.wait_on_signature(&sig);

        let bob_pubkey = get_pubkey(&bob_keypair);
        assert_eq!(
            acc.transfer(10_001, &alice_keypair, bob_pubkey),
            Err(AccountingError::InsufficientFunds)
        );
        sleep(Duration::from_millis(30));

        let alice_pubkey = get_pubkey(&alice_keypair);
        assert_eq!(acc.get_balance(&alice_pubkey).unwrap(), 10_000);
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_multiple_claims() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let keypair = generate_keypair();
        acc.deposit(1, &keypair).unwrap();
        let sig = acc.deposit(2, &keypair).unwrap();
        acc.wait_on_signature(&sig);

        let pubkey = get_pubkey(&keypair);
        assert_eq!(acc.get_balance(&pubkey).unwrap(), 3);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_transfer_to_newb() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        let sig = acc.deposit(10_000, &alice_keypair).unwrap();
        acc.wait_on_signature(&sig);

        let bob_pubkey = get_pubkey(&bob_keypair);
        let sig = acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();
        acc.wait_on_signature(&sig);
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 500);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }
}
