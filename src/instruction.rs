use budget::Budget;
use chrono::prelude::{DateTime, Utc};
use payment_plan::{Payment, PaymentPlan, Witness};
use signature::Pubkey;
use signature::Signature;

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

    fn apply_witness(&mut self, witness: &Witness, from: &Pubkey) {
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
    /// signed by the containing transaction's `Pubkey`.
    ApplySignature(Signature),

    /// Vote for a PoH that is equal to the lastid of this transaction
    NewVote(Vote),
}
