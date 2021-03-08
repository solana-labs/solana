use crate::{account_data::AccountData, clock::Epoch, pubkey::Pubkey};
use solana_program::{
    account_info::AccountInfo,
    sysvar::{Sysvar, SysvarEnum},
};
use std::{cell::RefCell, cmp, convert::TryFrom, fmt, rc::Rc};

/// An Account with data that is stored on chain
#[repr(C)]
#[frozen_abi(digest = "4KogjjSHMyd2yNNzmgG8VotZJHmsXCpaYkNswHrz3mf2")]
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Default, AbiExample)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// lamports in the account
    pub lamports: u64,
    /// data held in this account
    pub data: AccountData,
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
    pub fn new(lamports: u64, space: usize, owner: &Pubkey) -> Self {
        Self {
            lamports,
            data: vec![0u8; space].into(),
            owner: *owner,
            ..Self::default()
        }
    }
    pub fn new_ref(lamports: u64, space: usize, owner: &Pubkey) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self::new(lamports, space, owner)))
    }

    pub fn new_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<Self, bincode::Error> {
        let data = bincode::serialize(state)?.into();
        Ok(Self {
            lamports,
            data,
            owner: *owner,
            ..Self::default()
        })
    }
    pub fn new_ref_data<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        owner: &Pubkey,
    ) -> Result<RefCell<Self>, bincode::Error> {
        Ok(RefCell::new(Self::new_data(lamports, state, owner)?))
    }

    pub fn new_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<Self, bincode::Error> {
        let mut account = Self::new(lamports, space, owner);

        account.serialize_data(state)?;

        Ok(account)
    }
    pub fn new_ref_data_with_space<T: serde::Serialize>(
        lamports: u64,
        state: &T,
        space: usize,
        owner: &Pubkey,
    ) -> Result<RefCell<Self>, bincode::Error> {
        Ok(RefCell::new(Self::new_data_with_space(
            lamports, state, space, owner,
        )?))
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

/// Create an `Account` from a `Sysvar`.
pub fn create_account<S>(sysvar: &S, lamports: u64) -> Account
where
    S: Sysvar + Clone + Into<SysvarEnum> + 'static,
{
    let data_len = S::size_of().max(bincode::serialized_size(sysvar).unwrap() as usize);
    let mut account = Account::new(lamports, data_len, &solana_program::sysvar::id());
    to_account::<S>(sysvar, &mut account).unwrap();
    account
}

/// Create a `Sysvar` from an `Account`'s data.
pub fn from_account<S>(account: &Account) -> Option<S>
where
    S: Sysvar + Clone + Into<SysvarEnum> + TryFrom<SysvarEnum> + 'static,
{
    account.data.get_sysvar()
}

/// Serialize a `Sysvar` into an `Account`'s data.
pub fn to_account<S>(sysvar: &S, account: &mut Account) -> Option<()>
where
    S: Sysvar + Clone + Into<SysvarEnum> + 'static,
{
    account.data.put_sysvar(sysvar).ok()
}

/// Return the information required to construct an `AccountInfo`.  Used by the
/// `AccountInfo` conversion implementations.
impl solana_program::account_info::Account for Account {
    // note that this makes a copy of the data if there is > 0 other references
    fn get(&mut self) -> (&mut u64, &mut [u8], &Pubkey, bool, Epoch) {
        (
            &mut self.lamports,
            &mut self.data,
            &self.owner,
            self.executable,
            self.rent_epoch,
        )
    }
}

/// Create `AccountInfo`s
pub fn create_account_infos(accounts: &mut [(Pubkey, Account)]) -> Vec<AccountInfo> {
    accounts.iter_mut().map(Into::into).collect()
}

/// Create `AccountInfo`s
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
                account.data.get_mut_data(),
                &account.owner,
                account.executable,
                account.rent_epoch,
            )
        })
        .collect()
}
