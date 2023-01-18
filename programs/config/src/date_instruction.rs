///
/// A library for creating a trusted date oracle.
///
use bincode::{deserialize, serialized_size};
use {
    crate::{config_instruction, ConfigState},
    chrono::{
        prelude::{DateTime, TimeZone, Utc},
        serde::ts_seconds,
    },
    serde_derive::{Deserialize, Serialize},
    solana_sdk::{instruction::Instruction, pubkey::Pubkey},
};

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct DateConfig {
    #[serde(with = "ts_seconds")]
    pub date_time: DateTime<Utc>,
}

impl Default for DateConfig {
    fn default() -> Self {
        Self {
            date_time: Utc.timestamp_opt(0, 0).unwrap(),
        }
    }
}
impl DateConfig {
    pub fn new(date_time: DateTime<Utc>) -> Self {
        Self { date_time }
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
pub fn store(date_pubkey: &Pubkey, date_time: DateTime<Utc>) -> Instruction {
    let date_config = DateConfig::new(date_time);
    config_instruction::store(date_pubkey, true, vec![], &date_config)
}
