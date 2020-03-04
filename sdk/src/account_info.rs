use crate::{account::Account, clock::Epoch, program_error::ProgramError, pubkey::Pubkey};
use std::{
    cell::{Ref, RefCell, RefMut},
    cmp, fmt,
    rc::Rc,
};

/// Account information
#[derive(Clone)]
pub struct AccountInfo<'a> {
    /// Public key of the account
    pub key: &'a Pubkey,
    // Was the transaction signed by this account's public key?
    pub is_signer: bool,
    // Is the account writable?
    pub is_writable: bool,
    /// Account members that are mutable by the program
    pub lamports: Rc<RefCell<&'a mut u64>>,
    /// Account members that are mutable by the program
    pub data: Rc<RefCell<&'a mut [u8]>>,
    /// Program that owns this account
    pub owner: &'a Pubkey,
    /// This account's data contains a loaded program (and is now read-only)
    pub executable: bool,
    /// The epoch at which this account will next owe rent
    pub rent_epoch: Epoch,
}

impl<'a> fmt::Debug for AccountInfo<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_len = cmp::min(64, self.data_len());
        let data_str = if data_len > 0 {
            format!(
                " data: {} ...",
                hex::encode(self.data.borrow()[..data_len].to_vec())
            )
        } else {
            "".to_string()
        };
        write!(
            f,
            "AccountInfo {{ key: {} owner: {} is_signer: {} is_writable: {} executable: {} rent_epoch: {} lamports: {} data.len: {} {} }}",
            self.key,
            self.owner,
            self.is_signer,
            self.is_writable,
            self.executable,
            self.rent_epoch,
            self.lamports(),
            self.data_len(),
            data_str,
        )
    }
}

impl<'a> AccountInfo<'a> {
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

    pub fn lamports(&self) -> u64 {
        **self.lamports.borrow()
    }

    pub fn try_lamports(&self) -> Result<u64, ProgramError> {
        Ok(**self.try_borrow_lamports()?)
    }

    pub fn data_len(&self) -> usize {
        self.data.borrow().len()
    }

    pub fn try_data_len(&self) -> Result<usize, ProgramError> {
        Ok(self.try_borrow_data()?.len())
    }

    pub fn data_is_empty(&self) -> bool {
        self.data.borrow().is_empty()
    }

    pub fn try_data_is_empty(&self) -> Result<bool, ProgramError> {
        Ok(self.try_borrow_data()?.is_empty())
    }

    pub fn try_borrow_lamports(&self) -> Result<Ref<&mut u64>, ProgramError> {
        self.lamports
            .try_borrow()
            .map_err(|_| ProgramError::AccountBorrowFailed)
    }

    pub fn try_borrow_mut_lamports(&self) -> Result<RefMut<&'a mut u64>, ProgramError> {
        self.lamports
            .try_borrow_mut()
            .map_err(|_| ProgramError::AccountBorrowFailed)
    }

    pub fn try_borrow_data(&self) -> Result<Ref<&mut [u8]>, ProgramError> {
        self.data
            .try_borrow()
            .map_err(|_| ProgramError::AccountBorrowFailed)
    }

    pub fn try_borrow_mut_data(&self) -> Result<RefMut<&'a mut [u8]>, ProgramError> {
        self.data
            .try_borrow_mut()
            .map_err(|_| ProgramError::AccountBorrowFailed)
    }

    pub fn new(
        key: &'a Pubkey,
        is_signer: bool,
        is_writable: bool,
        lamports: &'a mut u64,
        data: &'a mut [u8],
        owner: &'a Pubkey,
        executable: bool,
        rent_epoch: Epoch,
    ) -> Self {
        Self {
            key,
            is_signer,
            is_writable,
            lamports: Rc::new(RefCell::new(lamports)),
            data: Rc::new(RefCell::new(data)),
            owner,
            executable,
            rent_epoch,
        }
    }

    pub fn deserialize_data<T: serde::de::DeserializeOwned>(&self) -> Result<T, bincode::Error> {
        bincode::deserialize(&self.data.borrow())
    }

    pub fn serialize_data<T: serde::Serialize>(&mut self, state: &T) -> Result<(), bincode::Error> {
        if bincode::serialized_size(state)? > self.data_len() as u64 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        bincode::serialize_into(&mut self.data.borrow_mut()[..], state)
    }
}

impl<'a> From<(&'a Pubkey, &'a mut Account)> for AccountInfo<'a> {
    fn from((key, account): (&'a Pubkey, &'a mut Account)) -> Self {
        Self::new(
            key,
            false,
            false,
            &mut account.lamports,
            &mut account.data,
            &account.owner,
            account.executable,
            account.rent_epoch,
        )
    }
}

impl<'a> From<(&'a Pubkey, bool, &'a mut Account)> for AccountInfo<'a> {
    fn from((key, is_signer, account): (&'a Pubkey, bool, &'a mut Account)) -> Self {
        Self::new(
            key,
            is_signer,
            false,
            &mut account.lamports,
            &mut account.data,
            &account.owner,
            account.executable,
            account.rent_epoch,
        )
    }
}

impl<'a> From<&'a mut (Pubkey, Account)> for AccountInfo<'a> {
    fn from((key, account): &'a mut (Pubkey, Account)) -> Self {
        Self::new(
            key,
            false,
            false,
            &mut account.lamports,
            &mut account.data,
            &account.owner,
            account.executable,
            account.rent_epoch,
        )
    }
}

pub fn create_account_infos(accounts: &mut [(Pubkey, Account)]) -> Vec<AccountInfo> {
    accounts.iter_mut().map(Into::into).collect()
}

pub fn create_is_signer_account_infos<'a>(
    accounts: &'a mut [(&'a Pubkey, bool, &'a mut Account)],
) -> Vec<AccountInfo<'a>> {
    accounts
        .iter_mut()
        .map(|(key, is_signer, account)| {
            AccountInfo::new(
                key,
                *is_signer,
                false,
                &mut account.lamports,
                &mut account.data,
                &account.owner,
                account.executable,
                account.rent_epoch,
            )
        })
        .collect()
}
