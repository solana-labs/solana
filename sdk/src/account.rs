use pubkey::Pubkey;

/// An Account with userdata that is stored on chain
#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// tokens in the account
    pub tokens: i64,
    /// user data
    /// A transaction can write to its userdata
    pub userdata: Vec<u8>,
    /// contract id this contract belongs to
    pub program_id: Pubkey,

    /// this account contains a program (and is strictly read-only)
    pub executable: bool,

    /// the loader for this program (Pubkey::default() for no loader)
    pub loader_program_id: Pubkey,
}

impl Account {
    // TODO do we want to add executable and leader_program_id even though they should always be false/default?
    pub fn new(tokens: i64, space: usize, program_id: Pubkey) -> Account {
        Account {
            tokens,
            userdata: vec![0u8; space],
            program_id,
            executable: false,
            loader_program_id: Pubkey::default(),
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct KeyedAccount<'a> {
    pub key: &'a Pubkey,
    pub account: &'a mut Account,
}

impl<'a> From<(&'a Pubkey, &'a mut Account)> for KeyedAccount<'a> {
    fn from((key, account): (&'a Pubkey, &'a mut Account)) -> Self {
        KeyedAccount { key, account }
    }
}

impl<'a> From<&'a mut (Pubkey, Account)> for KeyedAccount<'a> {
    fn from((key, account): &'a mut (Pubkey, Account)) -> Self {
        KeyedAccount { key, account }
    }
}

pub fn create_keyed_accounts(accounts: &mut [(Pubkey, Account)]) -> Vec<KeyedAccount> {
    accounts.iter_mut().map(Into::into).collect()
}
