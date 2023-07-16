use solana_config_program::ConfigState;

pub const MAX_SHORT_FIELD_LENGTH: usize = 80;
pub const MAX_LONG_FIELD_LENGTH: usize = 300;
pub const MAX_VALIDATOR_INFO: u64 = 576;

solana_sdk::declare_id!("Va1idator1nfo111111111111111111111111111111");

#[derive(Debug, Deserialize, PartialEq, Eq, Serialize, Default)]
pub struct ValidatorInfo {
    pub info: String,
}

impl ConfigState for ValidatorInfo {
    fn max_space() -> u64 {
        MAX_VALIDATOR_INFO
    }
}
