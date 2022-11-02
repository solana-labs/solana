//! Functions related to nonce accounts.

use {
    crate::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::StateMut,
        hash::Hash,
        nonce::{
            state::{Data, Versions},
            State,
        },
    },
    std::cell::RefCell,
};

pub fn create_account(lamports: u64) -> RefCell<AccountSharedData> {
    RefCell::new(
        AccountSharedData::new_data_with_space(
            lamports,
            &Versions::new(State::Uninitialized),
            State::size(),
            &crate::system_program::id(),
        )
        .expect("nonce_account"),
    )
}

/// Checks if the recent_blockhash field in Transaction verifies, and returns
/// nonce account data if so.
pub fn verify_nonce_account(
    account: &AccountSharedData,
    recent_blockhash: &Hash, // Transaction.message.recent_blockhash
) -> Option<Data> {
    (account.owner() == &crate::system_program::id())
        .then(|| {
            StateMut::<Versions>::state(account)
                .ok()?
                .verify_recent_blockhash(recent_blockhash)
                .cloned()
        })
        .flatten()
}

pub fn lamports_per_signature_of(account: &AccountSharedData) -> Option<u64> {
    match StateMut::<Versions>::state(account).ok()?.state() {
        State::Initialized(data) => Some(data.fee_calculator.lamports_per_signature),
        State::Uninitialized => None,
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::{
            fee_calculator::FeeCalculator,
            nonce::state::{Data, DurableNonce},
            pubkey::Pubkey,
            system_program,
        },
    };

    #[test]
    fn test_verify_bad_account_owner_fails() {
        let program_id = Pubkey::new_unique();
        assert_ne!(program_id, crate::system_program::id());
        let account = AccountSharedData::new_data_with_space(
            42,
            &Versions::new(State::Uninitialized),
            State::size(),
            &program_id,
        )
        .expect("nonce_account");
        assert_eq!(verify_nonce_account(&account, &Hash::default()), None);
    }

    fn new_nonce_account(versions: Versions) -> AccountSharedData {
        AccountSharedData::new_data(
            1_000_000,             // lamports
            &versions,             // state
            &system_program::id(), // owner
        )
        .unwrap()
    }

    #[test]
    fn test_verify_nonce_account() {
        let blockhash = Hash::from([171; 32]);
        let versions = Versions::Legacy(Box::new(State::Uninitialized));
        let account = new_nonce_account(versions);
        assert_eq!(verify_nonce_account(&account, &blockhash), None);
        assert_eq!(verify_nonce_account(&account, &Hash::default()), None);
        let versions = Versions::Current(Box::new(State::Uninitialized));
        let account = new_nonce_account(versions);
        assert_eq!(verify_nonce_account(&account, &blockhash), None);
        assert_eq!(verify_nonce_account(&account, &Hash::default()), None);
        let durable_nonce = DurableNonce::from_blockhash(&blockhash);
        let data = Data {
            authority: Pubkey::new_unique(),
            durable_nonce,
            fee_calculator: FeeCalculator {
                lamports_per_signature: 2718,
            },
        };
        let versions = Versions::Legacy(Box::new(State::Initialized(data.clone())));
        let account = new_nonce_account(versions);
        assert_eq!(verify_nonce_account(&account, &blockhash), None);
        assert_eq!(verify_nonce_account(&account, &Hash::default()), None);
        assert_eq!(verify_nonce_account(&account, &data.blockhash()), None);
        assert_eq!(
            verify_nonce_account(&account, durable_nonce.as_hash()),
            None
        );
        let durable_nonce = DurableNonce::from_blockhash(durable_nonce.as_hash());
        assert_ne!(data.durable_nonce, durable_nonce);
        let data = Data {
            durable_nonce,
            ..data
        };
        let versions = Versions::Current(Box::new(State::Initialized(data.clone())));
        let account = new_nonce_account(versions);
        assert_eq!(verify_nonce_account(&account, &blockhash), None);
        assert_eq!(verify_nonce_account(&account, &Hash::default()), None);
        assert_eq!(
            verify_nonce_account(&account, &data.blockhash()),
            Some(data.clone())
        );
        assert_eq!(
            verify_nonce_account(&account, durable_nonce.as_hash()),
            Some(data)
        );
    }
}
