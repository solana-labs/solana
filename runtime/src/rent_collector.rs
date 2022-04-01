//! calculate and collect rent from Accounts
use {
    log::*,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount, WritableAccount},
        clock::Epoch,
        epoch_schedule::EpochSchedule,
        genesis_config::GenesisConfig,
        incinerator,
        pubkey::Pubkey,
        rent::{Rent, RentDue},
    },
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

/// when rent is collected for this account, this is the action to apply to the account
#[derive(Debug)]
pub enum RentResult {
    /// maybe collect rent later, leave account alone
    LeaveAloneNoRent,
    /// collect rent
    /// value is (new rent epoch, lamports of rent_due)
    CollectRent((Epoch, u64)),
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
        self.clone_with_epoch_and_rate(epoch, self.rent.lamports_per_byte_year)
    }

    pub fn clone_with_epoch_and_rate(&self, epoch: Epoch, lamports_per_byte_year: u64) -> Self {
        let rent = if lamports_per_byte_year != self.rent.lamports_per_byte_year {
            Rent {
                lamports_per_byte_year,
                ..self.rent
            }
        } else {
            self.rent
        };
        Self {
            rent,
            epoch,
            ..self.clone()
        }
    }

    /// true if it is easy to determine this account should consider having rent collected from it
    pub fn should_collect_rent(&self, address: &Pubkey, account: &impl ReadableAccount) -> bool {
        !(account.executable() // executable accounts must be rent-exempt balance
            || *address == incinerator::id())
    }

    /// given an account that 'should_collect_rent'
    /// returns (amount rent due, is_exempt_from_rent)
    pub fn get_rent_due(&self, account: &impl ReadableAccount) -> RentDue {
        if self
            .rent
            .is_exempt(account.lamports(), account.data().len())
        {
            RentDue::Exempt
        } else {
            let account_rent_epoch = account.rent_epoch();
            let slots_elapsed: u64 = (account_rent_epoch..=self.epoch)
                .map(|epoch| self.epoch_schedule.get_slots_in_epoch(epoch + 1))
                .sum();

            // avoid infinite rent in rust 1.45
            let years_elapsed = if self.slots_per_year != 0.0 {
                slots_elapsed as f64 / self.slots_per_year
            } else {
                0.0
            };

            // we know this account is not exempt
            let due = self.rent.due_amount(account.data().len(), years_elapsed);

            // we expect rent_epoch to always be one of: {0, self.epoch-1, self.epoch, self.epoch+1}
            if account_rent_epoch != 0
                && (account_rent_epoch + 1 < self.epoch || account_rent_epoch > self.epoch + 1)
            {
                // this should not occur in a running validator
                if due == 0 {
                    inc_new_counter_info!("rent-collector-rent-epoch-range-large-exempt", 1);
                } else {
                    inc_new_counter_info!("rent-collector-rent-epoch-range-large-paying", 1);
                }
            }

            RentDue::Paying(due)
        }
    }

    // Updates the account's lamports and status, and returns the amount of rent collected, if any.
    // This is NOT thread safe at some level. If we try to collect from the same account in
    // parallel, we may collect twice.
    #[must_use = "add to Bank::collected_rent"]
    pub fn collect_from_existing_account(
        &self,
        address: &Pubkey,
        account: &mut AccountSharedData,
        filler_account_suffix: Option<&Pubkey>,
    ) -> CollectedInfo {
        match self.calculate_rent_result(address, account, filler_account_suffix) {
            RentResult::LeaveAloneNoRent => CollectedInfo::default(),
            RentResult::CollectRent((next_epoch, rent_due)) => {
                account.set_rent_epoch(next_epoch);

                let begin_lamports = account.lamports();
                account.saturating_sub_lamports(rent_due);
                let end_lamports = account.lamports();
                let mut account_data_len_reclaimed = 0;
                if end_lamports == 0 {
                    account_data_len_reclaimed = account.data().len() as u64;
                    *account = AccountSharedData::default();
                }
                CollectedInfo {
                    rent_amount: begin_lamports - end_lamports,
                    account_data_len_reclaimed,
                }
            }
        }
    }

    /// determine what should happen to collect rent from this account
    #[must_use]
    pub fn calculate_rent_result(
        &self,
        address: &Pubkey,
        account: &impl ReadableAccount,
        filler_account_suffix: Option<&Pubkey>,
    ) -> RentResult {
        if self.can_skip_rent_collection(address, account, filler_account_suffix) {
            return RentResult::LeaveAloneNoRent;
        }

        let rent_due = self.get_rent_due(account);
        if let RentDue::Paying(0) = rent_due {
            // maybe collect rent later, leave account alone
            return RentResult::LeaveAloneNoRent;
        }

        let epoch_increment = match rent_due {
            // Rent isn't collected for the next epoch
            // Make sure to check exempt status again later in current epoch
            RentDue::Exempt => 0,
            // Rent is collected for next epoch
            RentDue::Paying(_) => 1,
        };
        RentResult::CollectRent((self.epoch + epoch_increment, rent_due.lamports()))
    }

    #[must_use = "add to Bank::collected_rent"]
    pub fn collect_from_created_account(
        &self,
        address: &Pubkey,
        account: &mut AccountSharedData,
    ) -> CollectedInfo {
        // initialize rent_epoch as created at this epoch
        account.set_rent_epoch(self.epoch);
        self.collect_from_existing_account(address, account, None)
    }

    /// Performs easy checks to see if rent collection can be skipped
    fn can_skip_rent_collection(
        &self,
        address: &Pubkey,
        account: &impl ReadableAccount,
        filler_account_suffix: Option<&Pubkey>,
    ) -> bool {
        !self.should_collect_rent(address, account)
            || account.rent_epoch() > self.epoch
            || crate::accounts_db::AccountsDb::is_filler_account_helper(
                address,
                filler_account_suffix,
            )
    }
}

