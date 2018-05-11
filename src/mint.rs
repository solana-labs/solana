//! The `mint` module is a library for generating the chain's genesis block.

use entry::create_entry;
use entry::Entry;
use event::Event;
use hash::{hash, Hash};
use ring::rand::SystemRandom;
use signature::{KeyPair, KeyPairUtil, PublicKey};
use transaction::Transaction;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Mint {
    pub pkcs8: Vec<u8>,
    pubkey: PublicKey,
    pub tokens: i64,
}

impl Mint {
    pub fn new(tokens: i64) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = KeyPair::generate_pkcs8(&rnd).expect("generate_pkcs8 in mint pub fn new").to_vec();
        let keypair = KeyPair::from_pkcs8(Input::from(&pkcs8)).expect("from_pkcs8 in mint pub fn new");
        let pubkey = keypair.pubkey();
        Mint {
            pkcs8,
            pubkey,
            tokens,
        }
    }

    pub fn seed(&self) -> Hash {
        hash(&self.pkcs8)
    }

    pub fn last_id(&self) -> Hash {
        self.create_entries()[1].id
    }

    pub fn keypair(&self) -> KeyPair {
        KeyPair::from_pkcs8(Input::from(&self.pkcs8)).expect("from_pkcs8 in mint pub fn keypair")
    }

    pub fn pubkey(&self) -> PublicKey {
        self.pubkey
    }

    pub fn create_events(&self) -> Vec<Event> {
        let keypair = self.keypair();
        let tr = Transaction::new(&keypair, self.pubkey(), self.tokens, self.seed());
        vec![Event::Transaction(tr)]
    }

    pub fn create_entries(&self) -> Vec<Entry> {
        let e0 = create_entry(&self.seed(), 0, vec![]);
        let e1 = create_entry(&e0.id, 0, self.create_events());
        vec![e0, e1]
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MintDemo {
    pub mint: Mint,
    pub users: Vec<(Vec<u8>, i64)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ledger::Block;
    use plan::Plan;

    #[test]
    fn test_create_events() {
        let mut events = Mint::new(100).create_events().into_iter();
        if let Event::Transaction(tr) = events.next().unwrap() {
            if let Plan::Pay(payment) = tr.data.plan {
                assert_eq!(tr.from, payment.to);
            }
        }
        assert_eq!(events.next(), None);
    }

    #[test]
    fn test_verify_entries() {
        let entries = Mint::new(100).create_entries();
        assert!(entries[..].verify(&entries[0].id));
    }
}
