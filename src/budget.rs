//! The `budget` module provides a domain-specific language for payment plans. Users create Budget objects that
//! are given to an interpreter. The interpreter listens for `Witness` transactions,
//! which it uses to reduce the payment plan. When the budget is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use payment_plan::{Payment, Witness};
use solana_sdk::pubkey::Pubkey;
use std::mem;

/// A data type representing a `Witness` that the payment plan is waiting on.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    /// Wait for a `Timestamp` `Witness` at or after the given `DateTime`.
    Timestamp(DateTime<Utc>, Pubkey),

    /// Wait for a `Signature` `Witness` from `Pubkey`.
    Signature(Pubkey),
}

impl Condition {
    /// Return true if the given Witness satisfies this Condition.
    pub fn is_satisfied(&self, witness: &Witness, from: &Pubkey) -> bool {
        match (self, witness) {
            (Condition::Signature(pubkey), Witness::Signature) => pubkey == from,
            (Condition::Timestamp(dt, pubkey), Witness::Timestamp(last_time)) => {
                pubkey == from && dt <= last_time
            }
            _ => false,
        }
    }
}

/// A data type representing a payment plan.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Budget {
    /// Make a payment.
    Pay(Payment),

    /// Make a payment after some condition.
    After(Condition, Payment),

    /// Either make a payment after one condition or a different payment after another
    /// condition, which ever condition is satisfied first.
    Or((Condition, Payment), (Condition, Payment)),

    /// Make a payment after both of two conditions are satisfied
    And(Condition, Condition, Payment),
}

impl Budget {
    /// Create the simplest budget - one that pays `tokens` to Pubkey.
    pub fn new_payment(tokens: i64, to: Pubkey) -> Self {
        Budget::Pay(Payment { tokens, to })
    }

    /// Create a budget that pays `tokens` to `to` after being witnessed by `from`.
    pub fn new_authorized_payment(from: Pubkey, tokens: i64, to: Pubkey) -> Self {
        Budget::After(Condition::Signature(from), Payment { tokens, to })
    }

    /// Create a budget that pays tokens` to `to` after being witnessed by 2x `from`s
    pub fn new_2_2_multisig_payment(from0: Pubkey, from1: Pubkey, tokens: i64, to: Pubkey) -> Self {
        Budget::And(
            Condition::Signature(from0),
            Condition::Signature(from1),
            Payment { tokens, to },
        )
    }

    /// Create a budget that pays `tokens` to `to` after the given DateTime.
    pub fn new_future_payment(dt: DateTime<Utc>, from: Pubkey, tokens: i64, to: Pubkey) -> Self {
        Budget::After(Condition::Timestamp(dt, from), Payment { tokens, to })
    }

