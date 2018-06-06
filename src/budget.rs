//! The `budget` module provides a domain-specific language for payment plans. Users create Budget objects that
//! are given to an interpreter. The interpreter listens for `Witness` transactions,
//! which it uses to reduce the payment plan. When the budget is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use payment_plan::{Payment, PaymentPlan, Witness};
use signature::PublicKey;
use std::mem;

/// A data type representing a `Witness` that the payment plan is waiting on.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    /// Wait for a `Timestamp` `Witness` at or after the given `DateTime`.
    Timestamp(DateTime<Utc>),

    /// Wait for a `Signature` `Witness` from `PublicKey`.
    Signature(PublicKey),
}

impl Condition {
    /// Return true if the given Witness satisfies this Condition.
    pub fn is_satisfied(&self, witness: &Witness) -> bool {
        match (self, witness) {
            (Condition::Signature(pubkey), Witness::Signature(from)) => pubkey == from,
            (Condition::Timestamp(dt), Witness::Timestamp(last_time)) => dt <= last_time,
            _ => false,
        }
    }
}

/// A data type reprsenting a payment plan.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Budget {
    /// Make a payment.
    Pay(Payment),

    /// Make a payment after some condition.
    After(Condition, Payment),

    /// Either make a payment after one condition or a different payment after another
    /// condition, which ever condition is satisfied first.
    Race((Condition, Payment), (Condition, Payment)),
}

impl Budget {
    /// Create the simplest budget - one that pays `tokens` to PublicKey.
    pub fn new_payment(tokens: i64, to: PublicKey) -> Self {
        Budget::Pay(Payment { tokens, to })
    }

    /// Create a budget that pays `tokens` to `to` after being witnessed by `from`.
    pub fn new_authorized_payment(from: PublicKey, tokens: i64, to: PublicKey) -> Self {
        Budget::After(Condition::Signature(from), Payment { tokens, to })
    }

    /// Create a budget that pays `tokens` to `to` after the given DateTime.
    pub fn new_future_payment(dt: DateTime<Utc>, tokens: i64, to: PublicKey) -> Self {
        Budget::After(Condition::Timestamp(dt), Payment { tokens, to })
    }

    /// Create a budget that pays `tokens` to `to` after the given DateTime
    /// unless cancelled by `from`.
    pub fn new_cancelable_future_payment(
        dt: DateTime<Utc>,
        from: PublicKey,
        tokens: i64,
        to: PublicKey,
    ) -> Self {
        Budget::Race(
            (Condition::Timestamp(dt), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        )
    }
}

impl PaymentPlan for Budget {
    /// Return Payment if the budget requires no additional Witnesses.
    fn final_payment(&self) -> Option<Payment> {
        match self {
            Budget::Pay(payment) => Some(payment.clone()),
            _ => None,
        }
    }

    /// Return true if the budget spends exactly `spendable_tokens`.
    fn verify(&self, spendable_tokens: i64) -> bool {
        match self {
            Budget::Pay(payment) | Budget::After(_, payment) => payment.tokens == spendable_tokens,
            Budget::Race(a, b) => a.1.tokens == spendable_tokens && b.1.tokens == spendable_tokens,
        }
    }

    /// Apply a witness to the budget to see if the budget can be reduced.
    /// If so, modify the budget in-place.
    fn apply_witness(&mut self, witness: &Witness) {
        let new_payment = match self {
            Budget::After(cond, payment) if cond.is_satisfied(witness) => Some(payment),
            Budget::Race((cond, payment), _) if cond.is_satisfied(witness) => Some(payment),
            Budget::Race(_, (cond, payment)) if cond.is_satisfied(witness) => Some(payment),
            _ => None,
        }.cloned();

        if let Some(payment) = new_payment {
            mem::replace(self, Budget::Pay(payment));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_satisfied() {
        let sig = PublicKey::default();
        assert!(Condition::Signature(sig).is_satisfied(&Witness::Signature(sig)));
    }

    #[test]
    fn test_timestamp_satisfied() {
        let dt1 = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let dt2 = Utc.ymd(2014, 11, 14).and_hms(10, 9, 8);
        assert!(Condition::Timestamp(dt1).is_satisfied(&Witness::Timestamp(dt1)));
        assert!(Condition::Timestamp(dt1).is_satisfied(&Witness::Timestamp(dt2)));
        assert!(!Condition::Timestamp(dt2).is_satisfied(&Witness::Timestamp(dt1)));
    }

    #[test]
    fn test_verify() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = PublicKey::default();
        let to = PublicKey::default();
        assert!(Budget::new_payment(42, to).verify(42));
        assert!(Budget::new_authorized_payment(from, 42, to).verify(42));
        assert!(Budget::new_future_payment(dt, 42, to).verify(42));
        assert!(Budget::new_cancelable_future_payment(dt, from, 42, to).verify(42));
    }

    #[test]
    fn test_authorized_payment() {
        let from = PublicKey::default();
        let to = PublicKey::default();

        let mut budget = Budget::new_authorized_payment(from, 42, to);
        budget.apply_witness(&Witness::Signature(from));
        assert_eq!(budget, Budget::new_payment(42, to));
    }

    #[test]
    fn test_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let to = PublicKey::default();

        let mut budget = Budget::new_future_payment(dt, 42, to);
        budget.apply_witness(&Witness::Timestamp(dt));
        assert_eq!(budget, Budget::new_payment(42, to));
    }

    #[test]
    fn test_cancelable_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = PublicKey::default();
        let to = PublicKey::default();

        let mut budget = Budget::new_cancelable_future_payment(dt, from, 42, to);
        budget.apply_witness(&Witness::Timestamp(dt));
        assert_eq!(budget, Budget::new_payment(42, to));

        let mut budget = Budget::new_cancelable_future_payment(dt, from, 42, to);
        budget.apply_witness(&Witness::Signature(from));
        assert_eq!(budget, Budget::new_payment(42, from));
    }
}
