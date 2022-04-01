//! config for staking
//!  carries variables that the stake program cares about
#[deprecated(
    since = "1.8.0",
    note = "Please use `solana_sdk::stake::config` or `solana_program::stake::config` instead"
)]
pub use solana_sdk::stake::config::*;
use {
    bincode::deserialize,
    solana_config_program::{create_config_account, get_config_data},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        genesis_config::GenesisConfig,
        stake::config::{self, Config},
    },
};

pub fn from<T: ReadableAccount>(account: &T) -> Option<Config> {
    get_config_data(account.data())
        .ok()
        .and_then(|data| deserialize(data).ok())
}

pub fn create_account(lamports: u64, config: &Config) -> AccountSharedData {
    create_config_account(vec![], config, lamports)
}

pub fn add_genesis_account(genesis_config: &mut GenesisConfig) -> u64 {
    let mut account = create_config_account(vec![], &Config::default(), 0);
    let lamports = genesis_config.rent.minimum_balance(account.data().len());

    account.set_lamports(lamports.max(1));

    genesis_config.add_account(config::id(), account);

    lamports
}

#[cfg(test)]
mod tests {
    use {super::*, std::cell::RefCell};

    #[test]
    fn test() {
        let account = RefCell::new(create_account(0, &Config::default()));
        assert_eq!(from(&account.borrow()), Some(Config::default()));
    }
}
