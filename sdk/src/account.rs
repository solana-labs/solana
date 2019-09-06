use crate::{pubkey::Pubkey, clock::Epoch};
use std::{cmp, fmt};

/// An Account with data that is stored on chain
#[repr(C)]
#[derive(Serialize, Deserialize, Clone, Default, Eq, PartialEq)]
pub struct Account {
    /// lamports in the account
    pub lamports: u64,
    /// data held in this account
    pub data: Vec<u8>,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
    /// the epoch at which this account will next owe rent
    pub rent_epoch: Epoch,
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_len = cmp::min(64, self.data.len());
        let data_str = if data_len > 0 {
            format!(" data: {}", hex::encode(self.data[..data_len].to_vec()))
        } else {
            "".to_string()
        };
        write!(
            f,
            "Account {{ lamports: {} data.len: {} owner: {} executable: {} rent_epoch: {}{} }}",
            self.lamports,
            self.data.len(),
            self.owner,
            self.executable,
            self.rent_epoch,
            data_str,
        )
    }
}

impl Account {
    // TODO do we want to add executable and leader_owner even though they should always be false/default?
    pub fn new(lamports: u64, space: usize, owner: &Pubkey) -> Account {
        Account {
            lamports,
            data: vec![0u8; space],
            owner: *owner,
            ..Account::default()
        }
    }

    pub fn new_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<Account, bincode::Error> {
        let data = bincode::serialize(state)?;
        Ok(Account {
            lamports,
            data,
            owner: *owner,
            ..Account::default()
        })
    }

    pub fn new_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<Account, bincode::Error> {
        let mut account = Self::new(lamports, space, owner);

        account.serialize_data(state)?;

        Ok(account)
    }

    pub fn deserialize_data<T: serde::de::DeserializeOwned>(&self) -> Result<T, bincode::Error> {
        bincode::deserialize(&self.data)
    }

    pub fn serialize_data<T: serde::Serialize>(&mut self, state: &T) -> Result<(), bincode::Error> {
        if bincode::serialized_size(state)? > self.data.len() as u64 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        bincode::serialize_into(&mut self.data[..], state)
    }
}

pub type LamportCredit = u64;

#[repr(C)]
#[derive(Debug)]
pub struct KeyedAccount<'a> {
    is_signer: bool, // Transaction was signed by this account's key
    is_debitable: bool,
    key: &'a Pubkey,
    pub account: &'a mut Account,
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

    pub fn is_debitable(&self) -> bool {
        self.is_debitable
    }

    pub fn new(key: &'a Pubkey, is_signer: bool, account: &'a mut Account) -> KeyedAccount<'a> {
        KeyedAccount {
            is_signer,
            is_debitable: true,
            key,
            account,
        }
    }

    pub fn new_credit_only(
        key: &'a Pubkey,
        is_signer: bool,
        account: &'a mut Account,
    ) -> KeyedAccount<'a> {
        KeyedAccount {
            is_signer,
            is_debitable: false,
            key,
            account,
        }
    }
}

impl<'a> From<(&'a Pubkey, &'a mut Account)> for KeyedAccount<'a> {
    fn from((key, account): (&'a Pubkey, &'a mut Account)) -> Self {
        KeyedAccount {
            is_signer: false,
            is_debitable: true,
            key,
            account,
        }
    }
}

impl<'a> From<&'a mut (Pubkey, Account)> for KeyedAccount<'a> {
    fn from((key, account): &'a mut (Pubkey, Account)) -> Self {
        KeyedAccount {
            is_signer: false,
            is_debitable: true,
            key,
            account,
        }
    }
}

pub fn create_keyed_accounts(accounts: &mut [(Pubkey, Account)]) -> Vec<KeyedAccount> {
    accounts.iter_mut().map(Into::into).collect()
}

pub fn create_keyed_credit_only_accounts(accounts: &mut [(Pubkey, Account)]) -> Vec<KeyedAccount> {
    accounts
        .iter_mut()
        .map(|(key, account)| KeyedAccount {
            is_signer: false,
            is_debitable: false,
            key,
            account,
        })
        .collect()
}
