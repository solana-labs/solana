//! The `transaction` module provides functionality for creating log transactions.

use bincode::serialize;
use budget::{Budget, Condition};
use chrono::prelude::*;
use hash::Hash;
use payment_plan::{Payment, PaymentPlan, Witness};
use signature::{KeyPair, KeyPairUtil, PublicKey, Signature, SignatureUtil};

pub const SIGNED_DATA_OFFSET: usize = 112;
pub const SIG_OFFSET: usize = 8;
pub const PUB_KEY_OFFSET: usize = 80;

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
    /// A digital signature of `instruction`, `last_id` and `fee`, signed by `PublicKey`.
    pub sig: Signature,

    /// The `PublicKey` of the entity that signed the transaction data.
    pub from: PublicKey,

    /// The action the server should take.
    pub instruction: Instruction,

    /// The ID of a recent ledger entry.
    pub last_id: Hash,

    /// The number of tokens paid for processing and storage of this transaction.
    pub fee: i64,
}

impl Transaction {
    /// Create a signed transaction from the given `Instruction`.
    fn new_from_instruction(
        from_keypair: &KeyPair,
        instruction: Instruction,
        last_id: Hash,
        fee: i64,
    ) -> Self {
        let from = from_keypair.pubkey();
        let mut tx = Transaction {
            sig: Signature::default(),
            instruction,
            last_id,
            from,
            fee,
        };
        tx.sign(from_keypair);
        tx
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new_taxed(
        from_keypair: &KeyPair,
        to: PublicKey,
        tokens: i64,
        fee: i64,
        last_id: Hash,
    ) -> Self {
        let payment = Payment {
            tokens: tokens - fee,
            to,
        };
        let budget = Budget::Pay(payment);
        let plan = Plan::Budget(budget);
        let instruction = Instruction::NewContract(Contract { plan, tokens });
        Self::new_from_instruction(from_keypair, instruction, last_id, fee)
    }

    /// Create and sign a new Transaction. Used for unit-testing.
    pub fn new(from_keypair: &KeyPair, to: PublicKey, tokens: i64, last_id: Hash) -> Self {
        Self::new_taxed(from_keypair, to, tokens, 0, last_id)
    }

    /// Create and sign a new Witness Timestamp. Used for unit-testing.
    pub fn new_timestamp(from_keypair: &KeyPair, dt: DateTime<Utc>, last_id: Hash) -> Self {
        let instruction = Instruction::ApplyTimestamp(dt);
        Self::new_from_instruction(from_keypair, instruction, last_id, 0)
    }

    /// Create and sign a new Witness Signature. Used for unit-testing.
    pub fn new_signature(from_keypair: &KeyPair, tx_sig: Signature, last_id: Hash) -> Self {
        let instruction = Instruction::ApplySignature(tx_sig);
        Self::new_from_instruction(from_keypair, instruction, last_id, 0)
    }

    pub fn new_vote(from_keypair: &KeyPair, vote: Vote, last_id: Hash, fee: i64) -> Self {
        Transaction::new_from_instruction(&from_keypair, Instruction::NewVote(vote), last_id, fee)
    }

    /// Create and sign a postdated Transaction. Used for unit-testing.
    pub fn new_on_date(
        from_keypair: &KeyPair,
        to: PublicKey,
        dt: DateTime<Utc>,
        tokens: i64,
        last_id: Hash,
    ) -> Self {
        let from = from_keypair.pubkey();
        let budget = Budget::Or(
            (Condition::Timestamp(dt, from), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        );
        let plan = Plan::Budget(budget);
        let instruction = Instruction::NewContract(Contract { plan, tokens });
        Self::new_from_instruction(from_keypair, instruction, last_id, 0)
    }

    /// Get the transaction data to sign.
    fn get_sign_data(&self) -> Vec<u8> {
        let mut data = serialize(&(&self.instruction)).expect("serialize Contract");
        let last_id_data = serialize(&(&self.last_id)).expect("serialize last_id");
        data.extend_from_slice(&last_id_data);

        let fee_data = serialize(&(&self.fee)).expect("serialize last_id");
        data.extend_from_slice(&fee_data);

        data
    }

    /// Sign this transaction.
    pub fn sign(&mut self, keypair: &KeyPair) {
        let sign_data = self.get_sign_data();
        self.sig = Signature::clone_from_slice(keypair.sign(&sign_data).as_ref());
    }

    /// Verify only the transaction signature.
    pub fn verify_sig(&self) -> bool {
        warn!("transaction signature verification called");
        self.sig.verify(&self.from.as_ref(), &self.get_sign_data())
    }

    /// Verify only the payment plan.
    pub fn verify_plan(&self) -> bool {
        if let Instruction::NewContract(contract) = &self.instruction {
            self.fee >= 0
                && self.fee <= contract.tokens
                && contract.plan.verify(contract.tokens - self.fee)
        } else {
            true
        }
    }
}

pub fn test_tx() -> Transaction {
    let keypair1 = KeyPair::new();
    let pubkey1 = keypair1.pubkey();
    let zero = Hash::default();
    Transaction::new(&keypair1, pubkey1, 42, zero)
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
        let tx0 = Transaction::new(&keypair, keypair.pubkey(), 42, zero);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer() {
        let zero = Hash::default();
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let pubkey1 = keypair1.pubkey();
        let tx0 = Transaction::new(&keypair0, pubkey1, 42, zero);
        assert!(tx0.verify_plan());
    }

    #[test]
    fn test_transfer_with_fee() {
        let zero = Hash::default();
        let keypair0 = KeyPair::new();
        let pubkey1 = KeyPair::new().pubkey();
        assert!(Transaction::new_taxed(&keypair0, pubkey1, 1, 1, zero).verify_plan());
        assert!(!Transaction::new_taxed(&keypair0, pubkey1, 1, 2, zero).verify_plan());
        assert!(!Transaction::new_taxed(&keypair0, pubkey1, 1, -1, zero).verify_plan());
    }

    #[test]
    fn test_serialize_claim() {
        let budget = Budget::Pay(Payment {
            tokens: 0,
            to: Default::default(),
        });
        let plan = Plan::Budget(budget);
        let instruction = Instruction::NewContract(Contract { plan, tokens: 0 });
        let claim0 = Transaction {
            instruction,
            from: Default::default(),
            last_id: Default::default(),
            sig: Default::default(),
            fee: 0,
        };
        let buf = serialize(&claim0).unwrap();
        let claim1: Transaction = deserialize(&buf).unwrap();
        assert_eq!(claim1, claim0);
    }

    #[test]
    fn test_token_attack() {
        let zero = Hash::default();
        let keypair = KeyPair::new();
        let pubkey = keypair.pubkey();
        let mut tx = Transaction::new(&keypair, pubkey, 42, zero);
        if let Instruction::NewContract(contract) = &mut tx.instruction {
            contract.tokens = 1_000_000; // <-- attack, part 1!
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.tokens = contract.tokens; // <-- attack, part 2!
            }
        }
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
        let mut tx = Transaction::new(&keypair0, pubkey1, 42, zero);
        if let Instruction::NewContract(contract) = &mut tx.instruction {
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.to = thief_keypair.pubkey(); // <-- attack!
            }
        }
        assert!(tx.verify_plan());
        assert!(!tx.verify_sig());
    }
    #[test]
    fn test_layout() {
        let tx = test_tx();
        let sign_data = tx.get_sign_data();
        let tx_bytes = serialize(&tx).unwrap();
        assert_matches!(memfind(&tx_bytes, &sign_data), Some(SIGNED_DATA_OFFSET));
        assert_matches!(memfind(&tx_bytes, &tx.sig), Some(SIG_OFFSET));
        assert_matches!(memfind(&tx_bytes, &tx.from.as_ref()), Some(PUB_KEY_OFFSET));
    }

    #[test]
    fn test_overspend_attack() {
        let keypair0 = KeyPair::new();
        let keypair1 = KeyPair::new();
        let zero = Hash::default();
        let mut tx = Transaction::new(&keypair0, keypair1.pubkey(), 1, zero);
        if let Instruction::NewContract(contract) = &mut tx.instruction {
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.tokens = 2; // <-- attack!
            }
        }
        assert!(!tx.verify_plan());

        // Also, ensure all branchs of the plan spend all tokens
        if let Instruction::NewContract(contract) = &mut tx.instruction {
            if let Plan::Budget(Budget::Pay(ref mut payment)) = contract.plan {
                payment.tokens = 0; // <-- whoops!
            }
        }
        assert!(!tx.verify_plan());
    }
}
