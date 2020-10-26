use crate::{
    account::Account,
    account_utils::StateMut,
    fee_calculator::FeeCalculator,
    hash::Hash,
    nonce::{state::Versions, State},
};

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
