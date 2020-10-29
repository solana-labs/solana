use crate::{
    account::Account,
    account_utils::StateMut,
    fee_calculator::FeeCalculator,
    hash::Hash,
    nonce::{state::Versions, State},
};
use std::cell::RefCell;

pub fn create_account(lamports: u64) -> RefCell<Account> {
    RefCell::new(
        Account::new_data_with_space(
            lamports,
            &Versions::new_current(State::Uninitialized),
            State::size(),
            &crate::system_program::id(),
        )
        .expect("nonce_account"),
    )
}

pub fn verify_nonce_account(acc: &Account, hash: &Hash) -> bool {
    match StateMut::<Versions>::state(acc).map(|v| v.convert_to_current()) {
        Ok(State::Initialized(ref data)) => *hash == data.blockhash,
        _ => false,
    }
}

pub fn fee_calculator_of(account: &Account) -> Option<FeeCalculator> {
    let state = StateMut::<Versions>::state(account)
        .ok()?
        .convert_to_current();
    match state {
        State::Initialized(data) => Some(data.fee_calculator),
        _ => None,
    }
}
