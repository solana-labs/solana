//! A library for generating the chain's genesis block.

use event::Event;
use transaction::Transaction;
use signature::{generate_keypair, get_pubkey, PublicKey};
use entry::Entry;
use log::create_entries;
use hash::{hash, Hash};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Creator {
    pub pubkey: PublicKey,
    pub tokens: i64,
}

impl Creator {
    pub fn new(tokens: i64) -> Self {
        Creator {
            pubkey: get_pubkey(&generate_keypair()),
            tokens,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Genesis {
    pub pkcs8: Vec<u8>,
    pub tokens: i64,
    pub creators: Vec<Creator>,
}

impl Genesis {
    pub fn new(tokens: i64, creators: Vec<Creator>) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rnd).unwrap().to_vec();
        println!("{:?}", pkcs8);
        Genesis {
            pkcs8,
            tokens,
            creators,
        }
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

    pub fn create_transaction(&self, asset: i64, to: &PublicKey) -> Event {
        let tr = Transaction::new(&self.get_keypair(), *to, asset, self.get_seed());
        Event::Transaction(tr)
    }

    pub fn create_events(&self) -> Vec<Event> {
        let pubkey = self.get_pubkey();
        let event0 = Event::Tick;
        let event1 = self.create_transaction(self.tokens, &pubkey);
        let mut events = vec![event0, event1];

        for x in &self.creators {
            let tx = self.create_transaction(x.tokens, &x.pubkey);
            events.push(tx);
        }

        events
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
        let mut events = Genesis::new(100, vec![]).create_events().into_iter();
        assert_eq!(events.next().unwrap(), Event::Tick);
        if let Event::Transaction(Transaction { from, to, .. }) = events.next().unwrap() {
            assert_eq!(from, to);
        } else {
            assert!(false);
        }
        assert_eq!(events.next(), None);
    }

    #[test]
    fn test_create_creator() {
        assert_eq!(
            Genesis::new(100, vec![Creator::new(42)])
                .create_events()
                .len(),
            3
        );
    }

    #[test]
    fn test_verify_entries() {
        let entries = Genesis::new(100, vec![]).create_entries();
        assert!(verify_slice(&entries, &entries[0].id));
    }

    #[test]
    fn test_verify_entries_with_transactions() {
        let entries = Genesis::new(100, vec![Creator::new(42)]).create_entries();
        assert!(verify_slice(&entries, &entries[0].id));
    }
}
