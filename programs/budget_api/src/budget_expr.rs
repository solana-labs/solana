//! The `budget_expr` module provides a domain-specific language for pa&yment plans. Users create BudgetExpr objects that
//! are given to an interpreter. The interpreter listens for `Witness` transactions,
//! which it uses to reduce the payment plan. When the budget is reduced to a
//! `Payment`, the payment is executed.

use chrono::prelude::*;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use std::mem;

/// The types of events a payment plan can process.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Witness {
    /// The current time.
    Timestamp(DateTime<Utc>),

    /// A signature from Pubkey.
    Signature,

    /// Account snapshot.
    AccountData(Hash, Pubkey),
}

/// Some amount of lamports that should be sent to the `to` `Pubkey`.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Payment {
    /// Amount to be paid.
    pub lamports: u64,

    /// The `Pubkey` that `lamports` should be paid to.
    pub to: Pubkey,
}

/// The account constraints a Condition would wait on.
/// Note: ideally this would be function that accepts an Account and returns
/// a bool, but we don't have a way to pass functions over the wire. To simulate
/// higher order programming, create your own program that includes an instruction
/// that sets account data to a boolean. Pass that account key and program_id here.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct AccountConstraints {
    /// The account holder.
    pub key: Pubkey,

    /// The program id that must own the account at `key`.
    pub program_id: Pubkey,

    /// The hash of the data in the account at `key`.
    pub data_hash: Hash,
}

/// A data type representing a `Witness` that the payment plan is waiting on.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    /// Wait for a `Timestamp` `Witness` at or after the given `DateTime`.
    Timestamp(DateTime<Utc>, Pubkey),

    /// Wait for a `Signature` `Witness` from `Pubkey`.
    Signature(Pubkey),

    /// Wait for the account with the given constraints.
    AccountData(AccountConstraints),
}

impl Condition {
    /// Return true if the given Witness satisfies this Condition.
    pub fn is_satisfied(&self, witness: &Witness, from: &Pubkey) -> bool {
        match (self, witness) {
            (Condition::Signature(pubkey), Witness::Signature) => pubkey == from,
            (Condition::Timestamp(dt, pubkey), Witness::Timestamp(last_time)) => {
                pubkey == from && dt <= last_time
            }
            (
                Condition::AccountData(constraints),
                Witness::AccountData(actual_hash, program_id),
            ) => {
                constraints.program_id == *program_id
                    && constraints.key == *from
                    && constraints.data_hash == *actual_hash
            }
            _ => false,
        }
    }
}

/// A data type representing a payment plan.
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum BudgetExpr {
    /// Make a payment.
    Pay(Payment),

    /// Make a payment after some condition.
    After(Condition, Box<BudgetExpr>),

    /// Either make a payment after one condition or a different payment after another
    /// condition, which ever condition is satisfied first.
    Or((Condition, Box<BudgetExpr>), (Condition, Box<BudgetExpr>)),

    /// Make a payment after both of two conditions are satisfied
    And(Condition, Condition, Box<BudgetExpr>),
}

impl BudgetExpr {
    /// Create the simplest budget - one that pays `lamports` to Pubkey.
    pub fn new_payment(lamports: u64, to: &Pubkey) -> Self {
        BudgetExpr::Pay(Payment { lamports, to: *to })
    }

    /// Create a budget that pays `lamports` to `to` after being witnessed by `from`.
    pub fn new_authorized_payment(from: &Pubkey, lamports: u64, to: &Pubkey) -> Self {
        BudgetExpr::After(
            Condition::Signature(*from),
            Box::new(Self::new_payment(lamports, to)),
        )
    }

    /// Create a budget that pays `lamports` to `to` after witnessing account data in `account_pubkey` with the given hash.
    pub fn new_payment_when_account_data(
        account_pubkey: &Pubkey,
        account_program_id: &Pubkey,
        account_hash: Hash,
        lamports: u64,
        to: &Pubkey,
    ) -> Self {
        BudgetExpr::After(
            Condition::AccountData(AccountConstraints {
                key: *account_pubkey,
                program_id: *account_program_id,
                data_hash: account_hash,
            }),
            Box::new(Self::new_payment(lamports, to)),
        )
    }

