//! A library for generating the chain's genesis block.

use event::Event;
use transaction::Transaction;
use signature::{KeyPair, KeyPairUtil, PublicKey};
use entry::Entry;
use log::create_entries;
use hash::{hash, Hash};
use ring::rand::SystemRandom;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Genesis {
    pub pkcs8: Vec<u8>,
    pub tokens: i64,
}

impl Genesis {
    pub fn new(tokens: i64) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = KeyPair::generate_pkcs8(&rnd).unwrap().to_vec();
        Genesis { pkcs8, tokens }
    }

    pub fn seed(&self) -> Hash {
        hash(&self.pkcs8)
    }

    pub fn keypair(&self) -> KeyPair {
        KeyPair::from_pkcs8(Input::from(&self.pkcs8)).unwrap()
    }

    pub fn pubkey(&self) -> PublicKey {
        self.keypair().pubkey()
    }

    pub fn create_events(&self) -> Vec<Event> {
        let tr = Transaction::new(&self.keypair(), self.pubkey(), self.tokens, self.seed());
        vec![Event::Tick, Event::Transaction(tr)]
    }

    pub fn create_entries(&self) -> Vec<Entry> {
        create_entries(&self.seed(), self.create_events())
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
