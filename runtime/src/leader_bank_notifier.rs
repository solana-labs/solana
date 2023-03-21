use {
    crate::bank::Bank,
    solana_sdk::slot_history::Slot,
    std::{
        sync::{Arc, Condvar, Mutex, RwLock, Weak},
        time::{Duration, Instant},
    },
};

/// Tracks leader status of the validator node and notifies when:
///     1. A leader bank initiates (=PoH-initiated)
///     2. A leader slot completes (=PoH-completed)
#[derive(Debug, Default)]
pub struct LeaderBankNotifier {
    /// Current state of the system
    status: Mutex<Status>,
    /// Weak reference to the current bank
    bank: RwLock<Option<(Slot, Weak<Bank>)>>,
    /// CondVar to notify status changes and waiting
    condvar: Condvar,
}

/// Leader status state machine for the validator.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
enum Status {
    /// The leader bank is not currently available. Either not initialized, or PoH-completed bank.
    #[default]
    StandBy,
    /// PoH-initiated bank is available.
    InProgress,
}

impl LeaderBankNotifier {
    /// Set the status to `InProgress` and notify any waiting threads
    /// if the status was not already `InProgress`.
    pub fn set_in_progress(&self, bank: &Arc<Bank>) {
        let mut status = self.status.lock().unwrap();
        if matches!(*status, Status::InProgress) {
            return;
        }

        *status = Status::InProgress;
        *self.bank.write().unwrap() = Some((bank.slot(), Arc::downgrade(bank)));
        drop(status);

        self.condvar.notify_all();
    }

    /// Set the status to `StandBy` and notify any waiting threads if
    ///     1. the status was not already `StandBy` and
    ///     2. the slot is higher than the current slot (sanity check).
    pub fn set_completed(&self, slot: Slot) {
        let mut status = self.status.lock().unwrap();
        if matches!(*status, Status::StandBy) {
            return;
        }

        if let Some((current_slot, _)) = *self.bank.read().unwrap() {
            if slot < current_slot {
                return;
            }
        }

        *status = Status::StandBy;
        drop(status);

        self.condvar.notify_all();
    }

    /// If the status is `InProgress`, immediately return a weak reference to the bank.
    /// Otherwise, wait up to the `timeout` for the status to become `InProgress`.
    /// Returns `None` if the timeout is reached.
    pub fn wait_for_in_progress(&self, timeout: Duration) -> Option<Weak<Bank>> {
        let status = self.status.lock().unwrap();

        // Hold status lock until after the weak bank reference is cloned.
        let (_status, wait_timeout_result) = self
            .condvar
            .wait_timeout_while(status, timeout, |status| matches!(*status, Status::StandBy))
            .unwrap();

        (!wait_timeout_result.timed_out())
            .then(|| self.bank.read().unwrap().as_ref().unwrap().1.clone())
    }

    /// Wait for next notification for a completed leader slot.
    /// Returns `None` if the timeout is reached
    pub fn wait_for_next_completed(&self, mut remaining_timeout: Duration) -> Option<Slot> {
        loop {
            let start = Instant::now();
            let status = self.status.lock().unwrap();
            let (status, result) = self
                .condvar
                .wait_timeout(status, remaining_timeout)
                .unwrap();
            if result.timed_out() {
                return None;
            }

            if matches!(*status, Status::StandBy) {
                let slot = self.bank.read().unwrap().as_ref().unwrap().0;
                return Some(slot);
            }

            remaining_timeout = remaining_timeout.saturating_sub(start.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_bank_status_wait_for_in_progress() {
        let leader_bank_status = Arc::new(LeaderBankNotifier::default());
        let leader_bank_status2 = leader_bank_status.clone();

        let jh = std::thread::spawn(move || {
            let _weak_bank = leader_bank_status2.wait_for_in_progress(Duration::from_secs(1));
        });
        std::thread::sleep(Duration::from_millis(10));
        leader_bank_status.set_in_progress(&Arc::new(Bank::default_for_tests()));

        jh.join().unwrap();
    }

    #[test]
    fn test_leader_bank_status_wait_for_in_progress_timeout() {
        let leader_bank_status = Arc::new(LeaderBankNotifier::default());
        leader_bank_status.set_in_progress(&Arc::new(Bank::default_for_tests()));
        leader_bank_status.set_completed(1);

        assert!(leader_bank_status
            .wait_for_in_progress(Duration::from_millis(1))
            .is_none());
    }

    #[test]
    fn test_leader_bank_status_wait_for_next_completed() {
        let leader_bank_status = Arc::new(LeaderBankNotifier::default());
        let leader_bank_status2 = leader_bank_status.clone();

        let jh = std::thread::spawn(move || {
            let _slot = leader_bank_status2.wait_for_next_completed(Duration::from_secs(1));
        });
        leader_bank_status.set_in_progress(&Arc::new(Bank::default_for_tests()));
        std::thread::sleep(Duration::from_millis(10));
        leader_bank_status.set_completed(1);

        jh.join().unwrap();
    }

    #[test]
    fn test_leader_bank_status_wait_for_next_completed_timeout() {
        let leader_bank_status = Arc::new(LeaderBankNotifier::default());

        leader_bank_status.set_in_progress(&Arc::new(Bank::default_for_tests()));
        assert_eq!(
            leader_bank_status.wait_for_next_completed(Duration::from_millis(1)),
            None
        );
    }
}
