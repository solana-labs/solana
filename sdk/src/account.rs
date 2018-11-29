use pubkey::Pubkey;

/// An Account with userdata that is stored on chain
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Account {
    /// tokens in the account
    pub tokens: u64,
    /// data held in this account
    pub userdata: Vec<u8>,
    /// the program that owns this account
    pub owner: Pubkey,
    /// this account's userdata contains a loaded program (and is now read-only)
    pub executable: bool,
    /// the loader for this account
    /// (Pubkey::default() if the account is not executable and thus was never 'loaded')
    pub loader: Pubkey,
}

impl Account {
    // TODO do we want to add executable and leader_owner even though they should always be false/default?
    pub fn new(tokens: u64, space: usize, owner: Pubkey) -> Account {
        Account {
            tokens,
            userdata: vec![0u8; space],
            owner,
            executable: false,
            loader: Pubkey::default(),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct KeyedAccount<'a> {
    is_signer: bool, // Transaction was signed by this account's key
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

    pub fn new(key: &'a Pubkey, is_signer: bool, account: &'a mut Account) -> KeyedAccount<'a> {
        KeyedAccount {
            key,
            is_signer,
            account,
        }
    }
}

impl<'a> From<(&'a Pubkey, &'a mut Account)> for KeyedAccount<'a> {
    fn from((key, account): (&'a Pubkey, &'a mut Account)) -> Self {
        KeyedAccount {
            is_signer: false,
            key,
            account,
        }
    }
}

impl<'a> From<&'a mut (Pubkey, Account)> for KeyedAccount<'a> {
    fn from((key, account): &'a mut (Pubkey, Account)) -> Self {
        KeyedAccount {
            is_signer: false,
            key,
            account,
        }
    }
}

pub fn create_keyed_accounts(accounts: &mut [(Pubkey, Account)]) -> Vec<KeyedAccount> {
    accounts.iter_mut().map(Into::into).collect()
}
