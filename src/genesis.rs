//! A library for generating the chain's genesis block.

use event::Event;
use transaction::Transaction;
use signature::{get_pubkey, PublicKey};
use entry::Entry;
use log::create_entries;
use hash::{hash, Hash};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Genesis {
    pub pkcs8: Vec<u8>,
    pub tokens: i64,
}

impl Genesis {
    pub fn new(tokens: i64) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rnd).unwrap().to_vec();
        Genesis { pkcs8, tokens }
    }

    pub fn get_seed(&self) -> Hash {
        hash(&self.pkcs8)
    }

    pub fn get_keypair(&self) -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(Input::from(&self.pkcs8)).unwrap()
    }

    pub fn get_pubkey(&self) -> PublicKey {
        get_pubkey(&self.get_keypair())
    }

    pub fn create_events(&self) -> Vec<Event> {
        let tr = Transaction::new(
            &self.get_keypair(),
            self.get_pubkey(),
            self.tokens,
            self.get_seed(),
        );
        vec![Event::Tick, Event::Transaction(tr)]
    }

    pub fn create_entries(&self) -> Vec<Entry> {
        create_entries(&self.get_seed(), self.create_events())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::verify_slice;

    #[test]
    fn test_create_events() {
        let mut events = Genesis::new(100).create_events().into_iter();
        assert_eq!(events.next().unwrap(), Event::Tick);
        if let Event::Transaction(tr) = events.next().unwrap() {
            assert_eq!(tr.from, tr.to);
        } else {
            assert!(false);
        }
        assert_eq!(events.next(), None);
    }

    #[test]
    fn test_verify_entries() {
        let entries = Genesis::new(100).create_entries();
        assert!(verify_slice(&entries, &entries[0].id));
    }
}
