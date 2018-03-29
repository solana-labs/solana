//! A domain-specific language for payment plans. Users create Plan objects that
//! are given to an interpreter. The interpreter listens for `Witness` events,
//! which it uses to reduce the payment plan. When the plan is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use signature::PublicKey;
use std::mem;

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

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Plan {
    Pay(Payment),
    After(Condition, Payment),
    Race((Condition, Payment), (Condition, Payment)),
}

impl Plan {
    /// Create the simplest spending plan - one that pays `tokens` to PublicKey.
    pub fn new_payment(tokens: i64, to: PublicKey) -> Self {
        Plan::Pay(Payment { tokens, to })
    }

    /// Create a spending plan that pays `tokens` to `to` after being witnessed by `from`.
    pub fn new_authorized_payment(from: PublicKey, tokens: i64, to: PublicKey) -> Self {
        Plan::After(Condition::Signature(from), Payment { tokens, to })
    }

    /// Create a spending plan that pays `tokens` to `to` after the given DateTime.
    pub fn new_future_payment(dt: DateTime<Utc>, tokens: i64, to: PublicKey) -> Self {
        Plan::After(Condition::Timestamp(dt), Payment { tokens, to })
    }

    /// Create a spending plan that pays `tokens` to `to` after the given DateTime
    /// unless cancelled by `from`.
    pub fn new_cancelable_future_payment(
        dt: DateTime<Utc>,
        from: PublicKey,
        tokens: i64,
        to: PublicKey,
    ) -> Self {
        Plan::Race(
            (Condition::Timestamp(dt), Payment { tokens, to }),
            (Condition::Signature(from), Payment { tokens, to: from }),
        )
    }

    /// Return true if the spending plan requires no additional Witnesses.
    pub fn is_complete(&self) -> bool {
        match *self {
            Plan::Pay(_) => true,
            _ => false,
        }
    }

    /// Return true if the plan spends exactly `spendable_tokens`.
    pub fn verify(&self, spendable_tokens: i64) -> bool {
        match *self {
            Plan::Pay(ref payment) | Plan::After(_, ref payment) => {
                payment.tokens == spendable_tokens
            }
            Plan::Race(ref a, ref b) => {
                a.1.tokens == spendable_tokens && b.1.tokens == spendable_tokens
            }
        }
    }

    /// Apply a witness to the spending plan to see if the plan can be reduced.
    /// If so, modify the plan in-place.
    pub fn apply_witness(&mut self, witness: &Witness) {
        let new_payment = match *self {
            Plan::After(ref cond, ref payment) if cond.is_satisfied(witness) => Some(payment),
            Plan::Race((ref cond, ref payment), _) if cond.is_satisfied(witness) => Some(payment),
            Plan::Race(_, (ref cond, ref payment)) if cond.is_satisfied(witness) => Some(payment),
            _ => None,
        }.cloned();

        if let Some(payment) = new_payment {
            mem::replace(self, Plan::Pay(payment));
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
    fn test_verify_plan() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = PublicKey::default();
        let to = PublicKey::default();
        assert!(Plan::new_payment(42, to).verify(42));
        assert!(Plan::new_authorized_payment(from, 42, to).verify(42));
        assert!(Plan::new_future_payment(dt, 42, to).verify(42));
        assert!(Plan::new_cancelable_future_payment(dt, from, 42, to).verify(42));
    }

    #[test]
    fn test_authorized_payment() {
        let from = PublicKey::default();
        let to = PublicKey::default();

        let mut plan = Plan::new_authorized_payment(from, 42, to);
        plan.apply_witness(&Witness::Signature(from));
        assert_eq!(plan, Plan::new_payment(42, to));
    }

    #[test]
    fn test_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let to = PublicKey::default();

        let mut plan = Plan::new_future_payment(dt, 42, to);
        plan.apply_witness(&Witness::Timestamp(dt));
        assert_eq!(plan, Plan::new_payment(42, to));
    }

    #[test]
    fn test_cancelable_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = PublicKey::default();
        let to = PublicKey::default();

        let mut plan = Plan::new_cancelable_future_payment(dt, from, 42, to);
        plan.apply_witness(&Witness::Timestamp(dt));
        assert_eq!(plan, Plan::new_payment(42, to));

        let mut plan = Plan::new_cancelable_future_payment(dt, from, 42, to);
        plan.apply_witness(&Witness::Signature(from));
        assert_eq!(plan, Plan::new_payment(42, from));
    }
}
