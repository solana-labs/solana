use crate::non_circulating_accounts::{
    NON_CIRCULATING_ACCOUNTS, WITHDRAW_AUTHORITY_FOR_AUTOSTAKED_ACCOUNTS,
};
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_stake_program::stake_state::StakeState;
use std::{collections::HashSet, str::FromStr};

pub fn update_supply(bank: &mut Bank) {
    debug!("Updating Bank supply, epoch: {}", bank.epoch());
    let mut non_circulating_accounts: HashSet<Pubkey> = HashSet::new();

    for key in NON_CIRCULATING_ACCOUNTS.iter() {
        non_circulating_accounts.insert(Pubkey::from_str(key).unwrap());
    }

    let clock = bank.clock();
    let stake_accounts = bank.get_program_accounts(Some(&solana_stake_program::id()));
    for (pubkey, account) in stake_accounts.iter() {
        let stake_account = StakeState::from(&account).unwrap_or_default();
        match stake_account {
            StakeState::Initialized(meta) => {
                if meta.lockup.is_in_force(&clock, &HashSet::default())
                    || meta.authorized.withdrawer
                        == Pubkey::from_str(WITHDRAW_AUTHORITY_FOR_AUTOSTAKED_ACCOUNTS).unwrap()
                {
                    non_circulating_accounts.insert(*pubkey);
                }
            }
            StakeState::Stake(meta, _stake) => {
                if meta.lockup.is_in_force(&clock, &HashSet::default())
                    || meta.authorized.withdrawer
                        == Pubkey::from_str(WITHDRAW_AUTHORITY_FOR_AUTOSTAKED_ACCOUNTS).unwrap()
                {
                    non_circulating_accounts.insert(*pubkey);
                }
            }
            _ => {}
        }
    }

    let non_circulating = non_circulating_accounts
        .iter()
        .fold(0, |acc, pubkey| acc + bank.get_balance(&pubkey));

    bank.update_supply(
        non_circulating,
        non_circulating_accounts.into_iter().collect(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::get_entered_epoch_callback;
    use solana_sdk::{
        account::Account, epoch_schedule::EpochSchedule, genesis_config::GenesisConfig,
    };
    use solana_stake_program::stake_state::{Authorized, Lockup, Meta, StakeState};
    use std::{collections::BTreeMap, sync::Arc};

    fn new_from_parent(parent: &Arc<Bank>) -> Bank {
        Bank::new_from_parent(parent, &Pubkey::default(), parent.slot() + 1)
    }

    #[test]
    fn test_update() {
        let mut accounts: BTreeMap<Pubkey, Account> = BTreeMap::new();
        let balance = 10;
        let num_genesis_accounts = 10;
        for _ in 0..num_genesis_accounts {
            accounts.insert(
                Pubkey::new_rand(),
                Account::new(balance, 0, &Pubkey::default()),
            );
        }
        let num_non_circulating_accounts = NON_CIRCULATING_ACCOUNTS.len() as u64;
        for key in NON_CIRCULATING_ACCOUNTS.iter() {
            accounts.insert(
                Pubkey::from_str(key).unwrap(),
                Account::new(balance, 0, &Pubkey::default()),
            );
        }

        let num_stake_accounts = 3;
        for _ in 0..num_stake_accounts {
            let pubkey = Pubkey::new_rand();
            let meta = Meta {
                authorized: Authorized::auto(&pubkey),
                lockup: Lockup {
                    epoch: 2,
                    ..Lockup::default()
                },
                ..Meta::default()
            };
            let stake_account = Account::new_data_with_space(
                balance,
                &StakeState::Initialized(meta),
                std::mem::size_of::<StakeState>(),
                &solana_stake_program::id(),
            )
            .unwrap();
            accounts.insert(pubkey, stake_account);
        }

        let slots_per_epoch = 32;
        let genesis_config = GenesisConfig {
            accounts,
            epoch_schedule: EpochSchedule::new(slots_per_epoch),
            ..GenesisConfig::default()
        };
        let mut bank = Bank::new(&genesis_config);
        bank.set_entered_epoch_callback(get_entered_epoch_callback(genesis_config.operating_mode));
        assert_eq!(
            bank.capitalization(),
            (num_genesis_accounts + num_non_circulating_accounts + num_stake_accounts) * balance
        );

        update_supply(&mut bank); // Manually update first bank
        assert_eq!(
            bank.supply.non_circulating,
            (num_non_circulating_accounts + num_stake_accounts) * balance
        );
        assert_eq!(
            bank.supply.non_circulating_accounts.len(),
            NON_CIRCULATING_ACCOUNTS.len() + num_stake_accounts as usize
        );

        let mut bank = Arc::new(bank);
        bank = Arc::new(new_from_parent(&bank));
        let new_balance = 11;
        for key in NON_CIRCULATING_ACCOUNTS.iter() {
            bank.store_account(
                &Pubkey::from_str(key).unwrap(),
                &Account::new(new_balance, 0, &Pubkey::default()),
            );
        }
        // Update should only operate once per epoch, so non_circulating_supply should not change
        // even though account balances have changed
        assert_eq!(
            bank.supply.non_circulating,
            (num_non_circulating_accounts + num_stake_accounts) * balance
        );
        assert_eq!(
            bank.supply.non_circulating_accounts.len(),
            NON_CIRCULATING_ACCOUNTS.len() + num_stake_accounts as usize
        );

        // Advance bank one epoch
        for _ in 0..slots_per_epoch {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.epoch(), 1);
        assert_eq!(
            bank.supply.non_circulating,
            (num_non_circulating_accounts * new_balance) + (num_stake_accounts * balance)
        );
        assert_eq!(
            bank.supply.non_circulating_accounts.len(),
            NON_CIRCULATING_ACCOUNTS.len() + num_stake_accounts as usize
        );

        // Advance bank another epoch, which should unlock stakes
        for _ in 0..slots_per_epoch {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.epoch(), 2);
        assert_eq!(
            bank.supply.non_circulating,
            num_non_circulating_accounts * new_balance
        );
        assert_eq!(
            bank.supply.non_circulating_accounts.len(),
            NON_CIRCULATING_ACCOUNTS.len()
        );
    }
}
