use log::*;
use solana_runtime::bank::Bank;
use solana_sdk::{
    clock::{Clock, Epoch},
    pubkey::Pubkey,
    sysvar::{self, Sysvar},
};
use solana_stake_program::stake_state::StakeState;
use std::{collections::HashSet, str::FromStr, sync::Arc};

#[derive(Default, PartialEq)]
pub struct NonCirculatingSupply {
    pub epoch: Epoch,
    pub non_circulating_supply: u64,
    pub non_circulating_accounts: Vec<Pubkey>,
}

impl NonCirculatingSupply {
    pub fn update(&mut self, bank: Arc<Bank>) {
        let epoch = bank.epoch();
        // Add duplicate check to ensure only one thread may update per epoch
        if self == &NonCirculatingSupply::default() || epoch > self.epoch {
            debug!("Updating non_circulating_supply, epoch: {}", epoch);
            let mut non_circulating_accounts: HashSet<Pubkey> = HashSet::new();

            for key in NON_CIRCULATING_ACCOUNTS.iter() {
                non_circulating_accounts.insert(Pubkey::from_str(key).unwrap());
            }

            let clock =
                Clock::from_account(&bank.get_account(&sysvar::clock::id()).unwrap()).unwrap();
            let stake_accounts = bank.get_program_accounts(Some(&solana_stake_program::id()));
            for (pubkey, account) in stake_accounts.iter() {
                let stake_account = StakeState::from(&account).unwrap_or_default();
                match stake_account {
                    StakeState::Initialized(meta) => {
                        if meta.lockup.is_in_force(&clock, &HashSet::default())
                            || meta.authorized.withdrawer
                                == Pubkey::from_str(WITHDRAW_AUTHORITY_FOR_AUTOSTAKED_ACCOUNTS)
                                    .unwrap()
                        {
                            non_circulating_accounts.insert(*pubkey);
                        }
                    }
                    StakeState::Stake(meta, _stake) => {
                        if meta.lockup.is_in_force(&clock, &HashSet::default())
                            || meta.authorized.withdrawer
                                == Pubkey::from_str(WITHDRAW_AUTHORITY_FOR_AUTOSTAKED_ACCOUNTS)
                                    .unwrap()
                        {
                            non_circulating_accounts.insert(*pubkey);
                        }
                    }
                    _ => {}
                }
            }

            let non_circulating_supply = non_circulating_accounts
                .iter()
                .fold(0, |acc, pubkey| acc + bank.get_balance(&pubkey));

            self.epoch = bank.epoch();
            self.non_circulating_supply = non_circulating_supply;
            self.non_circulating_accounts = non_circulating_accounts.into_iter().collect();
        }
    }
}

const NON_CIRCULATING_ACCOUNTS: [&str; 12] = [
    "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
    "GK2zqSsXLA2rwVZk347RYhh6jJpRsCA69FjLW93ZGi3B",
    "HCV5dGFJXRrJ3jhDYA4DCeb9TEDTwGGYXtT3wHksu2Zr",
    "25odAafVXnd63L6Hq5Cx6xGmhKqkhE2y6UrLVuqUfWZj",
    "14FUT96s9swbmH7ZjpDvfEDywnAYy9zaNhv4xvezySGu",
    "HbZ5FfmKWNHC7uwk6TF1hVi6TCs7dtYfdjEcuPGgzFAg",
    "C7C8odR8oashR5Feyrq2tJKaXL18id1dSj2zbkDGL2C2",
    "APnSR52EC1eH676m7qTBHUJ1nrGpHYpV7XKPxgRDD8gX",
    "9ibqedFVnu5k4wo1mJRbH6KJ5HLBCyjpA9omPYkDeeT5",
    "FopBKzQkG9pkyQqjdMFBLMQ995pSkjy83ziR4aism4c6",
    "AiUHvJhTbMCcgFE2K26Ea9qCe74y3sFwqUt38iD5sfoR",
    "3DndE3W53QdHSfBJiSJgzDKGvKJBoQLVmRHvy5LtqYfG",
];

const WITHDRAW_AUTHORITY_FOR_AUTOSTAKED_ACCOUNTS: &str =
    "8CUUMKYNGxdgYio5CLHRHyzMEhhVRMcqefgE6dLqnVRK";

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        account::Account, epoch_schedule::EpochSchedule, genesis_config::GenesisConfig,
    };
    use solana_stake_program::stake_state::{Authorized, Lockup, Meta, StakeState};
    use std::collections::BTreeMap;

    fn new_from_parent(parent: &Arc<Bank>) -> Bank {
        Bank::new_from_parent(parent, &Pubkey::default(), parent.slot() + 1)
    }

    #[test]
    fn test_update() {
        let mut accounts: BTreeMap<Pubkey, Account> = BTreeMap::new();
        let balance = 10;
        for _ in 0..10 {
            accounts.insert(
                Pubkey::new_rand(),
                Account::new(balance, 0, &Pubkey::default()),
            );
        }
        for key in NON_CIRCULATING_ACCOUNTS.iter() {
            accounts.insert(
                Pubkey::from_str(key).unwrap(),
                Account::new(balance, 0, &Pubkey::default()),
            );
        }

        for _ in 0..3 {
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
        let mut bank = Arc::new(Bank::new(&GenesisConfig {
            accounts,
            epoch_schedule: EpochSchedule::new(slots_per_epoch),
            ..GenesisConfig::default()
        }));
        assert_eq!(bank.capitalization(), (10 + 14 + 3) * balance);

        let mut non_circulating_supply = NonCirculatingSupply::default();
        assert_eq!(
            non_circulating_supply.update(bank.clone()),
            (14 + 3) * balance
        );
        assert_eq!(non_circulating_supply.epoch, 0);
        assert_eq!(
            non_circulating_supply.non_circulating_supply,
            (14 + 3) * balance
        );

        bank = Arc::new(new_from_parent(&bank));
        let new_balance = 11;
        for key in NON_CIRCULATING_ACCOUNTS.iter() {
            bank.store_account(
                &Pubkey::from_str(key).unwrap(),
                &Account::new(new_balance, 0, &Pubkey::default()),
            );
        }
        // Update should only operate once per epoch, so non_circulating_supply should not change
        assert_eq!(
            non_circulating_supply.update(bank.clone()),
            (14 + 3) * balance
        );

        // Advance bank one epoch
        for _ in 0..slots_per_epoch {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(
            non_circulating_supply.update(bank.clone()),
            (14 * new_balance) + (3 * balance)
        );
        assert_eq!(non_circulating_supply.epoch, 1);
        assert_eq!(
            non_circulating_supply.non_circulating_supply,
            (14 * new_balance) + (3 * balance)
        );

        // Advance bank another epoch, which should unlock stakes
        for _ in 0..slots_per_epoch {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(
            non_circulating_supply.update(bank.clone()),
            14 * new_balance
        );
        assert_eq!(non_circulating_supply.epoch, 2);
        assert_eq!(
            non_circulating_supply.non_circulating_supply,
            14 * new_balance
        );
    }
}
