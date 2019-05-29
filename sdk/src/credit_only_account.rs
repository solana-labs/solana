use crate::account::Account;
use crate::pubkey::Pubkey;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::{cmp, fmt};

/// A credit-only account struct, allowing atomic adds to lamports, but no other changes
pub struct CreditOnlyAccount {
    /// lamports in the account
    pub lamports: AtomicU64,
    /// data held in this account
    pub data: Vec<u8>,
    /// the program that owns this account. If executable, the program that loads this account.
    pub owner: Pubkey,
    /// this account's data contains a loaded program (and is now read-only)
    pub executable: bool,
}

impl fmt::Debug for CreditOnlyAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let data_len = cmp::min(64, self.data.len());
        let data_str = if data_len > 0 {
            format!(" data: {}", hex::encode(self.data[..data_len].to_vec()))
        } else {
            "".to_string()
        };
        write!(
            f,
            "CreditOnlyAccount {{ lamports: {:?} data.len: {} owner: {} executable: {}{} }}",
            self.lamports,
            self.data.len(),
            self.owner,
            self.executable,
            data_str,
        )
    }
}

impl From<Account> for CreditOnlyAccount {
    fn from(account: Account) -> Self {
        CreditOnlyAccount {
            lamports: AtomicU64::from(account.lamports),
            data: account.data,
            owner: account.owner,
            executable: account.executable,
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct KeyedCreditOnlyAccount<'a> {
    is_signer: bool, // Transaction was signed by this account's key
    key: &'a Pubkey,
    pub credits: u64,
    pub account: &'a Arc<CreditOnlyAccount>,
}

impl<'a> KeyedCreditOnlyAccount<'a> {
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

    pub fn new(
        key: &'a Pubkey,
        is_signer: bool,
        account: &'a Arc<CreditOnlyAccount>,
    ) -> KeyedCreditOnlyAccount<'a> {
        KeyedCreditOnlyAccount {
            key,
            is_signer,
            credits: 0,
            account,
        }
    }

    pub fn credit(&mut self, lamports: u64) {
        self.credits += lamports;
    }
}

impl<'a> From<(&'a Pubkey, &'a Arc<CreditOnlyAccount>)> for KeyedCreditOnlyAccount<'a> {
    fn from((key, account): (&'a Pubkey, &'a Arc<CreditOnlyAccount>)) -> Self {
        KeyedCreditOnlyAccount {
            is_signer: false,
            key,
            credits: 0,
            account,
        }
    }
}
