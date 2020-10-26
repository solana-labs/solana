pub mod state;
pub mod utils;
pub use state::State;

use crate::{account, nonce::state::Versions};
use std::cell::RefCell;

pub fn create_account(lamports: u64) -> RefCell<account::Account> {
    RefCell::new(
        account::Account::new_data_with_space(
            lamports,
            &Versions::new_current(State::Uninitialized),
            State::size(),
            &crate::system_program::id(),
        )
        .expect("nonce_account"),
    )
}
