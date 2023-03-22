use {
    solana_runtime::bank::Bank,
    solana_sdk::slot_history::Slot,
    std::{
        sync::{Arc, Condvar, Mutex, Weak},
        time::{Duration, Instant},
    },
};

/// Tracks leader status of the validator node and notifies when:
///     1. A leader bank initiates (=PoH-initiated)
///     2. A leader slot completes (=PoH-completed)
#[derive(Debug, Default)]
pub struct LeaderBankNotifier {
    /// Current state (slot, bank, and status) of the system
    state: Mutex<SlotAndBankWithStatus>,
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

#[derive(Debug, Default)]
struct SlotAndBankWithStatus {
    status: Status,
    slot: Option<Slot>,
    bank: Weak<Bank>,
}

impl LeaderBankNotifier {
    /// Set the status to `InProgress` and notify any waiting threads.
    /// Panics if the status is not `StandBy` - cannot have multiple
    /// leader banks in progress.
    pub fn set_in_progress(&self, bank: &Arc<Bank>) {
        let mut state = self.state.lock().unwrap();
        assert_eq!(state.status, Status::StandBy);

        *state = SlotAndBankWithStatus {
            status: Status::InProgress,
            slot: Some(bank.slot()),
            bank: Arc::downgrade(bank),
        };
        drop(state);

        self.condvar.notify_all();
    }

    /// Set the status to `StandBy` and notify any waiting threads if the status was changed.
    /// Panics if the stored slot does not match the given slot.
    pub fn set_completed(&self, slot: Slot) {
        let mut state = self.state.lock().unwrap();
        assert_eq!(state.slot, Some(slot));

        // `set_completed` can be called multiple times for the same slot because it
        // may be called from multiple threads.
        if matches!(state.status, Status::StandBy) {
            return;
        }

        state.status = Status::StandBy;
        drop(state);

        self.condvar.notify_all();
    }

    /// If the status is `InProgress`, immediately return a weak reference to the bank.
    /// Otherwise, wait up to the `timeout` for the status to become `InProgress`.
    /// If the timeout is reached, the weak reference is unupgradable.
    pub fn get_or_wait_for_in_progress(&self, timeout: Duration) -> Weak<Bank> {
        let state = self.state.lock().unwrap();
        let (state, wait_timeout_result) = self
            .condvar
            .wait_timeout_while(state, timeout, |state| {
                matches!(state.status, Status::StandBy)
            })
            .unwrap();

        (!wait_timeout_result.timed_out())
            .then(|| state.bank.clone())
            .unwrap_or_else(Weak::new)
    }

    /// Wait for next notification for a completed leader slot.
    /// Returns `None` if the timeout is reached
    pub fn wait_for_completed(&self, mut remaining_timeout: Duration) -> Option<Slot> {
        loop {
            let start = Instant::now();
            let state = self.state.lock().unwrap();
            let (state, result) = self.condvar.wait_timeout(state, remaining_timeout).unwrap();
            if result.timed_out() {
                return None;
            } else if matches!(state.status, Status::StandBy) {
                return state.slot;
            }

            remaining_timeout = remaining_timeout.saturating_sub(start.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_bank_notifier_get_or_wait_for_in_progress() {
        let leader_bank_notifier = Arc::new(LeaderBankNotifier::default());
        let leader_bank_notifier2 = leader_bank_notifier.clone();

        let jh = std::thread::spawn(move || {
            let _weak_bank =
                leader_bank_notifier2.get_or_wait_for_in_progress(Duration::from_secs(1));
        });
        std::thread::sleep(Duration::from_millis(10));
        leader_bank_notifier.set_in_progress(&Arc::new(Bank::default_for_tests()));

        jh.join().unwrap();
    }

    #[test]
    fn test_leader_bank_notifier_get_or_wait_for_in_progress_timeout() {
        let leader_bank_notifier = Arc::new(LeaderBankNotifier::default());
        leader_bank_notifier.set_in_progress(&Arc::new(Bank::default_for_tests()));
        leader_bank_notifier.set_completed(1);

        assert!(leader_bank_notifier
            .get_or_wait_for_in_progress(Duration::from_millis(1))
            .upgrade()
            .is_none());
    }

    #[test]
    fn test_leader_bank_notifier_wait_for_completed() {
        let leader_bank_notifier = Arc::new(LeaderBankNotifier::default());
        let leader_bank_notifier2 = leader_bank_notifier.clone();

        let jh = std::thread::spawn(move || {
            let _slot = leader_bank_notifier2.wait_for_completed(Duration::from_secs(1));
        });
        leader_bank_notifier.set_in_progress(&Arc::new(Bank::default_for_tests()));
        std::thread::sleep(Duration::from_millis(10));
        leader_bank_notifier.set_completed(1);

        jh.join().unwrap();
    }

    #[test]
    fn test_leader_bank_notifier_wait_for_completed_timeout() {
        let leader_bank_notifier = Arc::new(LeaderBankNotifier::default());

        leader_bank_notifier.set_in_progress(&Arc::new(Bank::default_for_tests()));
        assert_eq!(
            leader_bank_notifier.wait_for_completed(Duration::from_millis(1)),
            None
        );
    }
}
