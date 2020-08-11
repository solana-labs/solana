//! calculate and collect rent from Accounts
use solana_sdk::{
    account::Account, clock::Epoch, epoch_schedule::EpochSchedule, genesis_config::GenesisConfig,
    incinerator, pubkey::Pubkey, rent::Rent, sysvar,
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
    // updates this account's lamports and status and returns
    //  the account rent collected, if any
    //
    #[must_use = "add to Bank::collected_rent"]
    pub fn collect_from_existing_account(&self, address: &Pubkey, account: &mut Account) -> u64 {
        if account.executable
            || account.rent_epoch > self.epoch
            || sysvar::check_id(&account.owner)
            || *address == incinerator::id()
        {
            0
        } else {
            let slots_elapsed: u64 = (account.rent_epoch..=self.epoch)
                .map(|epoch| self.epoch_schedule.get_slots_in_epoch(epoch + 1))
                .sum();

            // avoid infinite rent in rust 1.45
            let years_elapsed = if self.slots_per_year != 0.0 {
                slots_elapsed as f64 / self.slots_per_year
            } else {
                0.0
            };

            let (rent_due, exempt) =
                self.rent
                    .due(account.lamports, account.data.len(), years_elapsed);

            if exempt || rent_due != 0 {
                if account.lamports > rent_due {
                    account.rent_epoch = self.epoch + 1;
                    account.lamports -= rent_due;
                    rent_due
                } else {
                    let rent_charged = account.lamports;
                    *account = Account::default();
                    rent_charged
                }
            } else {
                // maybe collect rent later, leave account alone
                0
            }
        }
    }

    #[must_use = "add to Bank::collected_rent"]
    pub fn collect_from_created_account(&self, address: &Pubkey, account: &mut Account) -> u64 {
        // initialize rent_epoch as created at this epoch
        account.rent_epoch = self.epoch;
        self.collect_from_existing_account(address, account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collect_from_account_created_and_existing() {
        let old_lamports = 1000;
        let old_epoch = 1;
        let new_epoch = 3;

        let (mut created_account, mut existing_account) = {
            let mut account = Account::default();
            account.lamports = old_lamports;
            account.rent_epoch = old_epoch;

            (account.clone(), account)
        };

        let rent_collector = RentCollector::default().clone_with_epoch(new_epoch);

        let collected =
            rent_collector.collect_from_created_account(&Pubkey::new_rand(), &mut created_account);
        assert!(created_account.lamports < old_lamports);
        assert_eq!(created_account.lamports + collected, old_lamports);
        assert_ne!(created_account.rent_epoch, old_epoch);

        let collected = rent_collector
            .collect_from_existing_account(&Pubkey::new_rand(), &mut existing_account);
        assert!(existing_account.lamports < old_lamports);
        assert_eq!(existing_account.lamports + collected, old_lamports);
        assert_ne!(existing_account.rent_epoch, old_epoch);

        assert!(created_account.lamports > existing_account.lamports);
        assert_eq!(created_account.rent_epoch, existing_account.rent_epoch);
    }
}
