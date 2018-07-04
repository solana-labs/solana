//! The `transaction` module provides functionality for creating log transactions.

use bank::BANK_PROCESS_TRANSACTION_METHOD;
use bincode::{deserialize, serialize};
use budget::{Budget, Condition};
use chrono::prelude::*;
use hash::Hash;
use itertools::Itertools;
use page_table::{self, Call};
use payment_plan::{Payment, PaymentPlan, Witness};
use signature::{KeyPair, KeyPairUtil, PublicKey, Signature};

pub const SIGNED_DATA_OFFSET: usize = 80;
pub const SIG_OFFSET: usize = 16;
pub const PUB_KEY_OFFSET: usize = 96;

/// The type of payment plan. Each item must implement the PaymentPlan trait.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Plan {
    /// The builtin contract language Budget.
    Budget(Budget),
}

// A proxy for the underlying DSL.
impl PaymentPlan for Plan {
    fn final_payment(&self) -> Option<Payment> {
        match self {
            Plan::Budget(budget) => budget.final_payment(),
        }
    }

    fn verify(&self, spendable_tokens: i64) -> bool {
        match self {
            Plan::Budget(budget) => budget.verify(spendable_tokens),
        }
    }

    fn apply_witness(&mut self, witness: &Witness, from: &PublicKey) {
        match self {
            Plan::Budget(budget) => budget.apply_witness(witness, from),
        }
    }
}

/// A smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Contract {
    /// The number of tokens allocated to the `Plan` and any transaction fees.
    pub tokens: i64,
    pub plan: Plan,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Vote {
    /// We send some gossip specific membership information through the vote to shortcut
    /// liveness voting
    /// The version of the CRDT struct that the last_id of this network voted with
    pub version: u64,
    /// The version of the CRDT struct that has the same network configuration as this one
    pub contact_info_version: u64,
    // TODO: add signature of the state here as well
}

/// An instruction to progress the smart contract.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Instruction {
    /// Declare and instanstansiate `Contract`.
    NewContract(Contract),

    /// Tell a payment plan acknowledge the given `DateTime` has past.
    ApplyTimestamp(DateTime<Utc>),

    /// Tell the payment plan that the `NewContract` with `Signature` has been
    /// signed by the containing transaction's `PublicKey`.
    ApplySignature(Signature),

    /// Vote for a PoH that is equal to the lastid of this transaction
    NewVote(Vote),
}

