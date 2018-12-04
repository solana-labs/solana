//! The `transaction` module provides functionality for creating log transactions.

use crate::hash::{Hash, Hasher};
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil, Signature};
use bincode::serialize;
use serde::Serialize;
use std::mem::size_of;

pub const SIG_OFFSET: usize = size_of::<u64>();

/// An instruction to execute a program
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Instruction {
    /// Index into the transaction program ids array indicating the program account that executes this instruction
    pub program_ids_index: u8,
    /// Ordered indices into the transaction keys array indicating which accounts to pass to the program
    pub accounts: Vec<u8>,
    /// The program input data
    pub userdata: Vec<u8>,
}

impl Instruction {
    pub fn new<T: Serialize>(program_ids_index: u8, userdata: &T, accounts: Vec<u8>) -> Self {
        let userdata = serialize(userdata).unwrap();
        Instruction {
            program_ids_index,
            userdata,
            accounts,
        }
    }
}

/// An atomic transaction
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction {
    /// A set of digital signatures of `account_keys`, `program_ids`, `last_id`, `fee` and `instructions`, signed by the first
    /// signatures.len() keys of account_keys
    pub signatures: Vec<Signature>,
    /// All the account keys used by this transaction
    pub account_keys: Vec<Pubkey>,
    /// The id of a recent ledger entry.
    pub last_id: Hash,
    /// The number of tokens paid for processing and storing of this transaction.
    pub fee: u64,
    /// All the program id keys used to execute this transaction's instructions
    pub program_ids: Vec<Pubkey>,
    /// Programs that will be executed in sequence and committed in one atomic transaction if all
    /// succeed.
    pub instructions: Vec<Instruction>,
}

impl Transaction {
    pub fn new<T: Serialize>(
        from_keypair: &Keypair,
        transaction_keys: &[Pubkey],
        program_id: Pubkey,
        userdata: &T,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        let program_ids = vec![program_id];
        let accounts = (0..=transaction_keys.len() as u8).collect();
        let instructions = vec![Instruction::new(0, userdata, accounts)];
        Self::new_with_instructions(
            &[from_keypair],
            transaction_keys,
            last_id,
            fee,
            program_ids,
            instructions,
        )
    }
    pub fn new_unsigned<T: Serialize>(
        transaction_keys: &[Pubkey],
        program_id: Pubkey,
        userdata: &T,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        let program_ids = vec![program_id];
        let accounts = (0..=transaction_keys.len() as u8).collect();
        let instructions = vec![Instruction::new(0, userdata, accounts)];
        Self::new_with_instructions(
            &[],
            transaction_keys,
            last_id,
            fee,
            program_ids,
            instructions,
        )
    }
    /// Create a signed transaction
    /// * `from_keypair` - The key used to sign the transaction.  This key is stored as keys[0]
    /// * `account_keys` - The keys for the transaction.  These are the program state
    ///    instances or token recipient keys.
    /// * `last_id` - The PoH hash.
    /// * `fee` - The transaction fee.
    /// * `program_ids` - The keys that identify programs used in the `instruction` vector.
    /// * `instructions` - The programs and their arguments that the transaction will execute atomically
    pub fn new_with_instructions(
        from_keypairs: &[&Keypair],
        keys: &[Pubkey],
        last_id: Hash,
        fee: u64,
        program_ids: Vec<Pubkey>,
        instructions: Vec<Instruction>,
    ) -> Self {
        let mut account_keys: Vec<_> = from_keypairs
            .iter()
            .map(|keypair| keypair.pubkey())
            .collect();
        account_keys.extend_from_slice(keys);
        let mut tx = Transaction {
            signatures: vec![],
            account_keys,
            last_id: Hash::default(),
            fee,
            program_ids,
            instructions,
        };
        tx.sign(from_keypairs, last_id);
        tx
    }
    pub fn userdata(&self, instruction_index: usize) -> &[u8] {
        &self.instructions[instruction_index].userdata
    }
    fn key_index(&self, instruction_index: usize, accounts_index: usize) -> Option<usize> {
        self.instructions
            .get(instruction_index)
            .and_then(|instruction| instruction.accounts.get(accounts_index))
            .map(|&account_keys_index| account_keys_index as usize)
    }
    pub fn key(&self, instruction_index: usize, accounts_index: usize) -> Option<&Pubkey> {
        self.key_index(instruction_index, accounts_index)
            .and_then(|account_keys_index| self.account_keys.get(account_keys_index))
    }
    pub fn signer_key(&self, instruction_index: usize, accounts_index: usize) -> Option<&Pubkey> {
        match self.key_index(instruction_index, accounts_index) {
            None => None,
            Some(signature_index) => {
                if signature_index >= self.signatures.len() {
                    return None;
                }
                self.account_keys.get(signature_index)
            }
        }
    }
    pub fn program_id(&self, instruction_index: usize) -> &Pubkey {
        let program_ids_index = self.instructions[instruction_index].program_ids_index;
        &self.program_ids[program_ids_index as usize]
    }
    /// Get the transaction data to sign.
    pub fn get_sign_data(&self) -> Vec<u8> {
        let mut data = serialize(&self.account_keys).expect("serialize account_keys");

        let last_id_data = serialize(&self.last_id).expect("serialize last_id");
        data.extend_from_slice(&last_id_data);

        let fee_data = serialize(&self.fee).expect("serialize fee");
        data.extend_from_slice(&fee_data);

        let program_ids = serialize(&self.program_ids).expect("serialize program_ids");
        data.extend_from_slice(&program_ids);

        let instructions = serialize(&self.instructions).expect("serialize instructions");
        data.extend_from_slice(&instructions);
        data
    }

