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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account_utils::State as AccountUtilsState,
        hash::Hash,
        nonce::{account::with_test_keyed_account, Account as NonceAccount, State},
        sysvar::{recent_blockhashes::create_test_recent_blockhashes, rent::Rent},
    };
    use std::collections::HashSet;

    #[test]
    fn verify_nonce_ok() {
        with_test_keyed_account(42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap());
            let state: State = nonce_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, State::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_account.unsigned_key();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &Rent::free())
                .unwrap();
            assert!(verify_nonce_account(
                &nonce_account.account.borrow(),
                &recent_blockhashes[0].blockhash,
            ));
        });
    }

    #[test]
    fn verify_nonce_bad_acc_state_fail() {
        with_test_keyed_account(42, true, |nonce_account| {
            assert!(!verify_nonce_account(
                &nonce_account.account.borrow(),
                &Hash::default()
            ));
        });
    }

    #[test]
    fn verify_nonce_bad_query_hash_fail() {
        with_test_keyed_account(42, true, |nonce_account| {
            let mut signers = HashSet::new();
            signers.insert(nonce_account.signer_key().unwrap());
            let state: State = nonce_account.state().unwrap();
            // New is in Uninitialzed state
            assert_eq!(state, State::Uninitialized);
            let recent_blockhashes = create_test_recent_blockhashes(0);
            let authorized = nonce_account.unsigned_key();
            nonce_account
                .initialize_nonce_account(&authorized, &recent_blockhashes, &Rent::free())
                .unwrap();
            assert!(!verify_nonce_account(
                &nonce_account.account.borrow(),
                &recent_blockhashes[1].blockhash,
            ));
        });
    }
}