/// An instruction signed by a client with `PublicKey`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Transaction {
    pub call: Call,
}
pub enum TransactionKeys<'a> {
    KeyPair(&'a KeyPair),
    PubKey(&'a PublicKey),
}
impl Transaction {
    /// Compile and sign transaction into a Call
    fn new_from_instruction(
        keypairs: &[TransactionKeys],
        instruction: Instruction,
        last_id: Hash,
        fee: i64,
        version: u64,
    ) -> Self {
        assert_eq!(
            keypairs.is_empty(),
            false,
            "expect at least 1 transaction key"
        );
        let pubkeys: Vec<PublicKey> = keypairs
            .into_iter()
            .map(|tk| match tk {
                TransactionKeys::KeyPair(keypair) => keypair.pubkey(),
                TransactionKeys::PubKey(pubkey) => **pubkey,
            })
            .collect();
        let user_data = serialize(&instruction).expect("serialize instruction");
        let required_proofs: Vec<u8> = keypairs
            .iter()
            .enumerate()
            .filter_map(|(i, tk)| match tk {
                TransactionKeys::KeyPair(_) => Some(i as u8),
                _ => None,
            })
            .collect();
        let mut call = Call::new(
            pubkeys,
            required_proofs,
            0, //TODO(anatoly): PoH count
            last_id,
            page_table::DEFAULT_CONTRACT,
            version,
            fee,
            BANK_PROCESS_TRANSACTION_METHOD,
            user_data,
        );
        keypairs.iter().foreach(|tk| match tk {
            TransactionKeys::KeyPair(keypair) => call.append_signature(&keypair),
            _ => (),
        });
        Transaction { call }
    }
    pub fn new_noplan(
        from_keypair: &KeyPair,
        to: PublicKey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
        version: u64,
    ) -> Self {
        let mut call = Call::new_tx(from_keypair.pubkey(), 0, last_id, tokens, fee, version, to);
        call.append_signature(from_keypair);
        Transaction { call }
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new_taxed(
        from_keypair: &KeyPair,
        to: PublicKey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
        version: u64,
    ) -> Self {
        let payment = Payment {
            tokens: tokens - fee,
            to,
        };
        let budget = Budget::Pay(payment);
        let plan = Plan::Budget(budget);
        let instruction = Instruction::NewContract(Contract { plan, tokens });
        let keys = [
            TransactionKeys::KeyPair(from_keypair.clone()),
            TransactionKeys::PubKey(&to),
        ];
        Self::new_from_instruction(&keys, instruction, last_id, fee, version)
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new(
        from_keypair: &KeyPair,
        to: PublicKey,
        tokens: i64,
        last_id: Hash,
        version: u64,
    ) -> Self {
        Self::new_taxed(from_keypair, to, tokens, 0, last_id, version)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn new_timestamp(
        from_keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        last_id: Hash,
        version: u64,
    ) -> Self {
        let instruction = Instruction::ApplyTimestamp(dt);
        let keys = [
            TransactionKeys::KeyPair(from_keypair),
            TransactionKeys::PubKey(&to),
        ];
        Self::new_from_instruction(&keys, instruction, last_id, 0, version)
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn new_signature(
        from_keypair: &KeyPair,
        to: PublicKey,
        tx_sig: Signature,
        last_id: Hash,
        version: u64,
    ) -> Self {
        let instruction = Instruction::ApplySignature(tx_sig);
        let keys = [
            TransactionKeys::KeyPair(from_keypair),
            TransactionKeys::PubKey(&to),
        ];
        Self::new_from_instruction(&keys, instruction, last_id, 0, version)
    }

    pub fn new_vote(
        from_keypair: &KeyPair,
        vote: Vote,
        last_id: Hash,
        fee: i64,
        version: u64,
    ) -> Self {
        let keys = [TransactionKeys::KeyPair(from_keypair)];
        Transaction::new_from_instruction(&keys, Instruction::NewVote(vote), last_id, fee, version)
    }

    /// Create and sign a postdated Transaction. Used for unit-testing.
    pub fn new_on_date(
        from_keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        tokens: i64,
        last_id: Hash,
        version: u64,
    ) -> Self {
        let from = from_keypair.pubkey();
        let budget = Budget::Or(
            (Condition::Timestamp(dt, from), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        );
        let plan = Plan::Budget(budget);
        let instruction = Instruction::NewContract(Contract { plan, tokens });
        let keys = [
            TransactionKeys::KeyPair(from_keypair),
            TransactionKeys::PubKey(&to),
        ];
        Self::new_from_instruction(&keys, instruction, last_id, 0, version)
    }

    pub fn sig(&self) -> &Signature {
        &self.call.proofs[0]
    }
    pub fn sigs(&self) -> &[Signature] {
        &self.call.proofs
    }
    pub fn from(&self) -> &PublicKey {
        &self.call.data.keys[0]
    }
    pub fn last_id(&self) -> &Hash {
        &self.call.data.last_hash
    }
    pub fn fee(&self) -> i64 {
        self.call.data.fee
    }
    pub fn instruction(&self) -> Instruction {
        deserialize(&self.call.data.user_data).unwrap()
    }
    /// Sign this transaction.
    pub fn sign(&mut self, keypair: &KeyPair) {
        self.call.append_signature(keypair);
    }
    /// Sign this transaction.
    pub fn get_sign_data(&self) -> Vec<u8> {
        self.call.get_sign_data()
    }

    /// Verify only the transaction signature.
    pub fn verify_sig(&self) -> bool {
        warn!("transaction signature verification called");
        self.call.verify_sig()
    }

    /// Verify only the payment plan.
    pub fn verify_plan(&self) -> bool {
        let instruction = self.instruction();
        if let Instruction::NewContract(contract) = instruction {
            self.fee() >= 0
                && self.call.data.fee <= contract.tokens
                && contract.plan.verify(contract.tokens - self.fee())
        } else {
            true
        }
    }
}

pub fn test_tx() -> Transaction {
    let keypair1 = KeyPair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    Transaction::new(&keypair1, pubkey1, 42, zero, 0)
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

    #[test]
    fn test_claim() {
        let keypair = KeyPair::new();
        let zero = Hash::default();
        let tx0 = Transaction::new(&keypair, keypair.pubkey(), 42, zero, 0);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer() {
        let zero = Hash::default();
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
        let tx0 = Transaction::new(&keypair0, pubkey1, 42, zero, 0);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer_with_fee() {
        let zero = Hash::default();
        let keypair0 = KeyPair::new();
        let pubkey1 = KeyPair::new().pubkey();
        assert!(Transaction::new_taxed(&keypair0, pubkey1, 1, 1, zero, 0).verify_plan());
        assert!(!Transaction::new_taxed(&keypair0, pubkey1, 1, 2, zero, 0).verify_plan());
        assert!(!Transaction::new_taxed(&keypair0, pubkey1, 1, -1, zero, 0).verify_plan());
    }

    #[test]
    fn test_serialize_claim() {
        let keypair = KeyPair::new();
        let to = Default::default();
        let budget = Budget::Pay(Payment { tokens: 0, to: to });
        let plan = Plan::Budget(budget);
        let instruction = Instruction::NewContract(Contract { plan, tokens: 0 });
        let keys = [
            TransactionKeys::KeyPair(&keypair),
            TransactionKeys::PubKey(&to),
        ];
        let claim0 =
            Transaction::new_from_instruction(&keys, instruction, Default::default(), 0, 0);
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }

    #[test]
    fn test_size() {
        let keypair = KeyPair::new();
        let to = Default::default();
        let claim0 = Transaction::new_noplan(&keypair, to, 0, 0, Default::default(), 0);
        let buf = serialize(&claim0).unwrap();
        assert_eq!(buf.len(), 290);
    }

    #[test]
    fn test_token_attack() {
        let zero = Hash::default();
        let keypair = KeyPair::new();
        let pubkey = keypair.pubkey();
        let mut tx = Transaction::new(&keypair, pubkey, 42, zero, 0);
        let mut instruction = tx.instruction();
        if let Instruction::NewContract(contract) = &mut instruction {
            contract.tokens = 1_000_000; // <-- attack, part 1!
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.tokens = contract.tokens; // <-- attack, part 2!
            }
        }
        tx.call.data.user_data = serialize(&instruction).expect("serialize instruction");
        assert!(tx.verify_plan());
        assert!(!tx.verify_sig());
    }

    #[test]
    fn test_hijack_attack() {
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let thief_keypair = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        let mut tx = Transaction::new(&keypair0, pubkey1, 42, zero, 0);
        let mut instruction = tx.instruction();
        if let Instruction::NewContract(contract) = &mut instruction {
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        tx.call.data.user_data = serialize(&instruction).expect("serialize instruction");
        assert!(tx.verify_plan());
        assert!(!tx.verify_sig());
    }
    #[test]
    fn test_layout() {
        let tx = test_tx();
        let sign_data = tx.get_sign_data();
        let tx_bytes = serialize(&tx).unwrap();
        assert_matches!(memfind(&tx_bytes, &sign_data), Some(SIGNED_DATA_OFFSET));
        assert_matches!(memfind(&tx_bytes, tx.sig().as_ref()), Some(SIG_OFFSET));
        assert_matches!(memfind(&tx_bytes, tx.from().as_ref()), Some(PUB_KEY_OFFSET));
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let zero = Hash::default();
        let mut tx = Transaction::new(&keypair0, keypair1.pubkey(), 1, zero, 0);
        let mut instruction = tx.instruction();
        if let Instruction::NewContract(contract) = &mut instruction {
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.tokens = 2; // <-- attack!
            }
        }
        tx.call.data.user_data = serialize(&instruction).expect("serialize instruction");
        assert!(!tx.verify_plan());

        // Also, ensure all branchs of the plan spend all tokens
        if let Instruction::NewContract(contract) = &mut instruction {
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.tokens = 0; // <-- whoops!
            }
        }
        assert!(!tx.verify_plan());
    }
}
