use crate::{
    account::{from_account, Account},
    account_utils::{State, StateMut},
};
use solana_program::{clock::Epoch, instruction::InstructionError, pubkey::Pubkey, sysvar::Sysvar};
use std::{
    cell::{Ref, RefCell, RefMut},
    iter::FromIterator,
};

#[repr(C)]
#[derive(Debug)]
pub struct KeyedAccount<'a> {
    is_signer: bool, // Transaction was signed by this account's key
    is_writable: bool,
    key: &'a Pubkey,
    pub account: &'a RefCell<Account>,
}

impl<'a> KeyedAccount<'a> {
    pub fn signer_key(&self) -> Option<&Pubkey> {
        if self.is_signer {
            Some(self.key)
        } else {
            None
        }
    }

    pub fn unsigned_key(&self) -> &Pubkey {
        self.key
    }

    pub fn is_writable(&self) -> bool {
        self.is_writable
    }

    pub fn lamports(&self) -> Result<u64, InstructionError> {
        Ok(self.try_borrow()?.lamports)
    }

    pub fn data_len(&self) -> Result<usize, InstructionError> {
        Ok(self.try_borrow()?.data.len())
    }

    pub fn data_is_empty(&self) -> Result<bool, InstructionError> {
        Ok(self.try_borrow()?.data.is_empty())
    }

    pub fn owner(&self) -> Result<Pubkey, InstructionError> {
        Ok(self.try_borrow()?.owner)
    }

    pub fn executable(&self) -> Result<bool, InstructionError> {
        Ok(self.try_borrow()?.executable)
    }

    pub fn rent_epoch(&self) -> Result<Epoch, InstructionError> {
        Ok(self.try_borrow()?.rent_epoch)
    }

    pub fn try_account_ref(&'a self) -> Result<Ref<Account>, InstructionError> {
        self.try_borrow()
    }

    pub fn try_account_ref_mut(&'a self) -> Result<RefMut<Account>, InstructionError> {
        self.try_borrow_mut()
    }

    fn try_borrow(&self) -> Result<Ref<Account>, InstructionError> {
        self.account
            .try_borrow()
            .map_err(|_| InstructionError::AccountBorrowFailed)
    }
    fn try_borrow_mut(&self) -> Result<RefMut<Account>, InstructionError> {
        self.account
            .try_borrow_mut()
            .map_err(|_| InstructionError::AccountBorrowFailed)
    }

    pub fn new(key: &'a Pubkey, is_signer: bool, account: &'a RefCell<Account>) -> Self {
        Self {
            is_signer,
            is_writable: true,
            key,
            account,
        }
    }

    pub fn new_readonly(key: &'a Pubkey, is_signer: bool, account: &'a RefCell<Account>) -> Self {
        Self {
            is_signer,
            is_writable: false,
            key,
            account,
        }
    }
}

impl<'a> PartialEq for KeyedAccount<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<'a> From<(&'a Pubkey, &'a RefCell<Account>)> for KeyedAccount<'a> {
    fn from((key, account): (&'a Pubkey, &'a RefCell<Account>)) -> Self {
        Self {
            is_signer: false,
            is_writable: true,
            key,
            account,
        }
    }
}

impl<'a> From<(&'a Pubkey, bool, &'a RefCell<Account>)> for KeyedAccount<'a> {
    fn from((key, is_signer, account): (&'a Pubkey, bool, &'a RefCell<Account>)) -> Self {
        Self {
            is_signer,
            is_writable: true,
            key,
            account,
        }
    }
}

impl<'a> From<&'a (&'a Pubkey, &'a RefCell<Account>)> for KeyedAccount<'a> {
    fn from((key, account): &'a (&'a Pubkey, &'a RefCell<Account>)) -> Self {
        Self {
            is_signer: false,
            is_writable: true,
            key,
            account,
        }
    }
}

pub fn create_keyed_accounts<'a>(
    accounts: &'a [(&'a Pubkey, &'a RefCell<Account>)],
) -> Vec<KeyedAccount<'a>> {
    accounts.iter().map(Into::into).collect()
}

