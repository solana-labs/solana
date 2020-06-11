use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_stake_program::stake_state::StakeState;
use std::{collections::HashSet, sync::Arc};

pub struct NonCirculatingSupply {
    pub lamports: u64,
    pub accounts: Vec<Pubkey>,
}

pub fn calculate_non_circulating_supply(bank: &Arc<Bank>) -> NonCirculatingSupply {
    debug!("Updating Bank supply, epoch: {}", bank.epoch());
    let mut non_circulating_accounts_set: HashSet<Pubkey> = HashSet::new();

    for key in non_circulating_accounts() {
        non_circulating_accounts_set.insert(key);
    }
    let withdraw_authority_list = withdraw_authority();

    let clock = bank.clock();
    let stake_accounts = bank.get_program_accounts(Some(&solana_stake_program::id()));
    for (pubkey, account) in stake_accounts.iter() {
        let stake_account = StakeState::from(&account).unwrap_or_default();
        match stake_account {
            StakeState::Initialized(meta) => {
                if meta.lockup.is_in_force(&clock, &HashSet::default())
                    || withdraw_authority_list.contains(&meta.authorized.withdrawer)
                {
                    non_circulating_accounts_set.insert(*pubkey);
                }
            }
            StakeState::Stake(meta, _stake) => {
                if meta.lockup.is_in_force(&clock, &HashSet::default())
                    || withdraw_authority_list.contains(&meta.authorized.withdrawer)
                {
                    non_circulating_accounts_set.insert(*pubkey);
                }
            }
            _ => {}
        }
    }

    let lamports = non_circulating_accounts_set
        .iter()
        .map(|pubkey| bank.get_balance(&pubkey))
        .sum();

    NonCirculatingSupply {
        lamports,
        accounts: non_circulating_accounts_set.into_iter().collect(),
    }
}

// Mainnet-beta accounts that should be considered non-circulating
solana_sdk::pubkeys!(
    non_circulating_accounts,
    [
        "9huDUZfxoJ7wGMTffUE7vh1xePqef7gyrLJu9NApncqA",
        "GK2zqSsXLA2rwVZk347RYhh6jJpRsCA69FjLW93ZGi3B",
        "CWeRmXme7LmbaUWTZWFLt6FMnpzLCHaQLuR2TdgFn4Lq",
        "HCV5dGFJXRrJ3jhDYA4DCeb9TEDTwGGYXtT3wHksu2Zr",
        "14FUT96s9swbmH7ZjpDvfEDywnAYy9zaNhv4xvezySGu",
        "HbZ5FfmKWNHC7uwk6TF1hVi6TCs7dtYfdjEcuPGgzFAg",
        "C7C8odR8oashR5Feyrq2tJKaXL18id1dSj2zbkDGL2C2",
        "Eyr9P5XsjK2NUKNCnfu39eqpGoiLFgVAv1LSQgMZCwiQ",
        "DE1bawNcRJB9rVm3buyMVfr8mBEoyyu73NBovf2oXJsJ",
        "CakcnaRDHka2gXyfbEd2d3xsvkJkqsLw2akB3zsN1D2S",
        "7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2",
        "GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ",
        "Mc5XB47H3DKJHym5RLa9mPzWv5snERsF3KNv5AauXK8",
        "7cvkjYAkUYs4W8XcXsca7cBrEGFeSUjeZmKoNBvEwyri",
        "AG3m2bAibcY8raMt4oXEGqRHwX4FWKPPJVjZxn1LySDX",
        "5XdtyEDREHJXXW1CTtCsVjJRjBapAwK78ZquzvnNVRrV",
        "6yKHERk8rsbmJxvMpPuwPs1ct3hRiP7xaJF2tvnGU6nK",
        "CHmdL15akDcJgBkY6BP3hzs98Dqr6wbdDC5p8odvtSbq",
        "FR84wZQy3Y3j2gWz6pgETUiUoJtreMEuWfbg6573UCj9",
        "5q54XjQ7vDx4y6KphPeE97LUNiYGtP55spjvXAWPGBuf",
        "3o6xgkJ9sTmDeQWyfj3sxwon18fXJB9PV5LDc8sfgR4a",
    ]
);

// Withdraw authority for autostaked accounts on mainnet-beta
solana_sdk::pubkeys!(
    withdraw_authority,
    [
        "8CUUMKYNGxdgYio5CLHRHyzMEhhVRMcqefgE6dLqnVRK",
        "3FFaheyqtyAXZSYxDzsr5CVKvJuvZD1WE1VEsBtDbRqB",
    ]
);

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{
        account::Account, epoch_schedule::EpochSchedule, genesis_config::GenesisConfig,
    };
    use solana_stake_program::stake_state::{Authorized, Lockup, Meta, StakeState};
    use std::{collections::BTreeMap, sync::Arc};

    fn new_from_parent(parent: &Arc<Bank>) -> Bank {
        Bank::new_from_parent(parent, &Pubkey::default(), parent.slot() + 1)
    }

    #[test]
    fn test_calculate_non_circulating_supply() {
        let mut accounts: BTreeMap<Pubkey, Account> = BTreeMap::new();
        let balance = 10;
        let num_genesis_accounts = 10;
        for _ in 0..num_genesis_accounts {
            accounts.insert(
                Pubkey::new_rand(),
                Account::new(balance, 0, &Pubkey::default()),
            );
        }
        let non_circulating_accounts = non_circulating_accounts();
        let num_non_circulating_accounts = non_circulating_accounts.len() as u64;
        for key in non_circulating_accounts.clone() {
            accounts.insert(key, Account::new(balance, 0, &Pubkey::default()));
        }

        let num_stake_accounts = 3;
        for _ in 0..num_stake_accounts {
            let pubkey = Pubkey::new_rand();
            let meta = Meta {
                authorized: Authorized::auto(&pubkey),
                lockup: Lockup {
                    epoch: 1,
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
        let mut bank = Arc::new(Bank::new(&genesis_config));
        assert_eq!(
            bank.capitalization(),
            (num_genesis_accounts + num_non_circulating_accounts + num_stake_accounts) * balance
        );

        let non_circulating_supply = calculate_non_circulating_supply(&bank);
        assert_eq!(
            non_circulating_supply.lamports,
            (num_non_circulating_accounts + num_stake_accounts) * balance
        );
        assert_eq!(
            non_circulating_supply.accounts.len(),
            num_non_circulating_accounts as usize + num_stake_accounts as usize
        );

        bank = Arc::new(new_from_parent(&bank));
        let new_balance = 11;
        for key in non_circulating_accounts {
            bank.store_account(&key, &Account::new(new_balance, 0, &Pubkey::default()));
        }
        let non_circulating_supply = calculate_non_circulating_supply(&bank);
        assert_eq!(
            non_circulating_supply.lamports,
            (num_non_circulating_accounts * new_balance) + (num_stake_accounts * balance)
        );
        assert_eq!(
            non_circulating_supply.accounts.len(),
            num_non_circulating_accounts as usize + num_stake_accounts as usize
        );

        // Advance bank an epoch, which should unlock stakes
        for _ in 0..slots_per_epoch {
            bank = Arc::new(new_from_parent(&bank));
        }
        assert_eq!(bank.epoch(), 1);
        let non_circulating_supply = calculate_non_circulating_supply(&bank);
        assert_eq!(
            non_circulating_supply.lamports,
            num_non_circulating_accounts * new_balance
        );
        assert_eq!(
            non_circulating_supply.accounts.len(),
            num_non_circulating_accounts as usize
        );
    }
}
