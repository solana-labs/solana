use crate::{account::Account, pubkey::Pubkey};
use std::{
    cell::{Ref, RefCell, RefMut},
    cmp, fmt,
    rc::Rc,
};

/// Account information that is mutable by a program
pub struct AccountInfoMut<'a> {
    /// Number of lamports owned by this account
    pub lamports: &'a mut u64,
    /// On-chain data within this account
    pub data: &'a mut [u8],
}
/// Account information
#[derive(Clone)]
pub struct AccountInfo<'a> {
    /// Public key of the account
    pub key: &'a Pubkey,
    // Was the transaction signed by this account's public key?
    pub is_signer: bool,
    /// Account members that are mutable by the program
    pub m: Rc<RefCell<AccountInfoMut<'a>>>,
    /// Program that owns this account
    pub owner: &'a Pubkey,
}

impl<'a> fmt::Debug for AccountInfo<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_len = cmp::min(64, self.m.borrow().data.len());
        let data_str = if data_len > 0 {
            format!(
                " data: {}",
                hex::encode(self.m.borrow().data[..data_len].to_vec())
            )
        } else {
            "".to_string()
        };
        write!(
            f,
            "AccountInfo {{ lamports: {} data.len: {} owner: {} {} }}",
            self.m.borrow().lamports,
            self.m.borrow().data.len(),
            self.owner,
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

    pub fn try_account_ref(&'a self) -> Result<Ref<AccountInfoMut>, u32> {
        self.try_borrow()
    }

    pub fn try_account_ref_mut(&'a self) -> Result<RefMut<'a, AccountInfoMut>, u32> {
        self.try_borrow_mut()
    }

    fn try_borrow(&self) -> Result<Ref<AccountInfoMut>, u32> {
        self.m.try_borrow().map_err(|_| std::u32::MAX)
    }

    fn try_borrow_mut(&self) -> Result<RefMut<'a, AccountInfoMut>, u32> {
        self.m.try_borrow_mut().map_err(|_| std::u32::MAX)
    }

    pub fn new(
        key: &'a Pubkey,
        is_signer: bool,
        lamports: &'a mut u64,
        data: &'a mut [u8],
        owner: &'a Pubkey,
    ) -> Self {
        Self {
            key,
            is_signer,
            m: Rc::new(RefCell::new(AccountInfoMut { lamports, data })),
            owner,
        }
    }

    pub fn deserialize_data<T: serde::de::DeserializeOwned>(&self) -> Result<T, bincode::Error> {
        bincode::deserialize(&self.m.borrow().data)
    }

    pub fn serialize_data<T: serde::Serialize>(&mut self, state: &T) -> Result<(), bincode::Error> {
        if bincode::serialized_size(state)? > self.m.borrow().data.len() as u64 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        bincode::serialize_into(&mut self.m.borrow_mut().data[..], state)
    }
}

impl<'a> From<(&'a Pubkey, &'a mut Account)> for AccountInfo<'a> {
    fn from((key, account): (&'a Pubkey, &'a mut Account)) -> Self {
        Self::new(
            key,
            false,
            &mut account.lamports,
            &mut account.data,
            &account.owner,
        )
    }
}

impl<'a> From<(&'a Pubkey, bool, &'a mut Account)> for AccountInfo<'a> {
    fn from((key, is_signer, account): (&'a Pubkey, bool, &'a mut Account)) -> Self {
        Self::new(
            key,
            is_signer,
            &mut account.lamports,
            &mut account.data,
            &account.owner,
        )
    }
}

impl<'a> From<&'a mut (Pubkey, Account)> for AccountInfo<'a> {
    fn from((key, account): &'a mut (Pubkey, Account)) -> Self {
        Self::new(
            key,
            false,
            &mut account.lamports,
            &mut account.data,
            &account.owner,
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
                &mut account.lamports,
                &mut account.data,
                &account.owner,
            )
        })
        .collect()
}
