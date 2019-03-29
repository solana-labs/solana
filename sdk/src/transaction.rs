//! Defines a Transaction type to package an atomic sequence of instructions.

use crate::hash::Hash;
use crate::instruction::{CompiledInstruction, Instruction, InstructionError};
use crate::message::Message;
use crate::pubkey::Pubkey;
use crate::short_vec;
use crate::signature::{KeypairUtil, Signature};
use bincode::serialize;

/// Reasons a transaction might be rejected.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum TransactionError {
    /// This Pubkey is being processed in another transaction
    AccountInUse,

    /// Pubkey appears twice in the same transaction, typically in a pay-to-self
    /// transaction.
    AccountLoadedTwice,

    /// Attempt to debit from `Pubkey`, but no found no record of a prior credit.
    AccountNotFound,

    /// The from `Pubkey` does not have sufficient balance to pay the fee to schedule the transaction
    InsufficientFundsForFee,

    /// The bank has seen `Signature` before. This can occur under normal operation
    /// when a UDP packet is duplicated, as a user error from a client not updating
    /// its `recent_blockhash`, or as a double-spend attack.
    DuplicateSignature,

    /// The bank has not seen the given `recent_blockhash` or the transaction is too old and
    /// the `recent_blockhash` has been discarded.
    BlockhashNotFound,

    /// The program returned an error
    InstructionError(u8, InstructionError),

    /// Loader call chain too deep
    CallChainTooDeep,

    /// Transaction has a fee but has no signature present
    MissingSignatureForFee,

    /// Transaction contains an invalid account reference
    InvalidAccountIndex,
}

/// An atomic transaction
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// A set of digital signatures of `account_keys`, `program_ids`, `recent_blockhash`, `fee` and `instructions`, signed by the first
    /// signatures.len() keys of account_keys
    #[serde(with = "short_vec")]
    pub signatures: Vec<Signature>,
    /// All the account keys used by this transaction

    #[serde(with = "short_vec")]
    pub account_keys: Vec<Pubkey>,

    /// The id of a recent ledger entry.
    pub recent_blockhash: Hash,

    /// The number of lamports paid for processing and storing of this transaction.
    pub fee: u64,

    /// All the program id keys used to execute this transaction's instructions
    #[serde(with = "short_vec")]
    pub program_ids: Vec<Pubkey>,

    /// Programs that will be executed in sequence and committed in one atomic transaction if all
    /// succeed.
    #[serde(with = "short_vec")]
    pub instructions: Vec<CompiledInstruction>,
}

impl Transaction {
    pub fn new_unsigned(message: Message) -> Self {
        Self {
            signatures: Vec::with_capacity(message.num_signatures as usize),
            account_keys: message.account_keys,
            recent_blockhash: message.recent_blockhash,
            fee: message.fee,
            program_ids: message.program_ids,
            instructions: message.instructions,
        }
    }

    pub fn new_unsigned_instructions(instructions: Vec<Instruction>) -> Self {
        let message = Message::new(instructions);
        Self::new_unsigned(message)
    }

    pub fn new<T: KeypairUtil>(
        from_keypairs: &[&T],
        message: Message,
        recent_blockhash: Hash,
    ) -> Transaction {
        let mut tx = Self::new_unsigned(message);
        tx.sign(from_keypairs, recent_blockhash);
        tx
    }

    pub fn new_signed_instructions<T: KeypairUtil>(
        from_keypairs: &[&T],
        instructions: Vec<Instruction>,
        recent_blockhash: Hash,
        fee: u64,
    ) -> Transaction {
        let mut message = Message::new(instructions);
        message.fee = fee;
        Self::new(from_keypairs, message, recent_blockhash)
    }

    /// Create a signed transaction
    /// * `from_keypairs` - The keys used to sign the transaction.
    /// * `account_keys` - The keys for the transaction.  These are the program state
    ///    instances or lamport recipient keys.
    /// * `recent_blockhash` - The PoH hash.
    /// * `fee` - The transaction fee.
    /// * `program_ids` - The keys that identify programs used in the `instruction` vector.
    /// * `instructions` - Instructions that will be executed atomically.
    pub fn new_with_compiled_instructions<T: KeypairUtil>(
        from_keypairs: &[&T],
        keys: &[Pubkey],
        recent_blockhash: Hash,
        fee: u64,
        program_ids: Vec<Pubkey>,
        instructions: Vec<CompiledInstruction>,
    ) -> Self {
        let mut account_keys: Vec<_> = from_keypairs
            .iter()
            .map(|keypair| keypair.pubkey())
            .collect();
        account_keys.extend_from_slice(keys);
        let mut tx = Transaction {
            signatures: Vec::with_capacity(from_keypairs.len()),
            account_keys,
            recent_blockhash: Hash::default(),
            fee,
            program_ids,
            instructions,
        };
        tx.sign(from_keypairs, recent_blockhash);
        tx
    }

