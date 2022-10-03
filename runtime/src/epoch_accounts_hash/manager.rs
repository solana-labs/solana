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

    /// An epoch accounts hash calculation has complete; update our state
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
    pub fn get_epoch_accounts_hash(&self) -> EpochAccountsHash {
        let mut state = self.state.lock().unwrap();
        loop {
            match &*state {
                State::Valid(epoch_accounts_hash, _slot) => break *epoch_accounts_hash,
                State::Invalid => {
                    break EpochAccountsHash::new(Hash::new(
                        "sentinel epoch accounts hash val".as_bytes(),
                    ))
                }
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
    /// On boot, and if the snapshot does not already contain an EAH, the initial state of the EAH
    /// is invalid.  Since an EAH calculation has not yet been requested, the Bank should not wait
    /// for one to complete.
    Invalid,
    /// An EAH calculation has been requested (for `Slot`) and is in flight.  The Bank that should save the EAH
    /// must wait until the calculation has completed.
    InFlight(Slot),
    /// The EAH calculation is complete (for `Slot`) and the EAH value is valid to read/use.
    Valid(EpochAccountsHash, Slot),
}
