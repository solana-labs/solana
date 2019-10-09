//! calculate and collect rent from Accounts
use solana_sdk::{
    account::Account, clock::Epoch, epoch_schedule::EpochSchedule, rent_calculator::RentCalculator,
};

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct RentCollector {
    pub epoch: Epoch,
    pub epoch_schedule: EpochSchedule,
    pub slots_per_year: f64,
    pub rent_calculator: RentCalculator,
}

impl RentCollector {
    pub fn new(
        epoch: Epoch,
        epoch_schedule: &EpochSchedule,
        slots_per_year: f64,
        rent_calculator: &RentCalculator,
    ) -> Self {
        Self {
            epoch,
            epoch_schedule: *epoch_schedule,
            slots_per_year,
            rent_calculator: *rent_calculator,
        }
    }

    fn able_to_cover_rent_till_epoch(&self, account: &Account) -> u64 {
        let (rent_due_per_slot, _) = self.rent_calculator.due(
            account.lamports,
            account.data.len(),
            1 as f64 / self.slots_per_year,
        );

        let able_to_pay_for_slots = account.lamports / rent_due_per_slot;
        let mut remaining_slots_to_consume = able_to_pay_for_slots;

        (account.rent_epoch + 1..=self.epoch)
            .take_while(|epoch| {
                let slots_in_current_epoch = self.epoch_schedule.get_slots_in_epoch(*epoch);
                if remaining_slots_to_consume == 0 {
                    false
                } else if remaining_slots_to_consume < slots_in_current_epoch {
                    remaining_slots_to_consume = 0;
                    true
                } else {
                    remaining_slots_to_consume -= slots_in_current_epoch;
                    true
                }
            })
            .last()
            .unwrap_or(account.rent_epoch)
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
    pub fn update<'a>(
        &self,
        account: &'a mut Account,
        deduct_partial_rent: bool,
    ) -> (Option<&'a Account>, u64) {
        if account.data.is_empty() || account.rent_epoch > self.epoch {
            (Some(account), 0)
        } else {
            let slots_elapsed: u64 = (account.rent_epoch..=self.epoch)
                .map(|epoch| self.epoch_schedule.get_slots_in_epoch(epoch + 1))
                .sum();

            let (rent_due, exempt) = self.rent_calculator.due(
                account.lamports,
                account.data.len(),
                slots_elapsed as f64 / self.slots_per_year,
            );

            if exempt || rent_due != 0 {
                if account.lamports > rent_due {
                    account.rent_epoch = self.epoch + 1;
                    account.lamports -= rent_due;
                    (Some(account), rent_due)
                } else if !deduct_partial_rent {
                    (None, 0)
                } else {
                    let epoch_to_forward = self.able_to_cover_rent_till_epoch(account);
                    let rent_due = account.lamports;
                    account.lamports -= rent_due;
                    account.rent_epoch = epoch_to_forward;
                    (Some(account), rent_due)
                }
            } else {
                // maybe collect rent later, leave account alone
                (Some(account), 0)
            }
        }
    }
}
