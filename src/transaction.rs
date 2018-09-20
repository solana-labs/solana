//! The `transaction` module provides functionality for creating log transactions.

use bincode::{deserialize, serialize};
use budget::{Budget, Condition};
use budget_contract::BudgetContract;
use chrono::prelude::*;
use hash::Hash;
use instruction::{Contract, Instruction, Vote};
use payment_plan::{Payment, PaymentPlan};
use signature::{Keypair, KeypairUtil, Pubkey, Signature};
use std::mem::size_of;
use system_contract::SystemContract;

pub const SIGNED_DATA_OFFSET: usize = size_of::<Signature>();
pub const SIG_OFFSET: usize = 0;
pub const PUB_KEY_OFFSET: usize = size_of::<Signature>() + size_of::<u64>();

/// An instruction signed by a client with `Pubkey`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction {
    /// A digital signature of `keys`, `contract_id`, `last_id`, `fee` and `userdata`, signed by `Pubkey`.
    pub signature: Signature,

    /// The `Pubkeys` that are executing this transaction userdata.  The meaning of each key is
    /// contract-specific.
    /// * keys[0] - Typically this is the `caller` public key.  `signature` is verified with keys[0].
    /// In the future which key pays the fee and which keys have signatures would be configurable.
    /// * keys[1] - Typically this is the contract context or the recipient of the tokens
    pub keys: Vec<Pubkey>,

    /// the contract id to execute
    pub contract_id: Pubkey,

    /// The ID of a recent ledger entry.
    pub last_id: Hash,

    /// The number of tokens paid for processing and storage of this transaction.
    pub fee: i64,

    /// Userdata to be stored in the account
    pub userdata: Vec<u8>,
}

