//! useful extras for Account state
use crate::{
    account::{Account, KeyedAccount},
    instruction::InstructionError,
};
use bincode::ErrorKind;

/// Convenience trait to covert bincode errors to instruction errors.
pub trait StateMut<T> {
    fn state(&self) -> Result<T, InstructionError>;
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError>;
}
pub trait State<T> {
    fn state(&self) -> Result<T, InstructionError>;
    fn set_state(&self, state: &T) -> Result<(), InstructionError>;
}

impl<T> StateMut<T> for Account
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
        self.try_account_ref()?.state()
    }
    fn set_state(&self, state: &T) -> Result<(), InstructionError> {
        self.try_account_ref_mut()?.set_state(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::Account;
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
}
