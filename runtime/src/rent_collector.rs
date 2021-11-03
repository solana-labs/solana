//! calculate and collect rent from Accounts
use log::*;
use solana_sdk::{
    account::{AccountSharedData, ReadableAccount, WritableAccount},
    clock::Epoch,
    epoch_schedule::EpochSchedule,
    genesis_config::GenesisConfig,
    incinerator,
    pubkey::Pubkey,
    rent::Rent,
    sysvar,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, AbiExample)]
pub struct RentCollector {
    pub epoch: Epoch,
    pub epoch_schedule: EpochSchedule,
    pub slots_per_year: f64,
    pub rent: Rent,
}

impl Default for RentCollector {
    fn default() -> Self {
        Self {
            epoch: Epoch::default(),
            epoch_schedule: EpochSchedule::default(),
            // derive default value using GenesisConfig::default()
            slots_per_year: GenesisConfig::default().slots_per_year(),
            rent: Rent::default(),
        }
    }
}

impl RentCollector {
    pub fn new(
        epoch: Epoch,
        epoch_schedule: &EpochSchedule,
        slots_per_year: f64,
        rent: &Rent,
    ) -> Self {
        info!(
            "creating RentCollector, epoch: {}, slots_per_year: {}, rent: {:?}",
            epoch, slots_per_year, rent
        );

        Self {
            epoch,
            epoch_schedule: *epoch_schedule,
            slots_per_year,
            rent: *rent,
        }
    }

    pub fn clone_with_epoch(&self, epoch: Epoch) -> Self {
        Self {
            epoch,
            ..self.clone()
        }
    }

    /// true if it is easy to determine this account should not have rent collected from it
    pub fn no_rent(
        &self,
        address: &Pubkey,
        account: &impl ReadableAccount,
        rent_for_sysvars: bool,
    ) -> bool {
        account.executable() // executable accounts must be rent-exempt balance
            || account.rent_epoch() > self.epoch
            || (!rent_for_sysvars && sysvar::check_id(account.owner()))
            || *address == incinerator::id()
    }

    /// true if it is easy to determine this account should not have rent collected from it
    pub fn get_rent_due(&self, account: &impl ReadableAccount) -> (u64, bool) {
        let slots_elapsed: u64 = (account.rent_epoch()..=self.epoch)
            .map(|epoch| self.epoch_schedule.get_slots_in_epoch(epoch + 1))
            .sum();

        // avoid infinite rent in rust 1.45
        let years_elapsed = if self.slots_per_year != 0.0 {
            slots_elapsed as f64 / self.slots_per_year
        } else {
            0.0
        };

        self.rent
            .due(account.lamports(), account.data().len(), years_elapsed)
    }

    // updates this account's lamports and status and returns
    //  the account rent collected, if any
    // This is NOT thread safe at some level. If we try to collect from the same account in parallel, we may collect twice.
    #[must_use = "add to Bank::collected_rent"]
    pub fn collect_from_existing_account(
        &self,
        address: &Pubkey,
        account: &mut AccountSharedData,
        rent_for_sysvars: bool,
        filler_account_suffix: Option<&Pubkey>,
    ) -> u64 {
        if self.no_rent(address, account, rent_for_sysvars)
            || crate::accounts_db::AccountsDb::is_filler_account_helper(
                address,
                filler_account_suffix,
            )
        {
            0
        } else {
            let (rent_due, exempt) = self.get_rent_due(account);

            if exempt || rent_due != 0 {
                if account.lamports() > rent_due {
                    account.set_rent_epoch(
                        self.epoch
                            + if exempt {
                                // Rent isn't collected for the next epoch
                                // Make sure to check exempt status later in current epoch again
                                0
                            } else {
                                // Rent is collected for next epoch
                                1
                            },
                    );
                    let _ = account.checked_sub_lamports(rent_due); // will not fail. We check above.
                    rent_due
                } else {
                    let rent_charged = account.lamports();
                    *account = AccountSharedData::default();
                    rent_charged
                }
            } else {
                // maybe collect rent later, leave account alone
                0
            }
        }
    }

