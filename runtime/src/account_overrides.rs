use solana_sdk::{account::AccountSharedData, pubkey::Pubkey, sysvar};

/// Encapsulates overridden accounts, typically used for transaction simulations
#[derive(Default)]
pub struct AccountOverrides {
    pub slot_history: Option<AccountSharedData>,
}

impl AccountOverrides {
    /// Sets in the slot history
    ///
    /// Note: no checks are performed on the correctness of the contained data
    pub fn set_slot_history(&mut self, slot_history: Option<AccountSharedData>) {
        self.slot_history = slot_history;
    }

    /// Gets the account if it's found in the list of overrides
    pub fn get(&self, pubkey: &Pubkey) -> Option<&AccountSharedData> {
        if pubkey == &sysvar::slot_history::id() {
            self.slot_history.as_ref()
        } else {
            None
        }
    }
}
