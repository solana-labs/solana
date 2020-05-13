//! config for staking
//!  carries variables that the stake program cares about
use bincode::{deserialize, serialized_size};
use serde_derive::{Deserialize, Serialize};
use solana_config_program::{create_config_account, get_config_data, ConfigState};
use solana_sdk::{
    account::{Account, KeyedAccount},
    genesis_config::GenesisConfig,
    instruction::InstructionError,
};

// stake config ID
solana_sdk::declare_id!("StakeConfig11111111111111111111111111111111");

// means that no more than RATE of current effective stake may be added or subtracted per
//  epoch
pub const DEFAULT_WARMUP_COOLDOWN_RATE: f64 = 0.25;
pub const DEFAULT_SLASH_PENALTY: u8 = ((5 * std::u8::MAX as usize) / 100) as u8;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub struct Config {
    /// how much stake we can activate/deactivate per-epoch as a fraction of currently effective stake
    pub warmup_cooldown_rate: f64,
    /// percentage of stake lost when slash, expressed as a portion of std::u8::MAX
    pub slash_penalty: u8,
}

impl Config {
    pub fn from(account: &Account) -> Option<Self> {
        get_config_data(&account.data)
            .ok()
            .and_then(|data| deserialize(data).ok())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            warmup_cooldown_rate: DEFAULT_WARMUP_COOLDOWN_RATE,
            slash_penalty: DEFAULT_SLASH_PENALTY,
        }
    }
}

impl ConfigState for Config {
    fn max_space() -> u64 {
        serialized_size(&Config::default()).unwrap()
    }
}

pub fn add_genesis_account(genesis_config: &mut GenesisConfig) -> u64 {
    let mut account = create_config_account(vec![], &Config::default(), 0);
    let lamports = genesis_config.rent.minimum_balance(account.data.len());

    account.lamports = lamports.max(1);

    genesis_config.add_account(id(), account);

    lamports
}

pub fn create_account(lamports: u64, config: &Config) -> Account {
    create_config_account(vec![], config, lamports)
}

pub fn from_keyed_account(account: &KeyedAccount) -> Result<Config, InstructionError> {
    if !check_id(account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    Config::from(&*account.try_account_ref()?).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;
    use std::cell::RefCell;

    #[test]
    fn test() {
        let account = RefCell::new(create_account(0, &Config::default()));
        assert_eq!(Config::from(&account.borrow()), Some(Config::default()));
        assert_eq!(
            from_keyed_account(&KeyedAccount::new(&Pubkey::default(), false, &account)),
            Err(InstructionError::InvalidArgument)
        );
    }
}
