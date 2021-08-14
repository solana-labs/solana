use crate::{clock::Epoch, program_error::ProgramError, pubkey::Pubkey};
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
    /// Was the transaction signed by this account's public key?
    pub is_signer: bool,
    /// Is the account writable?
    pub is_writable: bool,
    /// The lamports in the account.  Modifiable by programs.
    pub lamports: Rc<RefCell<&'a mut u64>>,
    /// The data held in this account.  Modifiable by programs.
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

/// Constructs an `AccountInfo` from self, used in conversion implementations.
pub trait IntoAccountInfo<'a> {
    fn into_account_info(self) -> AccountInfo<'a>;
}
impl<'a, T: IntoAccountInfo<'a>> From<T> for AccountInfo<'a> {
    fn from(src: T) -> Self {
        src.into_account_info()
    }
}

/// Provides information required to construct an `AccountInfo`, used in
/// conversion implementations.
pub trait Account {
    fn get(&mut self) -> (&mut u64, &mut [u8], &Pubkey, bool, Epoch);
}

/// Convert (&'a Pubkey, &'a mut T) where T: Account into an `AccountInfo`
impl<'a, T: Account> IntoAccountInfo<'a> for (&'a Pubkey, &'a mut T) {
    fn into_account_info(self) -> AccountInfo<'a> {
        let (key, account) = self;
        let (lamports, data, owner, executable, rent_epoch) = account.get();
        AccountInfo::new(
            key, false, false, lamports, data, owner, executable, rent_epoch,
        )
    }
}

/// Convert (&'a Pubkey, bool, &'a mut T)  where T: Account into an
/// `AccountInfo`.
impl<'a, T: Account> IntoAccountInfo<'a> for (&'a Pubkey, bool, &'a mut T) {
    fn into_account_info(self) -> AccountInfo<'a> {
        let (key, is_signer, account) = self;
        let (lamports, data, owner, executable, rent_epoch) = account.get();
        AccountInfo::new(
            key, is_signer, false, lamports, data, owner, executable, rent_epoch,
        )
    }
}

/// Convert &'a mut (Pubkey, T) where T: Account into an `AccountInfo`.
impl<'a, T: Account> IntoAccountInfo<'a> for &'a mut (Pubkey, T) {
    fn into_account_info(self) -> AccountInfo<'a> {
        let (ref key, account) = self;
        let (lamports, data, owner, executable, rent_epoch) = account.get();
        AccountInfo::new(
            key, false, false, lamports, data, owner, executable, rent_epoch,
        )
    }
}

/// Return the next `AccountInfo` or a `NotEnoughAccountKeys` error.
pub fn next_account_info<'a, 'b, I: Iterator<Item = &'a AccountInfo<'b>>>(
    iter: &mut I,
) -> Result<I::Item, ProgramError> {
    iter.next().ok_or(ProgramError::NotEnoughAccountKeys)
}

/// Return a slice of the next `count` `AccountInfo`s or a
/// `NotEnoughAccountKeys` error.
pub fn next_account_infos<'a, 'b: 'a>(
    iter: &mut std::slice::Iter<'a, AccountInfo<'b>>,
    count: usize,
) -> Result<&'a [AccountInfo<'b>], ProgramError> {
    let accounts = iter.as_slice();
    if accounts.len() < count {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let (accounts, remaining) = accounts.split_at(count);
    *iter = remaining.iter();
    Ok(accounts)
}

impl<'a> AsRef<AccountInfo<'a>> for AccountInfo<'a> {
    fn as_ref(&self) -> &AccountInfo<'a> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_next_account_infos() {
        let k1 = Pubkey::new_unique();
        let k2 = Pubkey::new_unique();
        let k3 = Pubkey::new_unique();
        let k4 = Pubkey::new_unique();
        let k5 = Pubkey::new_unique();
        let l1 = &mut 0;
        let l2 = &mut 0;
        let l3 = &mut 0;
        let l4 = &mut 0;
        let l5 = &mut 0;
        let d1 = &mut [0u8];
        let d2 = &mut [0u8];
        let d3 = &mut [0u8];
        let d4 = &mut [0u8];
        let d5 = &mut [0u8];

        let infos = &[
            AccountInfo::new(&k1, false, false, l1, d1, &k1, false, 0),
            AccountInfo::new(&k2, false, false, l2, d2, &k2, false, 0),
            AccountInfo::new(&k3, false, false, l3, d3, &k3, false, 0),
            AccountInfo::new(&k4, false, false, l4, d4, &k4, false, 0),
            AccountInfo::new(&k5, false, false, l5, d5, &k5, false, 0),
        ];
        let infos_iter = &mut infos.iter();
        let info1 = next_account_info(infos_iter).unwrap();
        let info2_3_4 = next_account_infos(infos_iter, 3).unwrap();
        let info5 = next_account_info(infos_iter).unwrap();

        assert_eq!(k1, *info1.key);
        assert_eq!(k2, *info2_3_4[0].key);
        assert_eq!(k3, *info2_3_4[1].key);
        assert_eq!(k4, *info2_3_4[2].key);
        assert_eq!(k5, *info5.key);
    }
}