    /// Sign this transaction.
    pub fn sign(&mut self, keypairs: &[&Keypair], last_id: Hash) {
        self.last_id = last_id;
        let sign_data = self.get_sign_data();
        self.signatures = keypairs
            .iter()
            .map(|keypair| Signature::new(&keypair.sign(&sign_data).as_ref()))
            .collect();
    }

    /// Verify only the transaction signature.
    pub fn verify_signature(&self) -> bool {
        self.signatures
            .iter()
            .all(|s| s.verify(&self.from().as_ref(), &self.get_sign_data()))
    }

    /// Verify that references in the instructions are valid
    pub fn verify_refs(&self) -> bool {
        for instruction in &self.instructions {
            if (instruction.program_ids_index as usize) >= self.program_ids.len() {
                return false;
            }
            for account_index in &instruction.accounts {
                if (*account_index as usize) >= self.account_keys.len() {
                    return false;
                }
            }
        }
        true
    }

    pub fn from(&self) -> &Pubkey {
        &self.account_keys[0]
    }

    // a hash of a slice of transactions only needs to hash the signatures
    pub fn hash(transactions: &[Transaction]) -> Hash {
        let mut hasher = Hasher::default();
        transactions
            .iter()
            .for_each(|tx| hasher.hash(&tx.signatures[0].as_ref()));
        hasher.result()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::serialize;

    #[test]
    fn test_refs() {
        let key = Keypair::new();
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let prog1 = Keypair::new().pubkey();
        let prog2 = Keypair::new().pubkey();
        let instructions = vec![
            Instruction::new(0, &(), vec![0, 1]),
            Instruction::new(1, &(), vec![0, 2]),
        ];
        let tx = Transaction::new_with_instructions(
            &[&key],
            &[key1, key2],
            Default::default(),
            0,
            vec![prog1, prog2],
            instructions,
        );
        assert!(tx.verify_refs());

        assert_eq!(tx.key(0, 0), Some(&key.pubkey()));
        assert_eq!(tx.signer_key(0, 0), Some(&key.pubkey()));

        assert_eq!(tx.key(1, 0), Some(&key.pubkey()));
        assert_eq!(tx.signer_key(1, 0), Some(&key.pubkey()));

        assert_eq!(tx.key(0, 1), Some(&key1));
        assert_eq!(tx.signer_key(0, 1), None);

        assert_eq!(tx.key(1, 1), Some(&key2));
        assert_eq!(tx.signer_key(1, 1), None);

        assert_eq!(tx.key(2, 0), None);
        assert_eq!(tx.signer_key(2, 0), None);

        assert_eq!(tx.key(0, 2), None);
        assert_eq!(tx.signer_key(0, 2), None);

        assert_eq!(*tx.program_id(0), prog1);
        assert_eq!(*tx.program_id(1), prog2);
    }
    #[test]
    fn test_refs_invalid_program_id() {
        let key = Keypair::new();
        let instructions = vec![Instruction::new(1, &(), vec![])];
        let tx = Transaction::new_with_instructions(
            &[&key],
            &[],
            Default::default(),
            0,
            vec![],
            instructions,
        );
        assert!(!tx.verify_refs());
    }
    #[test]
    fn test_refs_invalid_account() {
        let key = Keypair::new();
        let instructions = vec![Instruction::new(0, &(), vec![1])];
        let tx = Transaction::new_with_instructions(
            &[&key],
            &[],
            Default::default(),
            0,
            vec![Default::default()],
            instructions,
        );
        assert_eq!(*tx.program_id(0), Default::default());
        assert!(!tx.verify_refs());
    }

    /// Detect binary changes in the serialized transaction userdata, which could have a downstream
    /// affect on SDKs and DApps
    #[test]
    fn test_sdk_serialize() {
        use untrusted::Input;
        let keypair = Keypair::from_pkcs8(Input::from(&[
            48, 83, 2, 1, 1, 48, 5, 6, 3, 43, 101, 112, 4, 34, 4, 32, 255, 101, 36, 24, 124, 23,
            167, 21, 132, 204, 155, 5, 185, 58, 121, 75, 156, 227, 116, 193, 215, 38, 142, 22, 8,
            14, 229, 239, 119, 93, 5, 218, 161, 35, 3, 33, 0, 36, 100, 158, 252, 33, 161, 97, 185,
            62, 89, 99, 195, 250, 249, 187, 189, 171, 118, 241, 90, 248, 14, 68, 219, 231, 62, 157,
            5, 142, 27, 210, 117,
        ]))
        .unwrap();
        let to = Pubkey::new(&[
            1, 1, 1, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4,
            1, 1, 1,
        ]);

        let program_id = Pubkey::new(&[
            2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4,
            2, 2, 2,
        ]);

        let tx = Transaction::new(
            &keypair,
            &[keypair.pubkey(), to],
            program_id,
            &(1u8, 2u8, 3u8),
            Hash::default(),
            99,
        );
        assert_eq!(
            serialize(&tx).unwrap(),
            vec![
                1, 0, 0, 0, 0, 0, 0, 0, 213, 248, 255, 179, 219, 217, 130, 31, 27, 85, 33, 217, 62,
                28, 180, 204, 186, 141, 178, 150, 153, 184, 205, 87, 123, 128, 101, 254, 222, 111,
                152, 17, 153, 210, 169, 1, 81, 208, 254, 64, 229, 205, 145, 10, 213, 241, 255, 31,
                184, 52, 242, 148, 213, 131, 241, 165, 144, 181, 18, 4, 58, 171, 44, 11, 3, 0, 0,
                0, 0, 0, 0, 0, 36, 100, 158, 252, 33, 161, 97, 185, 62, 89, 99, 195, 250, 249, 187,
                189, 171, 118, 241, 90, 248, 14, 68, 219, 231, 62, 157, 5, 142, 27, 210, 117, 36,
                100, 158, 252, 33, 161, 97, 185, 62, 89, 99, 195, 250, 249, 187, 189, 171, 118,
                241, 90, 248, 14, 68, 219, 231, 62, 157, 5, 142, 27, 210, 117, 1, 1, 1, 4, 5, 6, 7,
                8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 1, 1, 1, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 99, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 4, 5, 6, 7, 8, 9, 1,
                1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4, 2, 2, 2, 1, 0, 0, 0, 0, 0,
                0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3
            ],
        );
    }
}
