//! The `plan` module provides a domain-specific language for payment plans. Users create BudgetExpr objects that
//! are given to an interpreter. The interpreter listens for `Witness` transactions,
//! which it uses to reduce the payment plan. When the plan is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use solana_sdk::pubkey::Pubkey;

/// The types of events a payment plan can process.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Witness {
    /// The current time.
    Timestamp(DateTime<Utc>),

    /// A signature from Pubkey.
    Signature,
}

/// Some amount of tokens that should be sent to the `to` `Pubkey`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Payment {
    /// Amount to be paid.
    pub tokens: u64,

    /// The `Pubkey` that `tokens` should be paid to.
    pub to: Pubkey,
}
