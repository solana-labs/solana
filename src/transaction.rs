//! The `transaction` crate provides functionality for creating log transactions.

use signature::{KeyPair, KeyPairUtil, PublicKey, Signature, SignatureUtil};
use serde::Serialize;
use bincode::serialize;
use hash::Hash;
use chrono::prelude::*;
use plan::{Action, Condition, Payment, Plan};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction<T> {
    pub from: PublicKey,
    pub plan: Plan<T>,
    pub asset: T,
    pub last_id: Hash,
    pub sig: Signature,
}

impl<T: Serialize + Clone + Eq> Transaction<T> {
    pub fn new(from_keypair: &KeyPair, to: PublicKey, asset: T, last_id: Hash) -> Self {
        let from = from_keypair.pubkey();
        let plan = Plan::Action(Action::Pay(Payment {
            asset: asset.clone(),
            to,
        }));
        let mut tr = Transaction {
            from,
            plan,
            asset,
            last_id,
            sig: Signature::default(),
        };
        tr.sign(from_keypair);
        tr
    }

    pub fn new_on_date(
        from_keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        asset: T,
        last_id: Hash,
    ) -> Self {
        let from = from_keypair.pubkey();
        let plan = Plan::Race(
            Box::new(Plan::After(
                Condition::Timestamp(dt),
                Box::new(Plan::Action(Action::Pay(Payment {
                    asset: asset.clone(),
                    to,
                }))),
            )),
            Box::new(Plan::After(
                Condition::Signature(from),
                Box::new(Plan::Action(Action::Pay(Payment {
                    asset: asset.clone(),
                    to: from,
                }))),
            )),
        );
        let mut tr = Transaction {
            from,
            plan,
            asset,
            last_id,
            sig: Signature::default(),
        };
        tr.sign(from_keypair);
        tr
    }

    fn get_sign_data(&self) -> Vec<u8> {
        serialize(&(&self.from, &self.plan, &self.asset, &self.last_id)).unwrap()
    }

    pub fn sign(&mut self, keypair: &KeyPair) {
        let sign_data = self.get_sign_data();
        self.sig = Signature::clone_from_slice(keypair.sign(&sign_data).as_ref());
    }

    pub fn verify(&self) -> bool {
        self.sig.verify(&self.from, &self.get_sign_data()) && self.plan.verify(&self.asset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};
    use hash::hash;

    #[test]
    fn test_claim() {
        let keypair = KeyPair::new();
        let asset = hash(b"hello, world");
        let zero = Hash::default();
        let tr0 = Transaction::new(&keypair, keypair.pubkey(), asset, zero);
        assert!(tr0.verify());
    }

    #[test]
    fn test_transfer() {
        let zero = Hash::default();
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
        let asset = hash(b"hello, world");
        let tr0 = Transaction::new(&keypair0, pubkey1, asset, zero);
        assert!(tr0.verify());
    }

    #[test]
    fn test_serialize_claim() {
        let plan = Plan::Action(Action::Pay(Payment {
            asset: 0,
            to: Default::default(),
        }));
        let claim0 = Transaction {
            from: Default::default(),
            plan,
            asset: 0u8,
            last_id: Default::default(),
            sig: Default::default(),
        };
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction<u8> = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }

    #[test]
    fn test_bad_event_signature() {
        let zero = Hash::default();
        let keypair = KeyPair::new();
        let pubkey = keypair.pubkey();
        let mut tr = Transaction::new(&keypair, pubkey, hash(b"hello, world"), zero);
        tr.sign(&keypair);
        tr.asset = hash(b"goodbye cruel world"); // <-- attack!
        assert!(!tr.verify());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let thief_keypair = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let asset = hash(b"hello, world");
        let mut tr = Transaction::new(&keypair0, pubkey1, asset, zero);
        tr.sign(&keypair0);
        if let Plan::Action(Action::Pay(ref mut payment)) = tr.plan {
            payment.to = thief_keypair.pubkey(); // <-- attack!
        };
        assert!(!tr.verify());
    }
}
