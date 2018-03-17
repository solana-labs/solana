//! The `plan` crate provides functionality for creating spending plans.

use signature::PublicKey;
use chrono::prelude::*;
use std::mem;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    Timestamp(DateTime<Utc>),
    Signature(PublicKey),
}

pub enum PlanEvent {
    Timestamp(DateTime<Utc>),
    Signature(PublicKey),
}

impl Condition {
    pub fn is_satisfied(&self, event: &PlanEvent) -> bool {
        match (self, event) {
            (&Condition::Signature(ref pubkey), &PlanEvent::Signature(ref from)) => pubkey == from,
            (&Condition::Timestamp(ref dt), &PlanEvent::Timestamp(ref last_time)) => {
                dt <= last_time
            }
            _ => false,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Action {
    Pay(Payment),
}

impl Action {
    pub fn spendable(&self) -> i64 {
        match *self {
            Action::Pay(ref payment) => payment.asset,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Payment {
    pub asset: i64,
    pub to: PublicKey,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Plan {
    Action(Action),
    After(Condition, Action),
    Race((Condition, Action), (Condition, Action)),
}

impl Plan {
    pub fn verify(&self, spendable_assets: i64) -> bool {
        match *self {
            Plan::Action(ref action) => action.spendable() == spendable_assets,
            Plan::After(_, ref action) => action.spendable() == spendable_assets,
            Plan::Race(ref a, ref b) => {
                a.1.spendable() == spendable_assets && b.1.spendable() == spendable_assets
            }
        }
    }

    pub fn process_event(&mut self, event: PlanEvent) -> bool {
        let mut new_action = None;
        match *self {
            Plan::Action(_) => return true,
            Plan::After(ref cond, ref action) => {
                if cond.is_satisfied(&event) {
                    new_action = Some(action.clone());
                }
            }
            Plan::Race(ref a, ref b) => {
                if a.0.is_satisfied(&event) {
                    new_action = Some(a.1.clone());
                } else if b.0.is_satisfied(&event) {
                    new_action = Some(b.1.clone());
                }
            }
        }

        if let Some(action) = new_action {
            mem::replace(self, Plan::Action(action));
            true
        } else {
            false
        }
    }
}