impl Transaction {
    /// Create a signed transaction from the given `Instruction`.
    /// * `from_keypair` - The key used to sign the transaction.  This key is stored as keys[0]
    /// * `transaction_keys` - The keys for the transaction.  These are the contract state
    ///    instances or token recipient keys.
    /// * `userdata` - The input data that the contract will execute with
    /// * `last_id` - The PoH hash.
    /// * `fee` - The transaction fee.
    fn new_with_userdata(
        from_keypair: &Keypair,
        transaction_keys: &[Pubkey],
        contract_id: Pubkey,
        userdata: Vec<u8>,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let from = from_keypair.pubkey();
        let mut keys = vec![from];
        keys.extend_from_slice(transaction_keys);
        let mut tx = Transaction {
            signature: Signature::default(),
            keys,
            contract_id,
            last_id,
            fee,
            userdata,
        };
        tx.sign(from_keypair);
        tx
    }
    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn budget_new_taxed(
        from_keypair: &Keypair,
        contract: Pubkey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
    ) -> Self {
        let payment = Payment {
            tokens: tokens - fee,
            to: contract,
        };
        let budget = Budget::Pay(payment);
        let instruction = Instruction::NewContract(Contract { budget, tokens });
        let userdata = serialize(&instruction).unwrap();
        Self::new_with_userdata(
            from_keypair,
            &[contract],
            BudgetContract::id(),
            userdata,
            last_id,
            fee,
        )
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn budget_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
        Self::budget_new_taxed(from_keypair, to, tokens, 0, last_id)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn budget_new_timestamp(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        dt: DateTime<Utc>,
        last_id: Hash,
    ) -> Self {
        let instruction = Instruction::ApplyTimestamp(dt);
        let userdata = serialize(&instruction).unwrap();
        Self::new_with_userdata(
            from_keypair,
            &[contract, to],
            BudgetContract::id(),
            userdata,
            last_id,
            0,
        )
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn budget_new_signature(
        from_keypair: &Keypair,
        contract: Pubkey,
        to: Pubkey,
        last_id: Hash,
    ) -> Self {
        let instruction = Instruction::ApplySignature;
        let userdata = serialize(&instruction).unwrap();
        Self::new_with_userdata(
            from_keypair,
            &[contract, to],
            BudgetContract::id(),
            userdata,
            last_id,
            0,
        )
    }

    pub fn budget_new_vote(from_keypair: &Keypair, vote: Vote, last_id: Hash, fee: i64) -> Self {
        let instruction = Instruction::NewVote(vote);
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new_with_userdata(
            from_keypair,
            &[],
            BudgetContract::id(),
            userdata,
            last_id,
            fee,
        )
    }

    /// Create and sign a postdated Transaction. Used for unit-testing.
    pub fn budget_new_on_date(
        from_keypair: &Keypair,
        to: Pubkey,
        contract: Pubkey,
        dt: DateTime<Utc>,
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let from = from_keypair.pubkey();
        let budget = Budget::Or(
            (Condition::Timestamp(dt, from), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        );
        let instruction = Instruction::NewContract(Contract { budget, tokens });
        let userdata = serialize(&instruction).expect("serialize instruction");
        Self::new_with_userdata(
            from_keypair,
            &[contract],
            BudgetContract::id(),
            userdata,
            last_id,
            0,
        )
    }
    /// Create and sign new SystemContract::CreateAccount transaction
    pub fn system_create(
        from_keypair: &Keypair,
        to: Pubkey,
        last_id: Hash,
        tokens: i64,
        space: u64,
        contract_id: Pubkey,
        fee: i64,
    ) -> Self {
        let create = SystemContract::CreateAccount {
            tokens, //TODO, the tokens to allocate might need to be higher then 0 in the future
            space,
            contract_id,
        };
        Transaction::new_with_userdata(
            from_keypair,
            &[to],
            SystemContract::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemContract::CreateAccount transaction
    pub fn system_assign(
        from_keypair: &Keypair,
        last_id: Hash,
        contract_id: Pubkey,
        fee: i64,
    ) -> Self {
        let create = SystemContract::Assign { contract_id };
        Transaction::new_with_userdata(
            from_keypair,
            &[],
            SystemContract::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemContract::CreateAccount transaction with some defaults
    pub fn system_new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
        Transaction::system_create(from_keypair, to, last_id, tokens, 0, Pubkey::default(), 0)
    }
    /// Create and sign new SystemContract::Move transaction
    pub fn system_move(
        from_keypair: &Keypair,
        to: Pubkey,
        tokens: i64,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let create = SystemContract::Move { tokens };
        Transaction::new_with_userdata(
            from_keypair,
            &[to],
            SystemContract::id(),
            serialize(&create).unwrap(),
            last_id,
            fee,
        )
    }
    /// Create and sign new SystemContract::Move transaction
    pub fn new(from_keypair: &Keypair, to: Pubkey, tokens: i64, last_id: Hash) -> Self {
        Transaction::system_move(from_keypair, to, tokens, last_id, 0)
    }
    /// Get the transaction data to sign.
    fn get_sign_data(&self) -> Vec<u8> {
        let mut data = serialize(&(&self.keys)).expect("serialize keys");

        let contract_id = serialize(&(&self.contract_id)).expect("serialize contract_id");
        data.extend_from_slice(&contract_id);

        let last_id_data = serialize(&(&self.last_id)).expect("serialize last_id");
        data.extend_from_slice(&last_id_data);

        let fee_data = serialize(&(&self.fee)).expect("serialize last_id");
        data.extend_from_slice(&fee_data);

        let userdata = serialize(&(&self.userdata)).expect("serialize userdata");
        data.extend_from_slice(&userdata);
        data
    }

    /// Sign this transaction.
    pub fn sign(&mut self, keypair: &Keypair) {
        let sign_data = self.get_sign_data();
        self.signature = Signature::new(keypair.sign(&sign_data).as_ref());
    }

    /// Verify only the transaction signature.
    pub fn verify_signature(&self) -> bool {
        warn!("transaction signature verification called");
        self.signature
            .verify(&self.from().as_ref(), &self.get_sign_data())
    }

    pub fn vote(&self) -> Option<(Pubkey, Vote, Hash)> {
        if let Some(Instruction::NewVote(vote)) = self.instruction() {
            Some((*self.from(), vote, self.last_id))
        } else {
            None
        }
    }
    pub fn from(&self) -> &Pubkey {
        &self.keys[0]
    }
    pub fn instruction(&self) -> Option<Instruction> {
        deserialize(&self.userdata).ok()
    }
    /// Verify only the payment plan.
    pub fn verify_plan(&self) -> bool {
        if let Some(Instruction::NewContract(contract)) = self.instruction() {
            self.fee >= 0
                && self.fee <= contract.tokens
                && contract.budget.verify(contract.tokens - self.fee)
        } else {
            true
        }
    }
}

pub fn test_tx() -> Transaction {
    let keypair1 = Keypair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    Transaction::system_new(&keypair1, pubkey1, 42, zero)
}

#[cfg(test)]
pub fn memfind<A: Eq>(a: &[A], b: &[A]) -> Option<usize> {
    assert!(a.len() >= b.len());
    let end = a.len() - b.len() + 1;
    for i in 0..end {
        if a[i..i + b.len()] == b[..] {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use bincode::{deserialize, serialize};
    use packet::PACKET_DATA_SIZE;
    use signature::GenKeys;

    #[test]
    fn test_claim() {
        let keypair = Keypair::new();
        let zero = Hash::default();
        let tx0 = Transaction::budget_new(&keypair, keypair.pubkey(), 42, zero);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let tx0 = Transaction::budget_new(&keypair0, pubkey1, 42, zero);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer_with_fee() {
        let zero = Hash::default();
        let keypair0 = Keypair::new();
        let pubkey1 = Keypair::new().pubkey();
        assert!(Transaction::budget_new_taxed(&keypair0, pubkey1, 1, 1, zero).verify_plan());
        assert!(!Transaction::budget_new_taxed(&keypair0, pubkey1, 1, 2, zero).verify_plan());
        assert!(!Transaction::budget_new_taxed(&keypair0, pubkey1, 1, -1, zero).verify_plan());
    }

    #[test]
    fn test_serialize_claim() {
        let budget = Budget::Pay(Payment {
            tokens: 0,
            to: Default::default(),
        });
        let instruction = Instruction::NewContract(Contract { budget, tokens: 0 });
        let userdata = serialize(&instruction).unwrap();
        let claim0 = Transaction {
            keys: vec![],
            last_id: Default::default(),
            signature: Default::default(),
            contract_id: Default::default(),
            fee: 0,
            userdata,
        };
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }

    #[test]
    fn test_token_attack() {
        let zero = Hash::default();
        let keypair = Keypair::new();
        let pubkey = keypair.pubkey();
        let mut tx = Transaction::budget_new(&keypair, pubkey, 42, zero);
        let mut instruction = tx.instruction().unwrap();
        if let Instruction::NewContract(ref mut contract) = instruction {
            contract.tokens = 1_000_000; // <-- attack, part 1!
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.tokens = contract.tokens; // <-- attack, part 2!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(tx.verify_plan());
        assert!(!tx.verify_signature());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let thief_keypair = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let mut tx = Transaction::budget_new(&keypair0, pubkey1, 42, zero);
        let mut instruction = tx.instruction();
        if let Some(Instruction::NewContract(ref mut contract)) = instruction {
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(tx.verify_plan());
        assert!(!tx.verify_signature());
    }
    #[test]
    fn test_layout() {
        let tx = test_tx();
        let sign_data = tx.get_sign_data();
        let tx_bytes = serialize(&tx).unwrap();
        assert_eq!(memfind(&tx_bytes, &sign_data), Some(SIGNED_DATA_OFFSET));
        assert_eq!(memfind(&tx_bytes, &tx.signature.as_ref()), Some(SIG_OFFSET));
        assert_eq!(
            memfind(&tx_bytes, &tx.from().as_ref()),
            Some(PUB_KEY_OFFSET)
        );
        assert!(tx.verify_signature());
    }
    #[test]
    fn test_userdata_layout() {
        let mut tx0 = test_tx();
        tx0.userdata = vec![1, 2, 3];
        let sign_data0a = tx0.get_sign_data();
        let tx_bytes = serialize(&tx0).unwrap();
        assert!(tx_bytes.len() < PACKET_DATA_SIZE);
        assert_eq!(memfind(&tx_bytes, &sign_data0a), Some(SIGNED_DATA_OFFSET));
        assert_eq!(
            memfind(&tx_bytes, &tx0.signature.as_ref()),
            Some(SIG_OFFSET)
        );
        assert_eq!(
            memfind(&tx_bytes, &tx0.from().as_ref()),
            Some(PUB_KEY_OFFSET)
        );
        let tx1 = deserialize(&tx_bytes).unwrap();
        assert_eq!(tx0, tx1);
        assert_eq!(tx1.userdata, vec![1, 2, 3]);

        tx0.userdata = vec![1, 2, 4];
        let sign_data0b = tx0.get_sign_data();
        assert_ne!(sign_data0a, sign_data0b);
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = Keypair::new();
        let keypair1 = Keypair::new();
        let zero = Hash::default();
        let mut tx = Transaction::budget_new(&keypair0, keypair1.pubkey(), 1, zero);
        let mut instruction = tx.instruction().unwrap();
        if let Instruction::NewContract(ref mut contract) = instruction {
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.tokens = 2; // <-- attack!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(!tx.verify_plan());

        // Also, ensure all branchs of the plan spend all tokens
        let mut instruction = tx.instruction().unwrap();
        if let Instruction::NewContract(ref mut contract) = instruction {
            if let Budget::Pay(ref mut payment) = contract.budget {
                payment.tokens = 0; // <-- whoops!
            }
        }
        tx.userdata = serialize(&instruction).unwrap();
        assert!(!tx.verify_plan());
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

        let contract = Pubkey::new(&[
            2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4,
            2, 2, 2,
        ]);

        let tx = Transaction::new_with_userdata(
            keypair,
            &[keypair.pubkey(), to],
            contract,
            vec![1, 2, 3],
            Hash::default(),
            99,
        );
        assert_eq!(
            serialize(&tx).unwrap(),
            vec![
                88, 1, 212, 176, 31, 197, 35, 156, 135, 24, 30, 57, 204, 253, 224, 28, 89, 189, 53,
                64, 27, 148, 42, 199, 43, 236, 85, 182, 150, 64, 96, 53, 255, 235, 90, 197, 228, 6,
                105, 22, 140, 209, 206, 221, 85, 117, 125, 126, 11, 1, 176, 130, 57, 236, 7, 155,
                127, 58, 130, 92, 230, 219, 254, 0, 3, 0, 0, 0, 0, 0, 0, 0, 32, 253, 186, 201, 177,
                11, 117, 135, 187, 167, 181, 188, 22, 59, 206, 105, 231, 150, 215, 30, 78, 212, 76,
                16, 252, 180, 72, 134, 137, 247, 161, 68, 32, 253, 186, 201, 177, 11, 117, 135,
                187, 167, 181, 188, 22, 59, 206, 105, 231, 150, 215, 30, 78, 212, 76, 16, 252, 180,
                72, 134, 137, 247, 161, 68, 1, 1, 1, 4, 5, 6, 7, 8, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
                9, 9, 9, 9, 9, 9, 8, 7, 6, 5, 4, 1, 1, 1, 2, 2, 2, 4, 5, 6, 7, 8, 9, 1, 1, 1, 1, 1,
                1, 1, 1, 1, 1, 1, 1, 1, 1, 9, 8, 7, 6, 5, 4, 2, 2, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 99, 0, 0, 0, 0,
                0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3
            ],
        );
    }
}