    /// Create a budget that pays `lamports` to `to` after being witnessed by `witness` unless
    /// canceled with a signature from `from`.
    pub fn new_cancelable_authorized_payment(
        witness: &Pubkey,
        lamports: u64,
        to: &Pubkey,
        from: Option<Pubkey>,
    ) -> Self {
        if from.is_none() {
            return Self::new_authorized_payment(witness, lamports, to);
        }
        let from = from.unwrap();
        BudgetExpr::Or(
            (
                Condition::Signature(*witness),
                Box::new(BudgetExpr::new_payment(lamports, to)),
            ),
            (
                Condition::Signature(from),
                Box::new(BudgetExpr::new_payment(lamports, &from)),
            ),
        )
    }

    /// Create a budget that pays lamports` to `to` after being witnessed by 2x `from`s
    pub fn new_2_2_multisig_payment(
        from0: &Pubkey,
        from1: &Pubkey,
        lamports: u64,
        to: &Pubkey,
    ) -> Self {
        BudgetExpr::And(
            Condition::Signature(*from0),
            Condition::Signature(*from1),
            Box::new(Self::new_payment(lamports, to)),
        )
    }

    /// Create a budget that pays `lamports` to `to` after the given DateTime signed
    /// by `dt_pubkey`.
    pub fn new_future_payment(
        dt: DateTime<Utc>,
        dt_pubkey: &Pubkey,
        lamports: u64,
        to: &Pubkey,
    ) -> Self {
        BudgetExpr::After(
            Condition::Timestamp(dt, *dt_pubkey),
            Box::new(Self::new_payment(lamports, to)),
        )
    }

    /// Create a budget that pays `lamports` to `to` after the given DateTime
    /// signed by `dt_pubkey` unless canceled by `from`.
    pub fn new_cancelable_future_payment(
        dt: DateTime<Utc>,
        dt_pubkey: &Pubkey,
        lamports: u64,
        to: &Pubkey,
        from: Option<Pubkey>,
    ) -> Self {
        if from.is_none() {
            return Self::new_future_payment(dt, dt_pubkey, lamports, to);
        }
        let from = from.unwrap();
        BudgetExpr::Or(
            (
                Condition::Timestamp(dt, *dt_pubkey),
                Box::new(Self::new_payment(lamports, to)),
            ),
            (
                Condition::Signature(from),
                Box::new(Self::new_payment(lamports, &from)),
            ),
        )
    }

    /// Return Payment if the budget requires no additional Witnesses.
    pub fn final_payment(&self) -> Option<Payment> {
        match self {
            BudgetExpr::Pay(payment) => Some(payment.clone()),
            _ => None,
        }
    }

    /// Return true if the budget spends exactly `spendable_lamports`.
    pub fn verify(&self, spendable_lamports: u64) -> bool {
        match self {
            BudgetExpr::Pay(payment) => payment.lamports == spendable_lamports,
            BudgetExpr::After(_, sub_expr) | BudgetExpr::And(_, _, sub_expr) => {
                sub_expr.verify(spendable_lamports)
            }
            BudgetExpr::Or(a, b) => {
                a.1.verify(spendable_lamports) && b.1.verify(spendable_lamports)
            }
        }
    }

