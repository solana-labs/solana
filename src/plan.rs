//! The `plan` crate provides functionality for creating spending plans.

use signature::PublicKey;
use chrono::prelude::*;
use std::mem;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Condition {
    Timestamp(DateTime<Utc>),
    Signature(PublicKey),
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Action {
    Pay(Payment),
}

impl Action {
    pub fn spendable(&self) -> i64 {
        match *self {
            Action::Pay(ref payment) => payment.asset.clone(),
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
    After(Condition, Box<Plan>),
    Race(Box<Plan>, Box<Plan>),
}

impl Plan {
    pub fn verify(&self, spendable_assets: i64) -> bool {
        match *self {
            Plan::Action(ref action) => action.spendable() == spendable_assets,
            Plan::Race(ref plan_a, ref plan_b) => {
                plan_a.verify(spendable_assets) && plan_b.verify(spendable_assets)
            }
            Plan::After(_, ref plan) => plan.verify(spendable_assets),
        }
    }

    pub fn run_race(&mut self) -> bool {
        let new_plan = if let Plan::Race(ref a, ref b) = *self {
            if let Plan::Action(_) = **a {
                Some((**a).clone())
            } else if let Plan::Action(_) = **b {
                Some((**b).clone())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(plan) = new_plan {
            mem::replace(self, plan);
            true
        } else {
            false
        }
    }

    pub fn process_verified_sig(&mut self, from: PublicKey) -> bool {
        let mut new_plan = None;
        match *self {
            Plan::Action(_) => return true,
            Plan::Race(ref mut plan_a, ref mut plan_b) => {
                plan_a.process_verified_sig(from);
                plan_b.process_verified_sig(from);
            }
            Plan::After(Condition::Signature(pubkey), ref plan) => {
                if from == pubkey {
                    new_plan = Some((**plan).clone());
                }
            }
            _ => (),
        }
        if self.run_race() {
            return true;
        }

        if let Some(plan) = new_plan {
            mem::replace(self, plan);
            true
        } else {
            false
        }
    }

    pub fn process_verified_timestamp(&mut self, last_time: DateTime<Utc>) -> bool {
        let mut new_plan = None;
        match *self {
            Plan::Action(_) => return true,
            Plan::Race(ref mut plan_a, ref mut plan_b) => {
                plan_a.process_verified_timestamp(last_time);
                plan_b.process_verified_timestamp(last_time);
            }
            Plan::After(Condition::Timestamp(dt), ref plan) => {
                if dt <= last_time {
                    new_plan = Some((**plan).clone());
                }
            }
            _ => (),
        }
        if self.run_race() {
            return true;
        }

        if let Some(plan) = new_plan {
            mem::replace(self, plan);
            true
        } else {
            false
        }
    }
}
