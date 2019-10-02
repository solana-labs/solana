//! vest state
use bincode::{self, deserialize, serialize_into};
use chrono::{
    prelude::{DateTime, TimeZone, Utc},
    serde::ts_seconds,
};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VestState {
    /// The address authorized to terminate this contract with a signed Terminate instruction
    pub terminator_pubkey: Pubkey,

    /// The address authorized to redeem vested tokens
    pub payee_pubkey: Pubkey,

    /// The day from which the vesting contract begins
    #[serde(with = "ts_seconds")]
    pub start_dt: DateTime<Utc>,

    /// Address of an account containing a trusted date, used to drive the vesting schedule
    pub date_pubkey: Pubkey,

    /// The number of lamports to send the payee if the schedule completes
    pub lamports: u64,

    /// The number of lamports the payee has already redeemed
    pub redeemed_lamports: u64,
}

impl Default for VestState {
    fn default() -> Self {
        Self {
            terminator_pubkey: Pubkey::default(),
            payee_pubkey: Pubkey::default(),
            start_dt: Utc.timestamp(0, 0),
            date_pubkey: Pubkey::default(),
            lamports: 0,
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
