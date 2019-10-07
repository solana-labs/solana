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

    /// Redeem vested tokens.
    pub fn redeem_tokens(
        &mut self,
        contract_account: &mut Account,
        current_date: Date<Utc>,
        payee_account: &mut Account,
    ) {
        let schedule = create_vesting_schedule(self.start_date_time.date(), self.total_lamports);

        let vested_lamports = schedule
            .into_iter()
            .take_while(|(dt, _)| *dt <= current_date)
            .map(|(_, lamports)| lamports)
            .sum::<u64>();

        let redeemable_lamports = vested_lamports.saturating_sub(self.redeemed_lamports);

        contract_account.lamports -= redeemable_lamports;
        payee_account.lamports += redeemable_lamports;

        self.redeemed_lamports += redeemable_lamports;
    }

    /// Terminate the contract and return all tokens to the given pubkey.
    pub fn terminate(&mut self, contract_account: &mut Account, payee_account: &mut Account) {
        payee_account.lamports += contract_account.lamports;
        contract_account.lamports = 0;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::id;
    use solana_sdk::account::Account;

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
}