pub fn create_keyed_is_signer_accounts<'a>(
    accounts: &'a [(&'a Pubkey, bool, &'a RefCell<Account>)],
) -> Vec<KeyedAccount<'a>> {
    accounts
        .iter()
        .map(|(key, is_signer, account)| KeyedAccount {
            is_signer: *is_signer,
            is_writable: false,
            key,
            account,
        })
        .collect()
}

pub fn create_keyed_readonly_accounts(
    accounts: &[(Pubkey, RefCell<Account>)],
) -> Vec<KeyedAccount> {
    accounts
        .iter()
        .map(|(key, account)| KeyedAccount {
            is_signer: false,
            is_writable: false,
            key,
            account,
        })
        .collect()
}

/// Return all the signers from a set of KeyedAccounts
pub fn get_signers<A>(keyed_accounts: &[KeyedAccount]) -> A
where
    A: FromIterator<Pubkey>,
{
    keyed_accounts
        .iter()
        .filter_map(|keyed_account| keyed_account.signer_key())
        .cloned()
        .collect::<A>()
}

/// Return the next KeyedAccount or a NotEnoughAccountKeys error
pub fn next_keyed_account<'a, 'b, I: Iterator<Item = &'a KeyedAccount<'b>>>(
    iter: &mut I,
) -> Result<I::Item, InstructionError> {
    iter.next().ok_or(InstructionError::NotEnoughAccountKeys)
}

/// Return true if the first keyed_account is executable, used to determine if
/// the loader should call a program's 'main'
pub fn is_executable(keyed_accounts: &[KeyedAccount]) -> Result<bool, InstructionError> {
    Ok(!keyed_accounts.is_empty() && keyed_accounts[0].executable()?)
}

impl<'a, T> State<T> for crate::keyed_account::KeyedAccount<'a>
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

pub fn from_keyed_account<S: Sysvar>(
    keyed_account: &crate::keyed_account::KeyedAccount,
) -> Result<S, InstructionError> {
    if !S::check_id(keyed_account.unsigned_key()) {
        return Err(InstructionError::InvalidArgument);
    }
    from_account::<S>(&*keyed_account.try_account_ref()?).ok_or(InstructionError::InvalidArgument)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::{create_account, to_account},
        pubkey::Pubkey,
    };
    use std::cell::RefCell;

    #[repr(C)]
    #[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
    struct TestSysvar {
        something: Pubkey,
    }
    crate::declare_id!("TestSysvar111111111111111111111111111111111");
    impl solana_program::sysvar::SysvarId for TestSysvar {
        fn check_id(pubkey: &crate::pubkey::Pubkey) -> bool {
            check_id(pubkey)
        }
    }
    impl Sysvar for TestSysvar {}

    #[test]
    fn test_sysvar_keyed_account_to_from() {
        let test_sysvar = TestSysvar::default();
        let key = crate::keyed_account::tests::id();
        let wrong_key = Pubkey::new_unique();

        let account = create_account(&test_sysvar, 42);
        let test_sysvar = from_account::<TestSysvar>(&account).unwrap();
        assert_eq!(test_sysvar, TestSysvar::default());

        let mut account = Account::new(42, TestSysvar::size_of(), &key);
        to_account(&test_sysvar, &mut account).unwrap();
        let test_sysvar = from_account::<TestSysvar>(&account).unwrap();
        assert_eq!(test_sysvar, TestSysvar::default());

        let account = RefCell::new(account);
        let keyed_account = KeyedAccount::new(&key, false, &account);
        let new_test_sysvar = from_keyed_account::<TestSysvar>(&keyed_account).unwrap();
        assert_eq!(test_sysvar, new_test_sysvar);

        let keyed_account = KeyedAccount::new(&wrong_key, false, &account);
        assert_eq!(
            from_keyed_account::<TestSysvar>(&keyed_account),
            Err(InstructionError::InvalidArgument)
        );
    }
}
