//! The `accountant` is a client of the `historian`. It uses the historian's
//! event log to record transactions. Its users can deposit funds and
//! transfer funds to other users.

use log::{Event, PublicKey, Sha256Hash, Signature};
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

    pub fn process_event(self: &mut Self, event: &Event<u64>) {
        match *event {
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
        let mut entries = vec![];
        while let Ok(entry) = self.historian.receiver.try_recv() {
            entries.push(entry);
        }
        // TODO: Does this cause the historian's channel to get blocked?
        //use log::verify_slice_u64;
        //println!("accountant: verifying {} entries...", entries.len());
        //assert!(verify_slice_u64(&entries, &self.end_hash));
        //println!("accountant: Done verifying {} entries.", entries.len());
        if let Some(last_entry) = entries.last() {
            self.end_hash = last_entry.end_hash;
        }
        for e in &entries {
            self.process_event(&e.event);
        }
    }

    pub fn deposit_signed(
        self: &Self,
        key: PublicKey,
        data: u64,
        sig: Signature,
    ) -> Result<(), SendError<Event<u64>>> {
        let event = Event::Claim { key, data, sig };
        self.historian.sender.send(event)
    }

    pub fn deposit(
        self: &Self,
        n: u64,
        keypair: &Ed25519KeyPair,
    ) -> Result<(), SendError<Event<u64>>> {
        use log::{get_pubkey, sign_serialized};
        let key = get_pubkey(keypair);
        let sig = sign_serialized(&n, keypair);
        self.deposit_signed(key, n, sig)
    }

    pub fn transfer_signed(
        self: &mut Self,
        from: PublicKey,
        to: PublicKey,
        data: u64,
        sig: Signature,
    ) -> Result<(), SendError<Event<u64>>> {
        if self.get_balance(&from).unwrap() < data {
            // TODO: Replace the SendError result with a custom one.
            println!("Error: Insufficient funds");
            return Ok(());
        }
        let event = Event::Transaction {
            from,
            to,
            data,
            sig,
        };
        self.historian.sender.send(event)
    }

    pub fn transfer(
        self: &mut Self,
        n: u64,
        keypair: &Ed25519KeyPair,
        to: PublicKey,
    ) -> Result<(), SendError<Event<u64>>> {
        use log::{get_pubkey, sign_transaction_data};

        let from = get_pubkey(keypair);
        let sig = sign_transaction_data(&n, keypair, &to);
        self.transfer_signed(from, to, n, sig)
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
    use log::{generate_keypair, get_pubkey};
    use historian::ExitReason;

    #[test]
    fn test_accountant() {
        let zero = Sha256Hash::default();
        let mut acc = Accountant::new(&zero, Some(2));
        let alice_keypair = generate_keypair();
        let bob_keypair = generate_keypair();
        acc.deposit(10_000, &alice_keypair).unwrap();
        acc.deposit(1_000, &bob_keypair).unwrap();

        sleep(Duration::from_millis(30));
        let bob_pubkey = get_pubkey(&bob_keypair);
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
        let bob_pubkey = get_pubkey(&bob_keypair);
        acc.transfer(10_001, &alice_keypair, bob_pubkey).unwrap();

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
        acc.deposit(2, &keypair).unwrap();

        let pubkey = get_pubkey(&keypair);
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
        let bob_pubkey = get_pubkey(&bob_keypair);
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
