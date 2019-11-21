//! vest state
use crate::vest_schedule::create_vesting_schedule;
use bincode::{self, deserialize, serialize_into};
use chrono::prelude::*;
use chrono::{
    prelude::{DateTime, TimeZone, Utc},
    serde::ts_seconds,
};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{account::Account, instruction::InstructionError, pubkey::Pubkey};
use std::cmp::min;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VestState {
    /// The address authorized to terminate this contract with a signed Terminate instruction
    pub terminator_pubkey: Pubkey,

    /// The address authorized to redeem vested tokens
    pub payee_pubkey: Pubkey,

    /// The day from which the vesting contract begins
    #[serde(with = "ts_seconds")]
    pub start_date_time: DateTime<Utc>,

    /// Address of an account containing a trusted date, used to drive the vesting schedule
    pub date_pubkey: Pubkey,

    /// The number of lamports to send the payee if the schedule completes
    pub total_lamports: u64,

    /// The number of lamports the payee has already redeemed
    pub redeemed_lamports: u64,

    /// The number of lamports the terminator repurchased
    pub reneged_lamports: u64,

    /// True if the terminator has declared this contract fully vested.
    pub is_fully_vested: bool,
}

impl Default for VestState {
    fn default() -> Self {
        Self {
            terminator_pubkey: Pubkey::default(),
            payee_pubkey: Pubkey::default(),
            start_date_time: Utc.timestamp(0, 0),
            date_pubkey: Pubkey::default(),
            total_lamports: 0,
            redeemed_lamports: 0,
            reneged_lamports: 0,
            is_fully_vested: false,
        }
    }
}

impl VestState {
    pub fn serialize(&self, output: &mut [u8]) -> Result<(), InstructionError> {
        serialize_into(output, self).map_err(|_| InstructionError::AccountDataTooSmall)
    }

    pub fn deserialize(input: &[u8]) -> Result<Self, InstructionError> {
        deserialize(input).map_err(|_| InstructionError::InvalidAccountData)
    }

    fn calc_vested_lamports(&self, current_date: Date<Utc>) -> u64 {
        let total_lamports_after_reneged = self.total_lamports - self.reneged_lamports;
        if self.is_fully_vested {
            return total_lamports_after_reneged;
        }

        let schedule = create_vesting_schedule(self.start_date_time.date(), self.total_lamports);

        let vested_lamports = schedule
            .into_iter()
            .take_while(|(dt, _)| *dt <= current_date)
            .map(|(_, lamports)| lamports)
            .sum::<u64>();

        min(vested_lamports, total_lamports_after_reneged)
    }

    /// Redeem vested tokens.
    pub fn redeem_tokens(
        &mut self,
        contract_account: &mut Account,
        current_date: Date<Utc>,
        payee_account: &mut Account,
    ) {
        let vested_lamports = self.calc_vested_lamports(current_date);
        let redeemable_lamports = vested_lamports.saturating_sub(self.redeemed_lamports);

        contract_account.lamports -= redeemable_lamports;
        payee_account.lamports += redeemable_lamports;

        self.redeemed_lamports += redeemable_lamports;
    }

    /// Renege on the given number of tokens and send them to the given payee.
    pub fn renege(
        &mut self,
        contract_account: &mut Account,
        payee_account: &mut Account,
        lamports: u64,
    ) {
        let reneged_lamports = min(contract_account.lamports, lamports);
        payee_account.lamports += reneged_lamports;
        contract_account.lamports -= reneged_lamports;

        self.reneged_lamports += reneged_lamports;
    }

    /// Mark this contract as fully vested, regardless of the date.
    pub fn vest_all(&mut self) {
        self.is_fully_vested = true;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;
    use solana_sdk::system_program;

    #[test]
    fn test_serializer() {
        let mut a = Account::new(0, 512, &id());
        let b = VestState::default();
        b.serialize(&mut a.data).unwrap();
        let c = VestState::deserialize(&a.data).unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn test_serializer_data_too_small() {
        let mut a = Account::new(0, 1, &id());
        let b = VestState::default();
        assert_eq!(
            b.serialize(&mut a.data),
            Err(InstructionError::AccountDataTooSmall)
        );
    }

    #[test]
    fn test_schedule_after_renege() {
        let total_lamports = 3;
        let mut contract_account = Account::new(total_lamports, 512, &id());
        let mut payee_account = Account::new(0, 0, &system_program::id());
        let mut vest_state = VestState {
            total_lamports,
            start_date_time: Utc.ymd(2019, 1, 1).and_hms(0, 0, 0),
            ..VestState::default()
        };
        vest_state.serialize(&mut contract_account.data).unwrap();
        let current_date = Utc.ymd(2020, 1, 1);
        assert_eq!(vest_state.calc_vested_lamports(current_date), 1);

        // Verify vesting schedule is calculated with original amount.
        vest_state.renege(&mut contract_account, &mut payee_account, 1);
        assert_eq!(vest_state.calc_vested_lamports(current_date), 1);
        assert_eq!(vest_state.reneged_lamports, 1);

        // Verify reneged tokens aren't redeemable.
        assert_eq!(vest_state.calc_vested_lamports(Utc.ymd(2022, 1, 1)), 2);

        // Verify reneged tokens aren't redeemable after fully vesting.
        vest_state.vest_all();
        assert_eq!(vest_state.calc_vested_lamports(Utc.ymd(2022, 1, 1)), 2);
    }

    #[test]
    fn test_vest_all() {
        let total_lamports = 3;
        let mut contract_account = Account::new(total_lamports, 512, &id());
        let mut vest_state = VestState {
            total_lamports,
            start_date_time: Utc.ymd(2019, 1, 1).and_hms(0, 0, 0),
            ..VestState::default()
        };
        vest_state.serialize(&mut contract_account.data).unwrap();
        let current_date = Utc.ymd(2020, 1, 1);
        assert_eq!(vest_state.calc_vested_lamports(current_date), 1);

        vest_state.vest_all();
        assert_eq!(vest_state.calc_vested_lamports(current_date), 3);
    }
}