    /// Apply a witness to the budget to see if the budget can be reduced.
    /// If so, modify the budget in-place.
    pub fn apply_witness(&mut self, witness: &Witness, from: &Pubkey) {
        let new_expr = match self {
            BudgetExpr::After(cond, sub_expr) if cond.is_satisfied(witness, from) => {
                Some(sub_expr.clone())
            }
            BudgetExpr::Or((cond, sub_expr), _) if cond.is_satisfied(witness, from) => {
                Some(sub_expr.clone())
            }
            BudgetExpr::Or(_, (cond, sub_expr)) if cond.is_satisfied(witness, from) => {
                Some(sub_expr.clone())
            }
            BudgetExpr::And(cond0, cond1, sub_expr) => {
                if cond0.is_satisfied(witness, from) {
                    Some(Box::new(BudgetExpr::After(cond1.clone(), sub_expr.clone())))
                } else if cond1.is_satisfied(witness, from) {
                    Some(Box::new(BudgetExpr::After(cond0.clone(), sub_expr.clone())))
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(expr) = new_expr {
            mem::replace(self, *expr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(BudgetExpr::new_payment(42, &to).verify(42));
        assert!(BudgetExpr::new_authorized_payment(&from, 42, &to).verify(42));
        assert!(BudgetExpr::new_future_payment(dt, &from, 42, &to).verify(42));
        assert!(
            BudgetExpr::new_cancelable_future_payment(dt, &from, 42, &to, Some(from)).verify(42)
        );
    }

    #[test]
    fn test_authorized_payment() {
        let from = Pubkey::default();
        let to = Pubkey::default();

        let mut expr = BudgetExpr::new_authorized_payment(&from, 42, &to);
        expr.apply_witness(&Witness::Signature, &from);
        assert_eq!(expr, BudgetExpr::new_payment(42, &to));
    }

    #[test]
    fn test_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Pubkey::new_rand();
        let to = Pubkey::new_rand();

        let mut expr = BudgetExpr::new_future_payment(dt, &from, 42, &to);
        expr.apply_witness(&Witness::Timestamp(dt), &from);
        assert_eq!(expr, BudgetExpr::new_payment(42, &to));
    }

    #[test]
    fn test_unauthorized_future_payment() {
        // Ensure timestamp will only be acknowledged if it came from the
        // whitelisted public key.
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Pubkey::new_rand();
        let to = Pubkey::new_rand();

        let mut expr = BudgetExpr::new_future_payment(dt, &from, 42, &to);
        let orig_expr = expr.clone();
        expr.apply_witness(&Witness::Timestamp(dt), &to); // <-- Attack!
        assert_eq!(expr, orig_expr);
    }

    #[test]
    fn test_cancelable_future_payment() {
        let dt = Utc.ymd(2014, 11, 14).and_hms(8, 9, 10);
        let from = Pubkey::default();
        let to = Pubkey::default();

        let mut expr = BudgetExpr::new_cancelable_future_payment(dt, &from, 42, &to, Some(from));
        expr.apply_witness(&Witness::Timestamp(dt), &from);
        assert_eq!(expr, BudgetExpr::new_payment(42, &to));

        let mut expr = BudgetExpr::new_cancelable_future_payment(dt, &from, 42, &to, Some(from));
        expr.apply_witness(&Witness::Signature, &from);
        assert_eq!(expr, BudgetExpr::new_payment(42, &from));
    }
    #[test]
    fn test_2_2_multisig_payment() {
        let from0 = Pubkey::new_rand();
        let from1 = Pubkey::new_rand();
        let to = Pubkey::default();

        let mut expr = BudgetExpr::new_2_2_multisig_payment(&from0, &from1, 42, &to);
        expr.apply_witness(&Witness::Signature, &from0);
        assert_eq!(expr, BudgetExpr::new_authorized_payment(&from1, 42, &to));
    }

    #[test]
    fn test_multisig_after_sig() {
        let from0 = Pubkey::new_rand();
        let from1 = Pubkey::new_rand();
        let from2 = Pubkey::new_rand();
        let to = Pubkey::default();

        let expr = BudgetExpr::new_2_2_multisig_payment(&from0, &from1, 42, &to);
        let mut expr = BudgetExpr::After(Condition::Signature(from2), Box::new(expr));

        expr.apply_witness(&Witness::Signature, &from2);
        expr.apply_witness(&Witness::Signature, &from0);
        assert_eq!(expr, BudgetExpr::new_authorized_payment(&from1, 42, &to));
    }

    #[test]
    fn test_multisig_after_ts() {
        let from0 = Pubkey::new_rand();
        let from1 = Pubkey::new_rand();
        let dt = Utc.ymd(2014, 11, 11).and_hms(7, 7, 7);
        let to = Pubkey::default();

        let expr = BudgetExpr::new_2_2_multisig_payment(&from0, &from1, 42, &to);
        let mut expr = BudgetExpr::After(Condition::Timestamp(dt, from0), Box::new(expr));

        expr.apply_witness(&Witness::Timestamp(dt), &from0);
        assert_eq!(
            expr,
            BudgetExpr::new_2_2_multisig_payment(&from0, &from1, 42, &to)
        );

        expr.apply_witness(&Witness::Signature, &from0);
        assert_eq!(expr, BudgetExpr::new_authorized_payment(&from1, 42, &to));
    }
}
