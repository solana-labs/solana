//! A library for generating the chain's genesis block.

use event::{generate_keypair, get_pubkey, sign_claim_data, sign_transaction_data, Event, PublicKey};
use ring::rand::SystemRandom;
use ring::signature::Ed25519KeyPair;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Creator {
    pub name: String,
    pub pubkey: PublicKey,
    pub tokens: u64,
}

impl Creator {
    pub fn new(name: &str, tokens: u64) -> Self {
        Creator {
            name: name.to_string(),
            pubkey: get_pubkey(&generate_keypair()),
            tokens,
        }
    }

    pub fn create_transaction(&self, keypair: &Ed25519KeyPair) -> Event<u64> {
        let from = Some(get_pubkey(keypair));
        let to = self.pubkey;
        let data = self.tokens;
        let sig = sign_transaction_data(&data, keypair, &to);
        Event::Transaction {
            from,
            to,
            data,
            sig,
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

    pub fn create_events(&self) -> Vec<Event<u64>> {
        let org_keypair = Ed25519KeyPair::from_pkcs8(Input::from(&self.pkcs8)).unwrap();
        let sig = sign_claim_data(&self.tokens, &org_keypair);
        let event0 = Event::Tick;
        let event1 = Event::new_claim(get_pubkey(&org_keypair), self.tokens, sig);
        let mut events = vec![event0, event1];

        for creator in &self.creators {
            let tx = creator.create_transaction(&org_keypair);
            events.push(tx);
        }

        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use event::verify_event;

    #[test]
    fn test_creator_transaction() {
        assert!(verify_event(&Creator::new("Satoshi", 42)
            .create_transaction(&generate_keypair())));
    }

    #[test]
    fn test_create_events() {
        assert_eq!(Genesis::new(100, vec![]).create_events().len(), 2);
        assert_eq!(
            Genesis::new(100, vec![Creator::new("Satoshi", 42)])
                .create_events()
                .len(),
            3
        );
    }
}
