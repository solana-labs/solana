//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use log::{verify_entry, Event, PublicKey, Sha256Hash};
use historian::Historian;
use ring::signature::Ed25519KeyPair;
use std::sync::mpsc::{RecvError, SendError};
use std::collections::HashMap;

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

    pub fn process_event(self: &mut Self, event: Event<u64>) {
        match event {
            Event::Claim { key, data, .. } => {
                if self.balances.contains_key(&key) {
                    if let Some(x) = self.balances.get_mut(&key) {
                        *x += data;
                    }
                } else {
                    self.balances.insert(key, data);
                }
            }
            Event::Transaction { from, to, data, .. } => {
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
            }
            _ => (),
        }
    }

    pub fn sync(self: &mut Self) {
        while let Ok(entry) = self.historian.receiver.try_recv() {
            assert!(verify_entry(&entry, &self.end_hash));
            self.end_hash = entry.end_hash;

            self.process_event(entry.event);
        }
    }

    pub fn deposit(
        self: &Self,
        n: u64,
        keypair: &Ed25519KeyPair,
    ) -> Result<(), SendError<Event<u64>>> {
        use log::sign_hash;
        let event = sign_hash(n, &keypair);
        self.historian.sender.send(event)
    }

    pub fn transfer(
        self: &mut Self,
        n: u64,
        keypair: &Ed25519KeyPair,
        pubkey: PublicKey,
    ) -> Result<(), SendError<Event<u64>>> {
        use log::transfer_hash;
        use generic_array::GenericArray;

        let sender_pubkey = GenericArray::clone_from_slice(keypair.public_key_bytes());
        if self.get_balance(&sender_pubkey).unwrap() >= n {
            let event = transfer_hash(n, keypair, pubkey);
            return self.historian.sender.send(event);
        }
        Ok(())
    }

    pub fn get_balance(self: &mut Self, pubkey: &PublicKey) -> Result<u64, RecvError> {
        self.sync();
        Ok(*self.balances.get(pubkey).unwrap_or(&0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;
    use log::generate_keypair;
    use historian::ExitReason;
    use generic_array::GenericArray;

    #[test]
    fn test_accountant() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        acc.deposit(1_000, &bob_keypair).unwrap();

        sleep(Duration::from_millis(30));
        let bob_pubkey = GenericArray::clone_from_slice(bob_keypair.public_key_bytes());
        acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();

        sleep(Duration::from_millis(30));
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_500);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_invalid_transfer() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        acc.deposit(1_000, &bob_keypair).unwrap();

        sleep(Duration::from_millis(30));
        let bob_pubkey = GenericArray::clone_from_slice(bob_keypair.public_key_bytes());
        acc.transfer(10_001, &alice_keypair, bob_pubkey).unwrap();

        sleep(Duration::from_millis(30));
        let alice_pubkey = GenericArray::clone_from_slice(alice_keypair.public_key_bytes());
        assert_eq!(acc.get_balance(&alice_pubkey).unwrap(), 10_000);
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 1_000);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }

    #[test]
    fn test_mulitple_claims() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let keypair = generate_keypair();
        acc.deposit(1, &keypair).unwrap();
        acc.deposit(2, &keypair).unwrap();

        let pubkey = GenericArray::clone_from_slice(keypair.public_key_bytes());
        sleep(Duration::from_millis(30));
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
        acc.deposit(10_000, &alice_keypair).unwrap();

        sleep(Duration::from_millis(30));
        let bob_pubkey = GenericArray::clone_from_slice(bob_keypair.public_key_bytes());
        acc.transfer(500, &alice_keypair, bob_pubkey).unwrap();

        sleep(Duration::from_millis(30));
        assert_eq!(acc.get_balance(&bob_pubkey).unwrap(), 500);

        drop(acc.historian.sender);
        assert_eq!(
            acc.historian.thread_hdl.join().unwrap().1,
            ExitReason::RecvDisconnected
        );
    }
}
