//! calculate and collect rent from Accounts
use solana_sdk::{
    account::Account, clock::Epoch, epoch_schedule::EpochSchedule, incinerator, pubkey::Pubkey,
    rent::Rent, sysvar,
};

#[derive(Default, Serialize, Deserialize, Clone, PartialEq, Debug, AbiExample)]
pub struct RentCollector {
    pub epoch: Epoch,
    pub epoch_schedule: EpochSchedule,
    pub slots_per_year: f64,
    pub rent: Rent,
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

    pub fn collect_from_created_account(&self, address: &Pubkey, account: &mut Account) -> u64 {
        // initialize rent_epoch as created at this epoch
        account.rent_epoch = self.epoch;
        self.collect_from_existing_account(address, account)
    }

    pub fn collect_from_account(&self, address: &Pubkey, account: &mut Option<Account>) -> u64 {
        if let Some(account) = account {
            self.collect_from_existing_account(address, account)
        } else {
            *account = Some(Account::default());
            self.collect_from_created_account(address, &mut account.as_mut().unwrap())
        }
    }
}
