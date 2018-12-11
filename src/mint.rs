//! The `mint` module is a library for generating the chain's genesis block.

use crate::entry::Entry;
use ring::rand::SystemRandom;
use solana_sdk::hash::{hash, Hash};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
use untrusted::Input;

#[derive(Serialize, Deserialize, Debug)]
pub struct Mint {
    pub pkcs8: Vec<u8>,
    pubkey: Pubkey,
    pub tokens: u64,
    pub bootstrap_leader_id: Pubkey,
    pub bootstrap_leader_tokens: u64,
}

impl Mint {
    pub fn new_with_pkcs8(
        tokens: u64,
        pkcs8: Vec<u8>,
        bootstrap_leader_id: Pubkey,
        bootstrap_leader_tokens: u64,
    ) -> Self {
        let keypair =
            Keypair::from_pkcs8(Input::from(&pkcs8)).expect("from_pkcs8 in mint pub fn new");
        let pubkey = keypair.pubkey();
        Mint {
            pkcs8,
            pubkey,
            tokens,
            bootstrap_leader_id,
            bootstrap_leader_tokens,
        }
    }

    pub fn new_with_leader(
        tokens: u64,
        bootstrap_leader: Pubkey,
        bootstrap_leader_tokens: u64,
    ) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = Keypair::generate_pkcs8(&rnd)
            .expect("generate_pkcs8 in mint pub fn new")
            .to_vec();
        Self::new_with_pkcs8(tokens, pkcs8, bootstrap_leader, bootstrap_leader_tokens)
    }

    pub fn new(tokens: u64) -> Self {
        let rnd = SystemRandom::new();
        let pkcs8 = Keypair::generate_pkcs8(&rnd)
            .expect("generate_pkcs8 in mint pub fn new")
            .to_vec();
        Self::new_with_pkcs8(tokens, pkcs8, Pubkey::default(), 0)
    }

    pub fn seed(&self) -> Hash {
        hash(&self.pkcs8)
    }

    pub fn last_id(&self) -> Hash {
        self.create_entries().last().unwrap().id
    }

    pub fn keypair(&self) -> Keypair {
        Keypair::from_pkcs8(Input::from(&self.pkcs8)).expect("from_pkcs8 in mint pub fn keypair")
    }

    pub fn pubkey(&self) -> Pubkey {
        self.pubkey
    }

    pub fn create_transaction(&self) -> Vec<Transaction> {
        let keypair = self.keypair();
        // Check if creating the leader genesis entries is necessary
        if self.bootstrap_leader_id == Pubkey::default() {
            let tx = Transaction::system_move(&keypair, self.pubkey(), self.tokens, self.seed(), 0);
            vec![tx]
        } else {
            // Create moves from mint to itself (deposit), and then a move from the mint
            // to the bootstrap leader
            let moves = vec![
                (self.pubkey(), self.tokens),
                (self.bootstrap_leader_id, self.bootstrap_leader_tokens),
            ];
            vec![Transaction::system_move_many(
                &keypair,
                &moves,
                self.seed(),
                0,
            )]
        }
    }

    pub fn create_entries(&self) -> Vec<Entry> {
        let e0 = Entry::new(&self.seed(), 0, 0, vec![]);
        let e1 = Entry::new(&e0.id, 0, 1, self.create_transaction());
        let e2 = Entry::new(&e1.id, 0, 1, vec![]); // include a tick
        vec![e0, e1, e2]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Block;
    use bincode::deserialize;
    use solana_sdk::system_instruction::SystemInstruction;
    use solana_sdk::system_program;

    #[test]
    fn test_create_transactions() {
        let mut transactions = Mint::new(100).create_transaction().into_iter();
        let tx = transactions.next().unwrap();
        assert_eq!(tx.instructions.len(), 1);
        assert!(system_program::check_id(tx.program_id(0)));
        let instruction: SystemInstruction = deserialize(tx.userdata(0)).unwrap();
        if let SystemInstruction::Move { tokens } = instruction {
            assert_eq!(tokens, 100);
        }
        assert_eq!(transactions.next(), None);
    }

    #[test]
    fn test_create_leader_transactions() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let mut transactions = Mint::new_with_leader(100, dummy_leader_id, dummy_leader_tokens)
            .create_transaction()
            .into_iter();
        let tx = transactions.next().unwrap();
        assert_eq!(tx.instructions.len(), 2);
        assert!(system_program::check_id(tx.program_id(0)));
        assert!(system_program::check_id(tx.program_id(1)));
        let instruction: SystemInstruction = deserialize(tx.userdata(0)).unwrap();
        if let SystemInstruction::Move { tokens } = instruction {
            assert_eq!(tokens, 100);
        }
        let instruction: SystemInstruction = deserialize(tx.userdata(1)).unwrap();
        if let SystemInstruction::Move { tokens } = instruction {
            assert_eq!(tokens, 1);
        }
        assert_eq!(transactions.next(), None);
    }

    #[test]
    fn test_verify_entries() {
        let entries = Mint::new(100).create_entries();
        assert!(entries[..].verify(&entries[0].id));
    }

    #[test]
    fn test_verify_leader_entries() {
        let dummy_leader_id = Keypair::new().pubkey();
        let dummy_leader_tokens = 1;
        let entries =
            Mint::new_with_leader(100, dummy_leader_id, dummy_leader_tokens).create_entries();
        assert!(entries[..].verify(&entries[0].id));
    }
}