    /// Create a budget that pays `tokens` to `to` after the given DateTime
    /// unless cancelled by `from`.
    pub fn new_cancelable_future_payment(
        dt: DateTime<Utc>,
        from: Pubkey,
        tokens: i64,
        to: Pubkey,
    ) -> Self {
        Budget::Or(
            (Condition::Timestamp(dt, from), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        )
    }

    /// Return Payment if the budget requires no additional Witnesses.
    pub fn final_payment(&self) -> Option<Payment> {
        match self {
            Budget::Pay(payment) => Some(payment.clone()),
            _ => None,
        }
    }

    /// Return true if the budget spends exactly `spendable_tokens`.
    pub fn verify(&self, spendable_tokens: i64) -> bool {
        match self {
            Budget::Pay(payment) | Budget::After(_, payment) | Budget::And(_, _, payment) => {
                payment.tokens == spendable_tokens
            }
            Budget::Or(a, b) => a.1.tokens == spendable_tokens && b.1.tokens == spendable_tokens,
        }
    }

    /// Apply a witness to the budget to see if the budget can be reduced.
    /// If so, modify the budget in-place.
    pub fn apply_witness(&mut self, witness: &Witness, from: &Pubkey) {
        let new_budget = match self {
            Budget::After(cond, payment) if cond.is_satisfied(witness, from) => {
                Some(Budget::Pay(payment.clone()))
            }
            Budget::Or((cond, payment), _) if cond.is_satisfied(witness, from) => {
                Some(Budget::Pay(payment.clone()))
            }
            Budget::Or(_, (cond, payment)) if cond.is_satisfied(witness, from) => {
                Some(Budget::Pay(payment.clone()))
            }
            Budget::And(cond0, cond1, payment) => {
                if cond0.is_satisfied(witness, from) {
                    Some(Budget::After(cond1.clone(), payment.clone()))
                } else if cond1.is_satisfied(witness, from) {
                    Some(Budget::After(cond0.clone(), payment.clone()))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(budget) = new_budget {
            mem::replace(self, budget);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signature::{Keypair, KeypairUtil};

    #[test]
    fn test_signature_satisfied() {
        let from = Pubkey::default();
        assert!(Condition::Signature(from).is_satisfied(&Witness::Signature, &from));
    }

    #[test]
    fn test_timestamp_satisfied() {
        let dt1 = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let dt2 = Utc.ymd(2014, 11, 14).and_hms(10, 9, 8);
        let from = Pubkey::default();
        assert!(Condition::Timestamp(dt1, from).is_satisfied(&Witness::Timestamp(dt1), &from));
        assert!(Condition::Timestamp(dt1, from).is_satisfied(&Witness::Timestamp(dt2), &from));
        assert!(!Condition::Timestamp(dt2, from).is_satisfied(&Witness::Timestamp(dt1), &from));
    }

    #[test]
    fn test_verify() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Pubkey::default();
        let to = Pubkey::default();
        assert!(Budget::new_payment(42, to).verify(42));
        assert!(Budget::new_authorized_payment(from, 42, to).verify(42));
        assert!(Budget::new_future_payment(dt, from, 42, to).verify(42));
        assert!(Budget::new_cancelable_future_payment(dt, from, 42, to).verify(42));
    }

    #[test]
    fn test_authorized_payment() {
        let from = Pubkey::default();
        let to = Pubkey::default();

        let mut budget = Budget::new_authorized_payment(from, 42, to);
        budget.apply_witness(&Witness::Signature, &from);
        assert_eq!(budget, Budget::new_payment(42, to));
    }

    #[test]
    fn test_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();

        let mut budget = Budget::new_future_payment(dt, from, 42, to);
        budget.apply_witness(&Witness::Timestamp(dt), &from);
        assert_eq!(budget, Budget::new_payment(42, to));
    }

    #[test]
    fn test_unauthorized_future_payment() {
        // Ensure timestamp will only be acknowledged if it came from the
        // whitelisted public key.
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();

        let mut budget = Budget::new_future_payment(dt, from, 42, to);
        let orig_budget = budget.clone();
        budget.apply_witness(&Witness::Timestamp(dt), &to); // <-- Attack!
        assert_eq!(budget, orig_budget);
    }

    #[test]
    fn test_cancelable_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Pubkey::default();
        let to = Pubkey::default();

        let mut budget = Budget::new_cancelable_future_payment(dt, from, 42, to);
        budget.apply_witness(&Witness::Timestamp(dt), &from);
        assert_eq!(budget, Budget::new_payment(42, to));

        let mut budget = Budget::new_cancelable_future_payment(dt, from, 42, to);
        budget.apply_witness(&Witness::Signature, &from);
        assert_eq!(budget, Budget::new_payment(42, from));
    }
    #[test]
    fn test_2_2_multisig_payment() {
        let from0 = Keypair::new().pubkey();
        let from1 = Keypair::new().pubkey();
        let to = Pubkey::default();

        let mut budget = Budget::new_2_2_multisig_payment(from0, from1, 42, to);
        budget.apply_witness(&Witness::Signature, &from0);
        assert_eq!(budget, Budget::new_authorized_payment(from1, 42, to));
    }
}
