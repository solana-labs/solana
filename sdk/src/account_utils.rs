//! useful extras for Account state
use crate::account_api::{AccountApi, AccountWrapper};
use crate::credit_debit_account::{CreditDebitAccount, KeyedCreditDebitAccount};
use crate::instruction::InstructionError;
use bincode::{deserialize, serialize_into, ErrorKind};

/// Convenience trait to covert bincode errors to instruction errors.
pub trait State<T> {
    fn state(&self) -> Result<T, InstructionError>;
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError>;
}

impl<T> State<T> for CreditDebitAccount
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

impl<'a, T> State<T> for KeyedCreditDebitAccount<'a>
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

impl<'a, T> State<T> for AccountWrapper<'a>
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    fn state(&self) -> Result<T, InstructionError> {
        match self {
            AccountWrapper::CreditDebit(account) => {
                deserialize(account.get_data()).map_err(|_| InstructionError::InvalidAccountData)
            }
            AccountWrapper::CreditOnly(account) => {
                deserialize(account.get_data()).map_err(|_| InstructionError::InvalidAccountData)
            }
        }
    }
    fn set_state(&mut self, state: &T) -> Result<(), InstructionError> {
        match self {
            AccountWrapper::CreditDebit(_) => serialize_into(self.account_writer()?, state)
                .map_err(|err| match *err {
                    ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
                    _ => InstructionError::GenericError,
                }),
            AccountWrapper::CreditOnly(_) => {
                serialize_into(self.account_writer()?, state).map_err(|err| match *err {
                    ErrorKind::SizeLimit => InstructionError::AccountDataTooSmall,
                    _ => InstructionError::GenericError,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credit_debit_account::CreditDebitAccount;
    use crate::credit_only_account::{CreditOnlyAccount, KeyedCreditOnlyAccount};
    use crate::pubkey::Pubkey;
    use std::sync::Arc;

    #[test]
    fn test_account_state() {
        let state = 42u64;

        assert!(CreditDebitAccount::default().set_state(&state).is_err());
        let res = CreditDebitAccount::default().state() as Result<u64, InstructionError>;
        assert!(res.is_err());

        let mut account =
            CreditDebitAccount::new(0, std::mem::size_of::<u64>(), &Pubkey::default());

        assert!(account.set_state(&state).is_ok());
        let stored_state: u64 = account.state().unwrap();
        assert_eq!(stored_state, state);
    }

    #[test]
    fn test_keyed_account_state() {
        let state = 42u64;
        let key0 = Pubkey::new_rand();

        let mut account =
            CreditDebitAccount::new(0, std::mem::size_of::<u64>(), &Pubkey::default());
        let mut keyed_account = KeyedCreditDebitAccount::new(&key0, false, &mut account);

        assert!(keyed_account.set_state(&state).is_ok());
        let stored_state: u64 = keyed_account.state().unwrap();
        assert_eq!(stored_state, state);
    }

    #[test]
    fn test_account_wrapper_state() {
        let uninitialized_state = 0u64;
        let state0 = 42u64;
        let state1 = 84u64;
        let key0 = Pubkey::new_rand();
        let key1 = Pubkey::new_rand();

        let mut account =
            CreditDebitAccount::new(0, std::mem::size_of::<u64>(), &Pubkey::default());
        let keyed_account = KeyedCreditDebitAccount::new(&key0, false, &mut account);

        let account = CreditDebitAccount::new(1, std::mem::size_of::<u64>(), &Pubkey::default());
        let credit_account = Arc::new(CreditOnlyAccount::from(account));
        let keyed_credit_account = KeyedCreditOnlyAccount::new(&key1, false, &credit_account);

        let mut collection: Vec<AccountWrapper> = vec![
            AccountWrapper::CreditDebit(keyed_account),
            AccountWrapper::CreditOnly(keyed_credit_account),
        ];

        assert!(collection[0].set_state(&state0).is_ok());
        let stored_state: u64 = collection[0].state().unwrap();
        assert_eq!(stored_state, state0);

        assert!(collection[1].set_state(&state0).is_err());
        let stored_state: u64 = collection[1].state().unwrap();
        assert_eq!(stored_state, uninitialized_state);

        // Test accessing accounts through variable assignment
        let (credit_debit, _credit_only) = collection.split_at_mut(1);
        let credit_debit = &mut credit_debit[0];

        assert!(credit_debit.set_state(&state1).is_ok());
        let stored_state: u64 = collection[0].state().unwrap();
        assert_eq!(stored_state, state1);
    }

}
