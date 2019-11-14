//! calculate and collect rent from Accounts
use solana_sdk::{account::Account, clock::Epoch, epoch_schedule::EpochSchedule, rent::Rent};

#[derive(Default, Serialize, Deserialize, Clone)]
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
    pub fn update(&self, account: &mut Account) -> u64 {
        if account.rent_epoch > self.epoch {
            0
        } else {
            let slots_elapsed: u64 = (account.rent_epoch..=self.epoch)
                .map(|epoch| self.epoch_schedule.get_slots_in_epoch(epoch + 1))
                .sum();

            let (rent_due, exempt) = self.rent.due(
                account.lamports,
                account.data.len(),
                slots_elapsed as f64 / self.slots_per_year,
            );

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
}
