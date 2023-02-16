use {
    crate::bank::Bank,
    solana_sdk::slot_history::Slot,
    std::{
        sync::{Arc, Condvar, Mutex, RwLock, Weak},
        time::{Duration, Instant},
    },
};

/// Tracks leader status of the validator node and notifies when:
///     1. A leader slot begins
///     2. A leader slot completes
#[derive(Debug, Default)]
pub struct LeaderBankStatus {
    /// Current state of the system
    status: Mutex<Status>,
    /// Weak reference to the current bank
    bank: RwLock<Option<(Slot, Weak<Bank>)>>,
    /// CondVar to notify status changes and waiting
    condvar: Condvar,
}

/// Leader status state machine for the validator:
/// [Unininitialized] -> [InProgress] -> [Completed] --|
///                          ^-------------------------|
#[derive(Debug, Default)]
enum Status {
    /// Initial state, no bank, but also not completed yet
    #[default]
    Uninitialized,
    /// Slot is in progress as leader
    InProgress,
    /// PoH has reached the end of the slot, and the next bank as leader is not available yet
    Completed,
}

impl LeaderBankStatus {
    /// Set the status to `InProgress` and notify any waiting threads
    /// if the status was not already `InProgress`.
    pub fn set_in_progress(&self, bank: &Arc<Bank>) {
        let mut status = self.status.lock().unwrap();
        if matches!(*status, Status::InProgress) {
            return;
        }

        *status = Status::InProgress;
        *self.bank.write().unwrap() = Some((bank.slot(), Arc::downgrade(bank)));
        self.condvar.notify_all();
    }

    /// Set the status to `Completed` and notify any waiting threads
    /// if the status was not already `Completed`
    /// and the slot is higher than the current slot (sanity check).
    pub fn set_completed(&self, slot: Slot) {
        let mut status = self.status.lock().unwrap();
        if matches!(*status, Status::Completed) {
            return;
        }

        if let Some((current_slot, _)) = *self.bank.read().unwrap() {
            if slot < current_slot {
                return;
            }
        }

        *status = Status::Completed;
        self.condvar.notify_all();
    }

    /// Return weak bank reference or wait for notification for an in progress bank.
    pub fn wait_for_in_progress(&self) -> Weak<Bank> {
        let status = self.status.lock().unwrap();

        // Hold status lock until after the weak bank reference is cloned.
        let status = self
            .condvar
            .wait_while(status, |status| {
                matches!(*status, Status::Uninitialized | Status::Completed)
            })
            .unwrap();
        let bank = self.bank.read().unwrap().as_ref().unwrap().1.clone();
        drop(status);

        bank
    }

    /// Wait for next notification for a completed slot.
    /// Returns None if the timeout is reached
    pub fn wait_for_next_completed_timeout(&self, mut timeout: Duration) -> Option<Slot> {
        loop {
            let start = Instant::now();
            let status = self.status.lock().unwrap();
            let (status, result) = self.condvar.wait_timeout(status, timeout).unwrap();
            if result.timed_out() {
                return None;
            }

            if matches!(*status, Status::Completed) {
                let slot = self.bank.read().unwrap().as_ref().unwrap().0;
                return Some(slot);
            }

            timeout = timeout.saturating_sub(start.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_bank_status_wait_for_in_progress() {
        let leader_bank_status = Arc::new(LeaderBankStatus::default());
        let leader_bank_status2 = leader_bank_status.clone();

        let jh = std::thread::spawn(move || {
            let _weak_bank = leader_bank_status2.wait_for_in_progress();
        });
        leader_bank_status.set_in_progress(&Arc::new(Bank::default_for_tests()));
        leader_bank_status.set_completed(1);

        jh.join().unwrap();
    }

    #[test]
    fn test_leader_bank_status_wait_for_completed() {
        let leader_bank_status = Arc::new(LeaderBankStatus::default());
        let leader_bank_status2 = leader_bank_status.clone();

        let jh = std::thread::spawn(move || {
            let _weak_bank =
                leader_bank_status2.wait_for_next_completed_timeout(Duration::from_secs(1));
        });
        leader_bank_status.set_in_progress(&Arc::new(Bank::default_for_tests()));

        jh.join().unwrap();
    }
}
