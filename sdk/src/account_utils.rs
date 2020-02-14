//! useful extras for Account state
use crate::{
    account::{Account, KeyedAccount},
    program_error::ProgramError,
};
use bincode::ErrorKind;

/// Convenience trait to covert bincode errors to instruction errors.
pub trait StateMut<T> {
    fn state(&self) -> Result<T, ProgramError>;
    fn set_state(&mut self, state: &T) -> Result<(), ProgramError>;
}
pub trait State<T> {
    fn state(&self) -> Result<T, ProgramError>;
    fn set_state(&self, state: &T) -> Result<(), ProgramError>;
}

impl<T> StateMut<T> for Account
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn state(&self) -> Result<T, ProgramError> {
        self.deserialize_data()
            .map_err(|_| ProgramError::InvalidAccountData)
    }
    fn set_state(&mut self, state: &T) -> Result<(), ProgramError> {
        self.serialize_data(state).map_err(|err| match *err {
            ErrorKind::SizeLimit => ProgramError::AccountDataTooSmall,
            _ => ProgramError::SerializationFailed,
        })
    }
}

impl<'a, T> State<T> for KeyedAccount<'a>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn state(&self) -> Result<T, ProgramError> {
        self.try_account_ref()?.state()
    }
    fn set_state(&self, state: &T) -> Result<(), ProgramError> {
        self.try_account_ref_mut()?.set_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{account::Account, pubkey::Pubkey};

    #[test]
    fn test_account_state() {
        let state = 42u64;

        assert!(Account::default().set_state(&state).is_err());
        let res = Account::default().state() as Result<u64, ProgramError>;
        assert!(res.is_err());

        let mut account = Account::new(0, std::mem::size_of::<u64>(), &Pubkey::default());

        assert!(account.set_state(&state).is_ok());
        let stored_state: u64 = account.state().unwrap();
        assert_eq!(stored_state, state);
    }
}
