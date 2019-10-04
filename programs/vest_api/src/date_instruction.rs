///
/// A library for creating a trusted date oracle.
///
use bincode::{deserialize, serialized_size};
use chrono::{
    prelude::{Date, DateTime, TimeZone, Utc},
    serde::ts_seconds,
};
use serde_derive::{Deserialize, Serialize};
use solana_config_api::{config_instruction, ConfigState};
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct DateConfig {
    #[serde(with = "ts_seconds")]
    pub dt: DateTime<Utc>,
}

impl Default for DateConfig {
    fn default() -> Self {
        Self {
            dt: Utc.timestamp(0, 0),
        }
    }
}
impl DateConfig {
    pub fn new(dt: Date<Utc>) -> Self {
        Self {
            dt: dt.and_hms(0, 0, 0),
        }
    }

    pub fn deserialize(input: &[u8]) -> Option<Self> {
        deserialize(input).ok()
    }
}

impl ConfigState for DateConfig {
    fn max_space() -> u64 {
        serialized_size(&Self::default()).unwrap()
    }
}

/// Create a date account. The date is set to the Unix epoch.
pub fn create_account(
    payer_pubkey: &Pubkey,
    date_pubkey: &Pubkey,
    lamports: u64,
) -> Vec<Instruction> {
    config_instruction::create_account::<DateConfig>(payer_pubkey, date_pubkey, lamports, vec![])
}

/// Set the date in the date account. The account pubkey must be signed in the
/// transaction containing this instruction.
pub fn store(date_pubkey: &Pubkey, dt: Date<Utc>) -> Instruction {
    let date_config = DateConfig::new(dt);
    config_instruction::store(&date_pubkey, true, vec![], &date_config)
}
