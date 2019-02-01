//! The `transaction` module provides functionality for creating log transactions.

use crate::hash::{Hash, Hasher};
use crate::packet::PACKET_DATA_SIZE;
use crate::pubkey::Pubkey;
use crate::shortvec::{
    deserialize_vec_bytes, deserialize_vec_with, encode_len, serialize_vec_bytes,
    serialize_vec_with,
};
use crate::signature::{Keypair, KeypairUtil, Signature};
use bincode::{serialize, Error};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;
use std::io::{Cursor, Read, Write};
use std::mem::size_of;

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

    pub fn serialize_with(
        mut writer: &mut Cursor<&mut [u8]>,
        ix: &Instruction,
    ) -> Result<(), Error> {
        writer.write_all(&[ix.program_ids_index])?;
        serialize_vec_bytes(&mut writer, &ix.accounts[..])?;
        serialize_vec_bytes(&mut writer, &ix.userdata[..])?;
        Ok(())
    }

    pub fn deserialize_from(mut reader: &mut Cursor<&[u8]>) -> Result<Self, Error> {
        let mut buf = [0];
        reader.read_exact(&mut buf)?;
        let program_ids_index = buf[0];
        let accounts = deserialize_vec_bytes(&mut reader)?;
        let userdata = deserialize_vec_bytes(&mut reader)?;
        Ok(Instruction {
            program_ids_index,
            accounts,
            userdata,
        })
    }

    pub fn serialized_size(&self) -> Result<u64, Error> {
        let mut buf = [0; size_of::<u64>() + 1];
        let mut wr = Cursor::new(&mut buf[..]);
        let mut size = size_of::<u8>();

        let len = self.accounts.len();
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<u8>());

        let len = self.userdata.len();
        wr.set_position(0);
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<u8>());

        Ok(size as u64)
    }
}

/// An atomic transaction
#[derive(Debug, PartialEq, Eq, Clone)]
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
    pub fn new<S: Serialize, T: KeypairUtil>(
        from_keypair: &T,
        transaction_keys: &[Pubkey],
        program_id: Pubkey,
        userdata: &S,
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
        from_pubkey: &Pubkey,
        transaction_keys: &[Pubkey],
        program_id: Pubkey,
        userdata: &T,
        last_id: Hash,
        fee: u64,
    ) -> Self {
        let program_ids = vec![program_id];
        let accounts = (0..=transaction_keys.len() as u8).collect();
        let instructions = vec![Instruction::new(0, userdata, accounts)];
        let mut keys = vec![*from_pubkey];
        keys.extend_from_slice(transaction_keys);
        Self::new_with_instructions::<Keypair>(
            &[],
            &keys[..],
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
    pub fn new_with_instructions<T: KeypairUtil>(
        from_keypairs: &[&T],
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
    pub fn message(&self) -> Vec<u8> {
        let mut buf = vec![0u8; PACKET_DATA_SIZE];
        let mut wr = Cursor::new(&mut buf[..]);
        serialize_vec_with(&mut wr, &self.account_keys, Transaction::serialize_pubkey)
            .expect("serialize account_keys");
        wr.write_all(self.last_id.as_ref())
            .expect("serialize last_id");
        wr.write_u64::<LittleEndian>(self.fee)
            .expect("serialize fee");
        serialize_vec_with(&mut wr, &self.program_ids, Transaction::serialize_pubkey)
            .expect("serialize program_ids");
        serialize_vec_with(&mut wr, &self.instructions, Instruction::serialize_with)
            .expect("serialize instructions");
        let len = wr.position() as usize;
        wr.into_inner()[..len].to_vec()
    }

    /// Sign this transaction.
    pub fn sign<T: KeypairUtil>(&mut self, keypairs: &[&T], last_id: Hash) {
        self.last_id = last_id;
        let message = self.message();
        self.signatures = keypairs
            .iter()
            .map(|keypair| keypair.sign_message(&message))
            .collect();
    }

    /// Verify only the transaction signature.
    pub fn verify_signature(&self) -> bool {
        self.signatures
            .iter()
            .all(|s| s.verify(&self.from().as_ref(), &self.message()))
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

    pub fn serialized_size(&self) -> Result<u64, Error> {
        let mut buf = [0u8; size_of::<u64>() + 1];
        let mut wr = Cursor::new(&mut buf[..]);
        let mut size = size_of::<u64>();

        let len = self.signatures.len();
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<Signature>());

        let len = self.account_keys.len();
        wr.set_position(0);
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<Pubkey>());

        size += size_of::<Hash>();

        size += size_of::<u64>();

        let len = self.program_ids.len();
        wr.set_position(0);
        encode_len(&mut wr, len)?;
        size += wr.position() as usize + (len * size_of::<Pubkey>());

        let len = self.instructions.len();
        wr.set_position(0);
        encode_len(&mut wr, len)?;
        size += wr.position() as usize;
        let inst_size: u64 = self
            .instructions
            .iter()
            .map(|ix| ix.serialized_size().unwrap())
            .sum();
        Ok(size as u64 + inst_size)
    }

    fn serialize_signature(writer: &mut Cursor<&mut [u8]>, sig: &Signature) -> Result<(), Error> {
        writer.write_all(sig.as_ref())?;
        Ok(())
    }

    fn serialize_pubkey(writer: &mut Cursor<&mut [u8]>, key: &Pubkey) -> Result<(), Error> {
        writer.write_all(key.as_ref())?;
        Ok(())
    }

    fn deserialize_signature(reader: &mut Cursor<&[u8]>) -> Result<Signature, Error> {
        let mut buf = [0; size_of::<Signature>()];
        reader.read_exact(&mut buf)?;
        Ok(Signature::new(&buf))
    }

    fn deserialize_pubkey(reader: &mut Cursor<&[u8]>) -> Result<Pubkey, Error> {
        let mut buf = [0; size_of::<Pubkey>()];
        reader.read_exact(&mut buf)?;
        Ok(Pubkey::new(&buf))
    }
}

impl Serialize for Transaction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::Error;
        let mut buf = vec![0u8; self.serialized_size().unwrap() as usize];
        let mut wr = Cursor::new(&mut buf[..]);
        serialize_vec_with(&mut wr, &self.signatures, Transaction::serialize_signature)
            .map_err(Error::custom)?;
        serialize_vec_with(&mut wr, &self.account_keys, Transaction::serialize_pubkey)
            .map_err(Error::custom)?;
        wr.write_all(self.last_id.as_ref()).map_err(Error::custom)?;
        wr.write_u64::<LittleEndian>(self.fee)
            .map_err(Error::custom)?;
        serialize_vec_with(&mut wr, &self.program_ids, Transaction::serialize_pubkey)
            .map_err(Error::custom)?;
        serialize_vec_with(&mut wr, &self.instructions, Instruction::serialize_with)
            .map_err(Error::custom)?;
        let size = wr.position() as usize;
        serializer.serialize_bytes(&wr.into_inner()[..size])
    }
}

