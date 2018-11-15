//! The `transaction` module provides functionality for creating log transactions.

use bincode::serialize;
use hash::{Hash, Hasher};
use serde::Serialize;
use signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::pubkey::Pubkey;
use std::mem::size_of;

pub const SIG_OFFSET: usize = size_of::<u64>();

/// An instruction to execute a program under the `program_id` of `program_ids_index` with the
/// specified accounts and userdata
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Instruction {
    /// The program code that executes this transaction is identified by the program_id.
    /// this is an offset into the Transaction::program_ids field
    pub program_ids_index: u8,
    /// Indices into the keys array of which accounts to load
    pub accounts: Vec<u8>,
    /// Userdata to be stored in the account
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
    /// A set of digital signature of `account_keys`, `program_ids`, `last_id`, `fee` and `instructions`, signed by the first
    /// signatures.len() keys of account_keys
    pub signatures: Vec<Signature>,

    /// The `Pubkeys` that are executing this transaction userdata.  The meaning of each key is
    /// program-specific.
    /// * account_keys[0] - Typically this is the `caller` public key.  `signature` is verified with account_keys[0].
    /// In the future which key pays the fee and which keys have signatures would be configurable.
    /// * account_keys[1] - Typically this is the program context or the recipient of the tokens
    pub account_keys: Vec<Pubkey>,

    /// The ID of a recent ledger entry.
    pub last_id: Hash,

    /// The number of tokens paid for processing and storage of this transaction.
    pub fee: u64,

    /// Keys identifying programs in the instructions vector.
    pub program_ids: Vec<Pubkey>,
    /// Programs that will be executed in sequence and commited in one atomic transaction if all
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
    pub fn signed_key(&self, instruction_index: usize, accounts_index: usize) -> Option<&Pubkey> {
        match self.key_index(instruction_index, accounts_index) {
            None => None,
            Some(0) => self.account_keys.get(0),
            Some(_) => None,
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
        warn!("transaction signature verification called");
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
    use signature::GenKeys;

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
        assert_eq!(tx.signed_key(0, 0), Some(&key.pubkey()));

        assert_eq!(tx.key(1, 0), Some(&key.pubkey()));
        assert_eq!(tx.signed_key(1, 0), Some(&key.pubkey()));

        assert_eq!(tx.key(0, 1), Some(&key1));
        assert_eq!(tx.signed_key(0, 1), None);

        assert_eq!(tx.key(1, 1), Some(&key2));
        assert_eq!(tx.signed_key(1, 1), None);

        assert_eq!(tx.key(2, 0), None);
        assert_eq!(tx.signed_key(2, 0), None);

        assert_eq!(tx.key(0, 2), None);
        assert_eq!(tx.signed_key(0, 2), None);

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

    /// Detect binary changes in the serialized contract userdata, which could have a downstream
    /// affect on SDKs and DApps
    #[test]
    fn test_sdk_serialize() {
        let keypair = &GenKeys::new([0u8; 32]).gen_n_keypairs(1)[0];
        let to = Pubkey::new(&[
            1, 1, 1, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4,
            1, 1, 1,
        ]);

        let program_id = Pubkey::new(&[
            2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4,
            2, 2, 2,
        ]);

        let tx = Transaction::new(
            keypair,
            &[keypair.pubkey(), to],
            program_id,
            &(1u8, 2u8, 3u8),
            Hash::default(),
            99,
        );
        assert_eq!(
            serialize(&tx).unwrap(),
            vec![
                1, 0, 0, 0, 0, 0, 0, 0, 199, 111, 28, 46, 13, 235, 122, 203, 210, 118, 158, 0, 59,
                175, 148, 175, 225, 134, 90, 155, 188, 232, 218, 54, 199, 116, 67, 99, 65, 84, 164,
                94, 224, 60, 187, 249, 92, 210, 242, 99, 58, 121, 202, 5, 140, 152, 14, 166, 13,
                134, 103, 127, 216, 96, 217, 139, 5, 252, 40, 146, 48, 112, 59, 7, 1, 0, 0, 0, 0,
                0, 0, 0, 32, 253, 186, 201, 177, 11, 117, 135, 187, 167, 181, 188, 22, 59, 206,
                105, 231, 150, 215, 30, 78, 212, 76, 16, 252, 180, 72, 134, 137, 247, 161, 68, 3,
                0, 0, 0, 0, 0, 0, 0, 32, 253, 186, 201, 177, 11, 117, 135, 187, 167, 181, 188, 22,
                59, 206, 105, 231, 150, 215, 30, 78, 212, 76, 16, 252, 180, 72, 134, 137, 247, 161,
                68, 32, 253, 186, 201, 177, 11, 117, 135, 187, 167, 181, 188, 22, 59, 206, 105,
                231, 150, 215, 30, 78, 212, 76, 16, 252, 180, 72, 134, 137, 247, 161, 68, 1, 1, 1,
                4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 1, 1,
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 99, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 4, 5, 6,
                7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4, 2, 2, 2, 1, 0,
                0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 0, 0, 0, 0, 0, 0, 0, 1, 2,
                3
            ],
        );
    }
}
