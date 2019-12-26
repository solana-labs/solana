//! useful extras for Account state
use crate::account::{Account, KeyedAccount};
use crate::instruction::InstructionError;
use crate::nonce_state::NonceState;
use crate::system_program;
use bincode::ErrorKind;

/// Convenience trait to covert bincode errors to instruction errors.
pub trait State<T> {
    fn state(&self) -> Result<T, InstructionError>;
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError>;
}

impl<T> State<T> for Account
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn state(&self) -> Result<T, InstructionError> {
        self.deserialize_data()
            .map_err(|_| InstructionError::InvalidAccountData)
    }
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError> {
        self.serialize_data(state).map_err(|err| match *err {
            ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
            _ => InstructionError::GenericError,
        })
    }
}

impl<'a, T> State<T> for KeyedAccount<'a>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn state(&self) -> Result<T, InstructionError> {
        self.account.state()
    }
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError> {
        self.account.set_state(state)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SystemAccountKind {
    System,
    Nonce,
}

pub fn account_is_system(account: &Account) -> Option<SystemAccountKind> {
    if system_program::check_id(&account.owner) {
        if account.data.is_empty() {
            Some(SystemAccountKind::System)
        } else if let Ok(NonceState::Initialized(_, _)) = account.state() {
            Some(SystemAccountKind::Nonce)
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Account;
    use crate::hash::Hash;
    use crate::nonce_state;
    use crate::pubkey::Pubkey;

    #[test]
    fn test_account_state() {
        let state = 42u64;

        assert!(Account::default().set_state(&state).is_err());
        let res = Account::default().state() as Result<u64, InstructionError>;
        assert!(res.is_err());

        let mut account = Account::new(0, std::mem::size_of::<u64>(), &Pubkey::default());

        assert!(account.set_state(&state).is_ok());
        let stored_state: u64 = account.state().unwrap();
        assert_eq!(stored_state, state);
    }

    #[test]
    fn test_account_is_system_system_ok() {
        let system_account = Account::default();
        assert_eq!(
            account_is_system(&system_account),
            Some(SystemAccountKind::System)
        );
    }

    #[test]
    fn test_account_is_system_nonce_ok() {
        let nonce_account = Account::new_data(
            42,
            &nonce_state::NonceState::Initialized(
                nonce_state::Meta::new(&Pubkey::default()),
                Hash::default(),
            ),
            &system_program::id(),
        )
        .unwrap();
        assert_eq!(
            account_is_system(&nonce_account),
            Some(SystemAccountKind::Nonce)
        );
    }

    #[test]
    fn test_account_is_system_uninitialized_nonce_account_fail() {
        assert_eq!(account_is_system(&nonce_state::create_account(42)), None);
    }

    #[test]
    fn test_account_is_system_system_owner_nonzero_nonnonce_data_fail() {
        let other_data_account = Account::new_data(42, b"other", &Pubkey::default()).unwrap();
        assert_eq!(account_is_system(&other_data_account), None);
    }

    #[test]
    fn test_account_is_system_nonsystem_owner_with_nonce_state_data_fail() {
        let nonce_account = Account::new_data(
            42,
            &nonce_state::NonceState::Initialized(
                nonce_state::Meta::new(&Pubkey::default()),
                Hash::default(),
            ),
            &Pubkey::new_rand(),
        )
        .unwrap();
        assert_eq!(account_is_system(&nonce_account), None);
    }
}