    #[must_use = "add to Bank::collected_rent"]
    pub fn collect_from_created_account(
        &self,
        address: &Pubkey,
        account: &mut AccountSharedData,
        rent_for_sysvars: bool,
    ) -> u64 {
        // initialize rent_epoch as created at this epoch
        account.set_rent_epoch(self.epoch);
        self.collect_from_existing_account(address, account, rent_for_sysvars, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::account::Account;

    #[test]
    fn test_collect_from_account_created_and_existing() {
        let old_lamports = 1000;
        let old_epoch = 1;
        let new_epoch = 3;

        let (mut created_account, mut existing_account) = {
            let account = AccountSharedData::from(Account {
                lamports: old_lamports,
                rent_epoch: old_epoch,
                ..Account::default()
            });

            (account.clone(), account)
        };

        let rent_collector = RentCollector::default().clone_with_epoch(new_epoch);

        // collect rent on a newly-created account
        let collected = rent_collector.collect_from_created_account(
            &solana_sdk::pubkey::new_rand(),
            &mut created_account,
            true,
        );
        assert!(created_account.lamports() < old_lamports);
        assert_eq!(created_account.lamports() + collected, old_lamports);
        assert_ne!(created_account.rent_epoch(), old_epoch);

        // collect rent on a already-existing account
        let collected = rent_collector.collect_from_existing_account(
            &solana_sdk::pubkey::new_rand(),
            &mut existing_account,
            true,
            None,
        );
        assert!(existing_account.lamports() < old_lamports);
        assert_eq!(existing_account.lamports() + collected, old_lamports);
        assert_ne!(existing_account.rent_epoch(), old_epoch);

        // newly created account should be collected for less rent; thus more remaining balance
        assert!(created_account.lamports() > existing_account.lamports());
        assert_eq!(created_account.rent_epoch(), existing_account.rent_epoch());
    }

    #[test]
    fn test_rent_exempt_temporal_escape() {
        let mut account = AccountSharedData::default();
        let epoch = 3;
        let huge_lamports = 123_456_789_012;
        let tiny_lamports = 789_012;
        let mut collected;
        let pubkey = solana_sdk::pubkey::new_rand();

        account.set_lamports(huge_lamports);
        assert_eq!(account.rent_epoch(), 0);

        // create a tested rent collector
        let rent_collector = RentCollector::default().clone_with_epoch(epoch);

        // first mark account as being collected while being rent-exempt
        collected = rent_collector.collect_from_existing_account(&pubkey, &mut account, true, None);
        assert_eq!(account.lamports(), huge_lamports);
        assert_eq!(collected, 0);

        // decrease the balance not to be rent-exempt
        account.set_lamports(tiny_lamports);

        // ... and trigger another rent collection on the same epoch and check that rent is working
        collected = rent_collector.collect_from_existing_account(&pubkey, &mut account, true, None);
        assert_eq!(account.lamports(), tiny_lamports - collected);
        assert_ne!(collected, 0);
    }

    #[test]
    fn test_rent_exempt_sysvar() {
        let tiny_lamports = 1;
        let mut account = AccountSharedData::default();
        account.set_owner(sysvar::id());
        account.set_lamports(tiny_lamports);

        let pubkey = solana_sdk::pubkey::new_rand();

        assert_eq!(account.rent_epoch(), 0);

        let epoch = 3;
        let rent_collector = RentCollector::default().clone_with_epoch(epoch);

        // old behavior: sysvars are special-cased
        let collected =
            rent_collector.collect_from_existing_account(&pubkey, &mut account, false, None);
        assert_eq!(account.lamports(), tiny_lamports);
        assert_eq!(collected, 0);

        // new behavior: sysvars are NOT special-cased
        let collected =
            rent_collector.collect_from_existing_account(&pubkey, &mut account, true, None);
        assert_eq!(account.lamports(), 0);
        assert_eq!(collected, 1);
    }
}
