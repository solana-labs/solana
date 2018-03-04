//! A library for generating the chain's genesis block.

use event::{generate_keypair, get_pubkey, sign_transaction_data, Event, PublicKey};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Creator {
    pub pubkey: PublicKey,
    pub tokens: u64,
}

impl Creator {
    pub fn new(tokens: u64) -> Self {
        Creator {
            pubkey: get_pubkey(&generate_keypair()),
            tokens,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Genesis {
    pub pkcs8: Vec<u8>,
    pub tokens: u64,
    pub creators: Vec<Creator>,
}

impl Genesis {
    pub fn new(tokens: u64, creators: Vec<Creator>) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rnd).unwrap().to_vec();
        Genesis {
            pkcs8,
            tokens,
            creators,
        }
    }

    pub fn get_keypair(&self) -> Ed25519KeyPair {
        Ed25519KeyPair::from_pkcs8(Input::from(&self.pkcs8)).unwrap()
    }

    pub fn get_pubkey(&self) -> PublicKey {
        get_pubkey(&self.get_keypair())
    }

    pub fn create_transaction(&self, data: u64, to: &PublicKey) -> Event<u64> {
        let from = self.get_pubkey();
        let sig = sign_transaction_data(&data, &self.get_keypair(), to);
        Event::Transaction {
            from,
            to: *to,
            data,
            sig,
        }
    }

    pub fn create_events(&self) -> Vec<Event<u64>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_events() {
        let mut events = Genesis::new(100, vec![]).create_events().into_iter();
        assert_eq!(events.next().unwrap(), Event::Tick);
        if let Event::Transaction { from, to, .. } = events.next().unwrap() {
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
}
