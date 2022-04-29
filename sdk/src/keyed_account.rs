#![deprecated(
    since = "1.11.0",
    note = "Please use BorrowedAccount instead of KeyedAccount"
)]
#![allow(deprecated)]
use {
    crate::{
        account::{AccountSharedData, ReadableAccount},
        account_utils::{State, StateMut},
    },
    solana_program::{clock::Epoch, instruction::InstructionError, pubkey::Pubkey},
    std::{
        cell::{Ref, RefCell, RefMut},
        iter::FromIterator,
        rc::Rc,
    },
};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct KeyedAccount<'a> {
    is_signer: bool, // Transaction was signed by this account's key
    is_writable: bool,
    key: &'a Pubkey,
    pub account: &'a RefCell<AccountSharedData>,
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
        Ok(self.try_borrow()?.lamports())
    }

    pub fn data_len(&self) -> Result<usize, InstructionError> {
        Ok(self.try_borrow()?.data().len())
    }

    pub fn data_is_empty(&self) -> Result<bool, InstructionError> {
        Ok(self.try_borrow()?.data().is_empty())
    }

    pub fn owner(&self) -> Result<Pubkey, InstructionError> {
        Ok(*self.try_borrow()?.owner())
    }

    pub fn executable(&self) -> Result<bool, InstructionError> {
        Ok(self.try_borrow()?.executable())
    }

    pub fn rent_epoch(&self) -> Result<Epoch, InstructionError> {
        Ok(self.try_borrow()?.rent_epoch())
    }

    pub fn try_account_ref(&'a self) -> Result<Ref<AccountSharedData>, InstructionError> {
        self.try_borrow()
    }

    pub fn try_account_ref_mut(&'a self) -> Result<RefMut<AccountSharedData>, InstructionError> {
        self.try_borrow_mut()
    }

    fn try_borrow(&self) -> Result<Ref<AccountSharedData>, InstructionError> {
        self.account
            .try_borrow()
            .map_err(|_| InstructionError::AccountBorrowFailed)
    }
    fn try_borrow_mut(&self) -> Result<RefMut<AccountSharedData>, InstructionError> {
        self.account
            .try_borrow_mut()
            .map_err(|_| InstructionError::AccountBorrowFailed)
    }

    pub fn new(key: &'a Pubkey, is_signer: bool, account: &'a RefCell<AccountSharedData>) -> Self {
        Self {
            is_signer,
            is_writable: true,
            key,
            account,
        }
    }

    pub fn new_readonly(
        key: &'a Pubkey,
        is_signer: bool,
        account: &'a RefCell<AccountSharedData>,
    ) -> Self {
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

impl<'a> From<(&'a Pubkey, &'a RefCell<AccountSharedData>)> for KeyedAccount<'a> {
    fn from((key, account): (&'a Pubkey, &'a RefCell<AccountSharedData>)) -> Self {
        Self {
            is_signer: false,
            is_writable: true,
            key,
            account,
        }
    }
}

impl<'a> From<(&'a Pubkey, bool, &'a RefCell<AccountSharedData>)> for KeyedAccount<'a> {
    fn from((key, is_signer, account): (&'a Pubkey, bool, &'a RefCell<AccountSharedData>)) -> Self {
        Self {
            is_signer,
            is_writable: true,
            key,
            account,
        }
    }
}

impl<'a> From<&'a (&'a Pubkey, &'a RefCell<AccountSharedData>)> for KeyedAccount<'a> {
    fn from((key, account): &'a (&'a Pubkey, &'a RefCell<AccountSharedData>)) -> Self {
        Self {
            is_signer: false,
            is_writable: true,
            key,
            account,
        }
    }
}

pub fn create_keyed_accounts<'a>(
    accounts: &'a [(&'a Pubkey, &'a RefCell<AccountSharedData>)],
) -> Vec<KeyedAccount<'a>> {
    accounts.iter().map(Into::into).collect()
}

#[deprecated(
    since = "1.7.0",
    note = "Please use create_keyed_accounts_unified instead"
)]
pub fn create_keyed_is_signer_accounts<'a>(
    accounts: &'a [(&'a Pubkey, bool, &'a RefCell<AccountSharedData>)],
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

#[deprecated(
    since = "1.7.0",
    note = "Please use create_keyed_accounts_unified instead"
)]
pub fn create_keyed_readonly_accounts(
    accounts: &[(Pubkey, Rc<RefCell<AccountSharedData>>)],
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

pub fn create_keyed_accounts_unified<'a>(
    accounts: &[(bool, bool, &'a Pubkey, &'a RefCell<AccountSharedData>)],
) -> Vec<KeyedAccount<'a>> {
    accounts
        .iter()
        .map(|(is_signer, is_writable, key, account)| KeyedAccount {
            is_signer: *is_signer,
            is_writable: *is_writable,
            key,
            account,
        })
        .collect()
}

#[deprecated(
    since = "1.11.0",
    note = "Please use InstructionContext::get_signers() instead"
)]
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

#[deprecated(since = "1.7.0", note = "Please use keyed_account_at_index instead")]
/// Return the next KeyedAccount or a NotEnoughAccountKeys error
pub fn next_keyed_account<'a, 'b, I: Iterator<Item = &'a KeyedAccount<'b>>>(
    iter: &mut I,
) -> Result<I::Item, InstructionError> {
    iter.next().ok_or(InstructionError::NotEnoughAccountKeys)
}

/// Return the KeyedAccount at the specified index or a NotEnoughAccountKeys error
///
/// Index zero starts at the chain of program accounts, followed by the instruction accounts.
pub fn keyed_account_at_index<'a>(
    keyed_accounts: &'a [KeyedAccount],
    index: usize,
) -> Result<&'a KeyedAccount<'a>, InstructionError> {
    keyed_accounts
        .get(index)
        .ok_or(InstructionError::NotEnoughAccountKeys)
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
