//! The `mint` module is a library for generating the chain's genesis block.

use entry::Entry;
use hash::{hash, Hash};
use ring::rand::SystemRandom;
use signature::{Keypair, KeypairUtil};
use solana_program_interface::pubkey::Pubkey;
use system_transaction::SystemTransaction;
use transaction::Transaction;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Mint {
    pub pkcs8: Vec<u8>,
    pubkey: Pubkey,
    pub tokens: i64,
}

impl Mint {
    pub fn new_with_pkcs8(tokens: i64, pkcs8: Vec<u8>) -> Self {
        let keypair =
            Keypair::from_pkcs8(Input::from(&pkcs8)).expect("from_pkcs8 in mint pub fn new");
        let pubkey = keypair.pubkey();
        Mint {
            pkcs8,
            pubkey,
            tokens,
        }
    }

    pub fn new(tokens: i64) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = Keypair::generate_pkcs8(&rnd)
            .expect("generate_pkcs8 in mint pub fn new")
            .to_vec();
        Self::new_with_pkcs8(tokens, pkcs8)
    }

    pub fn seed(&self) -> Hash {
        hash(&self.pkcs8)
    }

    pub fn last_id(&self) -> Hash {
        self.create_entries()[1].id
    }

    pub fn keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.pkcs8)).expect("from_pkcs8 in mint pub fn keypair")
    }

    pub fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    pub fn create_transactions(&self) -> Vec<Transaction> {
        let keypair = self.keypair();
        let tx = Transaction::system_move(&keypair, self.pubkey(), self.tokens, self.seed(), 0);
        vec![tx]
    }

    pub fn create_entries(&self) -> Vec<Entry> {
        let e0 = Entry::new(&self.seed(), 0, vec![]);
        let e1 = Entry::new(&e0.id, 1, self.create_transactions());
        vec![e0, e1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::deserialize;
    use ledger::Block;
    use system_program::SystemProgram;

    #[test]
    fn test_create_transactions() {
        let mut transactions = Mint::new(100).create_transactions().into_iter();
        let tx = transactions.next().unwrap();
        assert_eq!(tx.instructions.len(), 1);
        assert!(SystemProgram::check_id(tx.program_id(0)));
        let instruction: SystemProgram = deserialize(tx.userdata(0)).unwrap();
        if let SystemProgram::Move { tokens } = instruction {
            assert_eq!(tokens, 100);
        }
        assert_eq!(transactions.next(), None);
    }

    #[test]
    fn test_verify_entries() {
        let entries = Mint::new(100).create_entries();
        assert!(entries[..].verify(&entries[0].id));
    }
}
