//! The `plan` module provides a domain-specific language for payment plans. Users create Budget objects that
//! are given to an interpreter. The interpreter listens for `Witness` transactions,
//! which it uses to reduce the payment plan. When the plan is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use signature::PublicKey;

/// The types of events a payment plan can process.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Witness {
    /// The current time.
    Timestamp(DateTime<Utc>),

    /// A siganture from PublicKey.
    Signature,
}

/// Some amount of tokens that should be sent to the `to` `PublicKey`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Payment {
    /// Amount to be paid.
    pub tokens: i64,

    /// The `PublicKey` that `tokens` should be paid to.
    pub to: PublicKey,
}

/// Interface to smart contracts.
pub trait PaymentPlan {
    /// Return Payment if the payment plan requires no additional Witnesses.
    fn final_payment(&self) -> Option<Payment>;

    /// Return true if the plan spends exactly `spendable_tokens`.
    fn verify(&self, spendable_tokens: i64) -> bool;

    /// Apply a witness to the payment plan to see if the plan can be reduced.
    /// If so, modify the plan in-place.
    fn apply_witness(&mut self, witness: &Witness, from: &PublicKey);
}