    pub fn data(&self, instruction_index: usize) -> &[u8] {
        &self.instructions[instruction_index].data
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

    /// Return a message containing all data that should be signed.
    pub fn message(&self) -> Message {
        Message {
            num_signatures: self.signatures.len() as u8,
            account_keys: self.account_keys.clone(),
            recent_blockhash: self.recent_blockhash,
            fee: self.fee,
            program_ids: self.program_ids.clone(),
            instructions: self.instructions.clone(),
        }
    }

    /// Return the serialized message data to sign.
    pub fn message_data(&self) -> Vec<u8> {
        serialize(&self.message()).unwrap()
    }

    /// Sign this transaction.
    pub fn sign_unchecked<T: KeypairUtil>(&mut self, keypairs: &[&T], recent_blockhash: Hash) {
        self.recent_blockhash = recent_blockhash;
        let message_data = self.message_data();
        self.signatures = keypairs
            .iter()
            .map(|keypair| keypair.sign_message(&message_data))
            .collect();
    }

    /// Check keys and keypair lengths, then sign this transaction.
    /// Note: this presumes signatures.capacity() was set to the number of required signatures.
    pub fn sign<T: KeypairUtil>(&mut self, keypairs: &[&T], recent_blockhash: Hash) {
        let signed_keys = &self.account_keys[0..self.signatures.capacity()];
        for (i, keypair) in keypairs.iter().enumerate() {
            assert_eq!(keypair.pubkey(), signed_keys[i], "keypair-pubkey mismatch");
        }
        assert_eq!(keypairs.len(), signed_keys.len(), "not enough keypairs");

        self.sign_unchecked(keypairs, recent_blockhash);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruction::AccountMeta;
    use crate::signature::Keypair;
    use crate::system_instruction::SystemInstruction;
    use bincode::{deserialize, serialize, serialized_size};
    use std::mem::size_of;

    #[test]
    fn test_refs() {
        let key = Keypair::new();
        let key1 = Keypair::new().pubkey();
        let key2 = Keypair::new().pubkey();
        let prog1 = Keypair::new().pubkey();
        let prog2 = Keypair::new().pubkey();
        let instructions = vec![
            CompiledInstruction::new(0, &(), vec![0, 1]),
            CompiledInstruction::new(1, &(), vec![0, 2]),
        ];
        let tx = Transaction::new_with_compiled_instructions(
            &[&key],
            &[key1, key2],
            Hash::default(),
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
        let instructions = vec![CompiledInstruction::new(1, &(), vec![])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&key],
            &[],
            Hash::default(),
            0,
            vec![],
            instructions,
        );
        assert!(!tx.verify_refs());
    }
    #[test]
    fn test_refs_invalid_account() {
        let key = Keypair::new();
        let instructions = vec![CompiledInstruction::new(0, &(), vec![1])];
        let tx = Transaction::new_with_compiled_instructions(
            &[&key],
            &[],
            Hash::default(),
            0,
            vec![Pubkey::default()],
            instructions,
        );
        assert_eq!(*tx.program_id(0), Pubkey::default());
        assert!(!tx.verify_refs());
    }

    fn create_sample_transaction() -> Transaction {
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
        let account_metas = vec![
            AccountMeta::new(keypair.pubkey(), true),
            AccountMeta::new(to, false),
        ];
        let instruction = Instruction::new(program_id, &(1u8, 2u8, 3u8), account_metas);
        let mut message = Message::new(vec![instruction]);
        message.fee = 99;
        Transaction::new(&[&keypair], message, Hash::default())
    }

    #[test]
    fn test_transaction_serialize() {
        let tx = create_sample_transaction();
        let ser = serialize(&tx).unwrap();
        let deser = deserialize(&ser).unwrap();
        assert_eq!(tx, deser);
    }

    /// Detect changes to the serialized size of payment transactions, which affects TPS.
    #[test]
    fn test_transaction_minimum_serialized_size() {
        let alice_keypair = Keypair::new();
        let alice_pubkey = alice_keypair.pubkey();
        let bob_pubkey = Keypair::new().pubkey();
        let ix = SystemInstruction::new_move(&alice_pubkey, &bob_pubkey, 42);

        let expected_data_size = size_of::<u32>() + size_of::<u64>();
        assert_eq!(expected_data_size, 12);
        assert_eq!(
            ix.data.len(),
            expected_data_size,
            "unexpected system instruction size"
        );

        let expected_instruction_size = 1 + 1 + ix.accounts.len() + 1 + expected_data_size;
        assert_eq!(expected_instruction_size, 17);

        let message = Message::new(vec![ix]);
        assert_eq!(
            serialized_size(&message.instructions[0]).unwrap() as usize,
            expected_instruction_size,
            "unexpected Instruction::serialized_size"
        );

        let tx = Transaction::new(&[&alice_keypair], message, Hash::default());

        let expected_transaction_size = 1
            + (tx.signatures.len() * size_of::<Signature>())
            + 1
            + (tx.account_keys.len() * size_of::<Pubkey>())
            + size_of::<Hash>()
            + size_of::<u64>()
            + 1
            + (tx.program_ids.len() * size_of::<Pubkey>())
            + 1
            + expected_instruction_size;
        assert_eq!(expected_transaction_size, 221);

        assert_eq!(
            serialized_size(&tx).unwrap() as usize,
            expected_transaction_size,
            "unexpected serialized transaction size"
        );
    }

    /// Detect binary changes in the serialized transaction data, which could have a downstream
    /// affect on SDKs and DApps
    #[test]
    fn test_sdk_serialize() {
        assert_eq!(
            serialize(&create_sample_transaction()).unwrap(),
            vec![
                1, 107, 231, 179, 42, 11, 220, 153, 173, 229, 29, 51, 218, 98, 26, 46, 164, 248,
                228, 118, 244, 191, 192, 198, 228, 190, 119, 21, 52, 66, 25, 124, 247, 192, 73, 48,
                231, 2, 70, 34, 82, 133, 137, 148, 66, 73, 231, 72, 195, 100, 133, 214, 2, 168,
                108, 252, 200, 83, 99, 105, 51, 216, 145, 30, 14, 2, 36, 100, 158, 252, 33, 161,
                97, 185, 62, 89, 99, 195, 250, 249, 187, 189, 171, 118, 241, 90, 248, 14, 68, 219,
                231, 62, 157, 5, 142, 27, 210, 117, 1, 1, 1, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9,
                9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 99, 0, 0, 0, 0, 0, 0,
                0, 1, 2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7,
                6, 5, 4, 2, 2, 2, 1, 0, 2, 0, 1, 3, 1, 2, 3
            ]
        );
    }

    #[test]
    #[should_panic]
    fn test_transaction_missing_key() {
        let keypair = Keypair::new();
        Transaction::new_unsigned_instructions(vec![]).sign(&[&keypair], Hash::default());
    }

    #[test]
    #[should_panic]
    fn test_transaction_missing_keypair() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let id0 = keypair0.pubkey();
        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]);
        Transaction::new_unsigned_instructions(vec![ix])
            .sign(&Vec::<&Keypair>::new(), Hash::default());
    }

    #[test]
    #[should_panic]
    fn test_transaction_wrong_key() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let wrong_id = Pubkey::default();
        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(wrong_id, true)]);
        Transaction::new_unsigned_instructions(vec![ix]).sign(&[&keypair0], Hash::default());
    }

    #[test]
    fn test_transaction_correct_key() {
        let program_id = Pubkey::default();
        let keypair0 = Keypair::new();
        let id0 = keypair0.pubkey();
        let ix = Instruction::new(program_id, &0, vec![AccountMeta::new(id0, true)]);
        let mut tx = Transaction::new_unsigned_instructions(vec![ix]);
        tx.sign(&[&keypair0], Hash::default());
        assert_eq!(tx.instructions[0], CompiledInstruction::new(0, &0, vec![0]));
    }
}
