//! The `plan` module provides a domain-specific language for payment plans. Users create Budget objects that
//! are given to an interpreter. The interpreter listens for `Witness` transactions,
//! which it uses to reduce the payment plan. When the plan is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use signature::PublicKey;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Witness {
    Timestamp(DateTime<Utc>),
    Signature(PublicKey),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    Timestamp(DateTime<Utc>),
    Signature(PublicKey),
}

impl Condition {
    /// Return true if the given Witness satisfies this Condition.
    pub fn is_satisfied(&self, witness: &Witness) -> bool {
        match (self, witness) {
            (&Condition::Signature(ref pubkey), &Witness::Signature(ref from)) => pubkey == from,
            (&Condition::Timestamp(ref dt), &Witness::Timestamp(ref last_time)) => dt <= last_time,
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Payment {
    pub tokens: i64,
    pub to: PublicKey,
}

pub trait PaymentPlan {
    /// Return Payment if the spending plan requires no additional Witnesses.
    fn final_payment(&self) -> Option<Payment>;

    /// Return true if the plan spends exactly `spendable_tokens`.
    fn verify(&self, spendable_tokens: i64) -> bool;

    /// Apply a witness to the spending plan to see if the plan can be reduced.
    /// If so, modify the plan in-place.
    fn apply_witness(&mut self, witness: &Witness);
}