/// Information computed during rent collection
#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct CollectedInfo {
    /// Amount of rent collected from account
    pub rent_amount: u64,
    /// Size of data reclaimed from account (happens when account's lamports go to zero)
    pub account_data_len_reclaimed: u64,
}

impl std::ops::Add for CollectedInfo {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            rent_amount: self.rent_amount + other.rent_amount,
            account_data_len_reclaimed: self.account_data_len_reclaimed
                + other.account_data_len_reclaimed,
        }
    }
}

impl std::ops::AddAssign for CollectedInfo {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_sdk::{account::Account, sysvar},
    };

    fn default_rent_collector_clone_with_epoch(epoch: Epoch) -> RentCollector {
        RentCollector::default().clone_with_epoch(epoch)
    }

    #[test]
    fn test_collect_from_account_created_and_existing() {
        let old_lamports = 1000;
        let old_epoch = 1;
        let new_epoch = 2;

        let (mut created_account, mut existing_account) = {
            let account = AccountSharedData::from(Account {
                lamports: old_lamports,
                rent_epoch: old_epoch,
                ..Account::default()
            });

            (account.clone(), account)
        };

        let rent_collector = default_rent_collector_clone_with_epoch(new_epoch);

        // collect rent on a newly-created account
        let collected = rent_collector
            .collect_from_created_account(&solana_sdk::pubkey::new_rand(), &mut created_account);
        assert!(created_account.lamports() < old_lamports);
        assert_eq!(
            created_account.lamports() + collected.rent_amount,
            old_lamports
        );
        assert_ne!(created_account.rent_epoch(), old_epoch);
        assert_eq!(collected.account_data_len_reclaimed, 0);

        // collect rent on a already-existing account
        let collected = rent_collector.collect_from_existing_account(
            &solana_sdk::pubkey::new_rand(),
            &mut existing_account,
            None,
        );
        assert!(existing_account.lamports() < old_lamports);
        assert_eq!(
            existing_account.lamports() + collected.rent_amount,
            old_lamports
        );
        assert_ne!(existing_account.rent_epoch(), old_epoch);
        assert_eq!(collected.account_data_len_reclaimed, 0);

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
        let pubkey = solana_sdk::pubkey::new_rand();

        account.set_lamports(huge_lamports);
        assert_eq!(account.rent_epoch(), 0);

        // create a tested rent collector
        let rent_collector = default_rent_collector_clone_with_epoch(epoch);

        // first mark account as being collected while being rent-exempt
        let collected = rent_collector.collect_from_existing_account(&pubkey, &mut account, None);
        assert_eq!(account.lamports(), huge_lamports);
        assert_eq!(collected, CollectedInfo::default());

        // decrease the balance not to be rent-exempt
        account.set_lamports(tiny_lamports);

        // ... and trigger another rent collection on the same epoch and check that rent is working
        let collected = rent_collector.collect_from_existing_account(&pubkey, &mut account, None);
        assert_eq!(account.lamports(), tiny_lamports - collected.rent_amount);
        assert_ne!(collected, CollectedInfo::default());
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
        let rent_collector = default_rent_collector_clone_with_epoch(epoch);

        let collected = rent_collector.collect_from_existing_account(&pubkey, &mut account, None);
        assert_eq!(account.lamports(), 0);
        assert_eq!(collected.rent_amount, 1);
    }

    /// Ensure that when an account is "rent collected" away, its data len is returned.
    #[test]
    fn test_collect_cleans_up_account() {
        solana_logger::setup();
        let account_lamports = 1; // must be *below* rent amount
        let account_data_len = 567;
        let account_rent_epoch = 11;
        let mut account = AccountSharedData::from(Account {
            lamports: account_lamports, // <-- must be below rent-exempt amount
            data: vec![u8::default(); account_data_len],
            rent_epoch: account_rent_epoch,
            ..Account::default()
        });
        let rent_collector = default_rent_collector_clone_with_epoch(account_rent_epoch + 1);

        let collected =
            rent_collector.collect_from_existing_account(&Pubkey::new_unique(), &mut account, None);

        assert_eq!(collected.rent_amount, account_lamports);
        assert_eq!(
            collected.account_data_len_reclaimed,
            account_data_len as u64
        );
        assert_eq!(account, AccountSharedData::default());
    }
}