struct TransactionVisitor;
impl<'a> serde::de::Visitor<'a> for TransactionVisitor {
    type Value = Transaction;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("Expecting Instruction")
    }
    fn visit_bytes<E>(self, data: &[u8]) -> Result<Transaction, E>
    where
        E: serde::de::Error,
    {
        use serde::de::Error;
        let mut rd = Cursor::new(&data[..]);
        let signatures: Vec<Signature> =
            deserialize_vec_with(&mut rd, Transaction::deserialize_signature)
                .map_err(Error::custom)?;
        let account_keys: Vec<Pubkey> =
            deserialize_vec_with(&mut rd, Transaction::deserialize_pubkey)
                .map_err(Error::custom)?;
        let mut buf = [0; size_of::<Hash>()];
        rd.read_exact(&mut buf).map_err(Error::custom)?;
        let last_id: Hash = Hash::new(&buf);
        let fee = rd.read_u64::<LittleEndian>().map_err(Error::custom)?;
        let program_ids: Vec<Pubkey> =
            deserialize_vec_with(&mut rd, Transaction::deserialize_pubkey)
                .map_err(Error::custom)?;
        let instructions: Vec<Instruction> =
            deserialize_vec_with(&mut rd, Instruction::deserialize_from).map_err(Error::custom)?;
        Ok(Transaction {
            signatures,
            account_keys,
            last_id,
            fee,
            program_ids,
            instructions,
        })
    }
}

impl<'de> Deserialize<'de> for Transaction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: ::serde::Deserializer<'de>,
    {
        deserializer.deserialize_bytes(TransactionVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::deserialize;

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

    #[test]
    fn test_transaction_serialize() {
        let keypair = Keypair::new();
        let program_id = Pubkey::new(&[4; 32]);
        let to = Pubkey::new(&[5; 32]);
        let tx = Transaction::new(
            &keypair,
            &[keypair.pubkey(), to],
            program_id,
            &(1u8, 2u8, 3u8),
            Hash::default(),
            99,
        );

        let ser = serialize(&tx).unwrap();
        let deser = deserialize(&ser).unwrap();
        assert_eq!(tx, deser);
    }

    #[test]
    fn test_transaction_serialized_size() {
        let keypair = Keypair::new();
        let program_id = Pubkey::new(&[4; 32]);
        let to = Pubkey::new(&[5; 32]);
        let tx = Transaction::new(
            &keypair,
            &[keypair.pubkey(), to],
            program_id,
            &(1u8, 2u8, 3u8),
            Hash::default(),
            99,
        );
        let req_size = size_of::<u64>()
            + 1
            + (tx.signatures.len() * size_of::<Signature>())
            + 1
            + (tx.account_keys.len() * size_of::<Pubkey>())
            + size_of::<Hash>()
            + size_of::<u64>()
            + 1
            + (tx.program_ids.len() * size_of::<Pubkey>())
            + 1
            + tx.instructions[0].serialized_size().unwrap() as usize;
        let size = tx.serialized_size().unwrap() as usize;
        assert_eq!(req_size, size);
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
                245, 0, 0, 0, 0, 0, 0, 0, 1, 151, 224, 239, 74, 248, 111, 129, 62, 193, 150, 178,
                53, 242, 136, 228, 153, 16, 245, 127, 217, 6, 122, 114, 165, 224, 243, 191, 164,
                197, 107, 71, 41, 57, 132, 240, 19, 166, 239, 109, 168, 225, 215, 1, 59, 120, 57,
                141, 103, 243, 182, 221, 176, 161, 153, 217, 129, 87, 178, 228, 151, 57, 163, 75,
                13, 3, 36, 100, 158, 252, 33, 161, 97, 185, 62, 89, 99, 195, 250, 249, 187, 189,
                171, 118, 241, 90, 248, 14, 68, 219, 231, 62, 157, 5, 142, 27, 210, 117, 36, 100,
                158, 252, 33, 161, 97, 185, 62, 89, 99, 195, 250, 249, 187, 189, 171, 118, 241, 90,
                248, 14, 68, 219, 231, 62, 157, 5, 142, 27, 210, 117, 1, 1, 1, 4, 5, 6, 7, 8, 9, 9,
                9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 1, 1, 1, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 99,
                0, 0, 0, 0, 0, 0, 0, 1, 2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
                1, 1, 1, 9, 8, 7, 6, 5, 4, 2, 2, 2, 1, 0, 3, 0, 1, 2, 3, 1, 2, 3
            ],
        );
    }
}
