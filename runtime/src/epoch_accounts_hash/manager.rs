use {
    super::EpochAccountsHash,
    solana_sdk::{clock::Slot, hash::Hash},
    std::sync::{Condvar, Mutex},
};

/// Manage the epoch accounts hash
///
/// Handles setting when an EAH calculation is requested and when it completes.  Also handles
/// waiting for in-flight calculations to complete when the "stop" Bank must include it.
#[derive(Debug)]
pub struct Manager {
    /// Current state of the epoch accounts hash
    state: Mutex<State>,
    /// This condition variable is used to wait for an in-flight EAH calculation to complete
    cvar: Condvar,
}

impl Manager {
    #[must_use]
    fn _new(state: State) -> Self {
        Self {
            state: Mutex::new(state),
            cvar: Condvar::new(),
        }
    }

    /// Create a new epoch accounts hash manager, with the initial state set to Invalid
    #[must_use]
    pub fn new_invalid() -> Self {
        Self::_new(State::Invalid)
    }

    /// Create a new epoch accounts hash manager, with the initial state set to Valid
    #[must_use]
    pub fn new_valid(epoch_accounts_hash: EpochAccountsHash, slot: Slot) -> Self {
        Self::_new(State::Valid(epoch_accounts_hash, slot))
    }

    /// An epoch accounts hash calculation has been requested; update our state
    pub fn set_in_flight(&self, slot: Slot) {
        let mut state = self.state.lock().unwrap();
        if let State::InFlight(old_slot) = &*state {
            panic!("An epoch accounts hash calculation is already in-flight from slot {old_slot}!");
        }
        *state = State::InFlight(slot);
    }

    /// An epoch accounts hash calculation has completed; update our state
    pub fn set_valid(&self, epoch_accounts_hash: EpochAccountsHash, slot: Slot) {
        let mut state = self.state.lock().unwrap();
        if let State::Valid(old_epoch_accounts_hash, old_slot) = &*state {
            panic!(
                "The epoch accounts hash is already valid! \
                \nold slot: {old_slot}, epoch accounts hash: {old_epoch_accounts_hash:?} \
                \nnew slot: {slot}, epoch accounts hash: {epoch_accounts_hash:?}"
            );
        }
        *state = State::Valid(epoch_accounts_hash, slot);
        self.cvar.notify_all();
    }

    /// Get the epoch accounts hash
    ///
    /// If an EAH calculation is in-flight, then this call will block until it completes.
    pub fn wait_get_epoch_accounts_hash(&self) -> EpochAccountsHash {
        let mut state = self.state.lock().unwrap();
        loop {
            match &*state {
                State::Valid(epoch_accounts_hash, _slot) => break *epoch_accounts_hash,
                State::Invalid => break SENTINEL_EPOCH_ACCOUNTS_HASH,
                State::InFlight(_slot) => state = self.cvar.wait(state).unwrap(),
            }
        }
    }

    /// Get the epoch accounts hash
    ///
    /// This fn does not block, and will only yield an EAH if the state is `Valid`
    pub fn try_get_epoch_accounts_hash(&self) -> Option<EpochAccountsHash> {
        let state = self.state.lock().unwrap();
        match &*state {
            State::Valid(epoch_accounts_hash, _slot) => Some(*epoch_accounts_hash),
            _ => None,
        }
    }

    /// **FOR TESTS ONLY**
    /// Set the state to Invalid
    /// This is needed by tests that do not fully startup all the accounts background services.
    /// **FOR TESTS ONLY**
    pub fn set_invalid_for_tests(&self) {
        *self.state.lock().unwrap() = State::Invalid;
    }
}

/// The EpochAccountsHash is calculated in the background via AccountsBackgroundService.  This enum
/// is used to track the state of that calculation, and queried when saving the EAH into a Bank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// On startup from genesis/slot0, the initial state of the EAH is invalid since one has not
    /// yet been requested.  This state should only really occur for tests and new clusters; not
    /// for established running clusters.
    Invalid,
    /// An EAH calculation has been requested (for `Slot`) and is in flight.  The Bank that should
    /// save the EAH must wait until the calculation has completed.
    InFlight(Slot),
    /// The EAH calculation is complete (for `Slot`) and the EAH value is valid to read/use.
    Valid(EpochAccountsHash, Slot),
}

/// Sentinel epoch accounts hash value; used when getting an Invalid EAH
///
/// Displays as "Sentine1EpochAccountsHash111111111111111111"
const SENTINEL_EPOCH_ACCOUNTS_HASH: EpochAccountsHash =
    EpochAccountsHash::new(Hash::new_from_array([
        0x06, 0x92, 0x40, 0x3b, 0xee, 0xea, 0x7e, 0xe2, 0x7d, 0xf4, 0x90, 0x7f, 0xbd, 0x9e, 0xd0,
        0xd2, 0x1c, 0x2b, 0x66, 0x9a, 0xc4, 0xda, 0xce, 0xd7, 0x23, 0x41, 0x69, 0xab, 0xb7, 0x80,
        0x00, 0x00,
    ]));

#[cfg(test)]
mod tests {
    use {super::*, std::time::Duration};

    #[test]
    fn test_new_valid() {
        let epoch_accounts_hash = EpochAccountsHash::new(Hash::new_unique());
        let manager = Manager::new_valid(epoch_accounts_hash, 5678);
        assert_eq!(
            manager.try_get_epoch_accounts_hash(),
            Some(epoch_accounts_hash),
        );
        assert_eq!(manager.wait_get_epoch_accounts_hash(), epoch_accounts_hash);
    }

    #[test]
    fn test_new_invalid() {
        let manager = Manager::new_invalid();
        assert!(manager.try_get_epoch_accounts_hash().is_none());
        assert_eq!(
            manager.wait_get_epoch_accounts_hash(),
            SENTINEL_EPOCH_ACCOUNTS_HASH,
        );
    }

    #[test]
    fn test_try_get_epoch_accounts_hash() {
        let epoch_accounts_hash = EpochAccountsHash::new(Hash::new_unique());
        for (state, expected) in [
            (State::Invalid, None),
            (State::InFlight(123), None),
            (
                State::Valid(epoch_accounts_hash, 5678),
                Some(epoch_accounts_hash),
            ),
        ] {
            let manager = Manager::_new(state);
            let actual = manager.try_get_epoch_accounts_hash();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn test_wait_epoch_accounts_hash() {
        // Test: State is Invalid, no need to wait
        {
            let manager = Manager::new_invalid();
            assert_eq!(
                manager.wait_get_epoch_accounts_hash(),
                SENTINEL_EPOCH_ACCOUNTS_HASH,
            );
        }

        // Test: State is Valid, no need to wait
        {
            let epoch_accounts_hash = EpochAccountsHash::new(Hash::new_unique());
            let manager = Manager::new_valid(epoch_accounts_hash, 5678);
            assert_eq!(manager.wait_get_epoch_accounts_hash(), epoch_accounts_hash);
        }

        // Test: State is InFlight, must wait
        {
            let epoch_accounts_hash = EpochAccountsHash::new(Hash::new_unique());
            let manager = Manager::new_invalid();
            manager.set_in_flight(123);

            std::thread::scope(|s| {
                s.spawn(|| {
                    std::thread::sleep(Duration::from_secs(1));
                    manager.set_valid(epoch_accounts_hash, 5678)
                });
                assert!(manager.try_get_epoch_accounts_hash().is_none());
                assert_eq!(manager.wait_get_epoch_accounts_hash(), epoch_accounts_hash);
                assert!(manager.try_get_epoch_accounts_hash().is_some());
            });
        }
    }
}
