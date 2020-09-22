use solana_sdk::{
    account::{Account, KeyedAccount},
    account_info::AccountInfo,
    clock::Slot,
    instruction::InstructionError,
    program_error::ProgramError,
};

solana_sdk::declare_id!("Feature111111111111111111111111111111111111");

/// The `Feature` struct is the on-chain representation of a runtime feature.
///
/// Feature activation is accomplished by:
/// 1. Activation is requested by the feature authority, who issues a transaction to create the
///    feature account.  The newly created feature account will have the value of
///    `Feature::default()`
/// 2. When the next epoch is entered the runtime will check for new activation requests and
///    active them.  When this occurs, the activation slot is recorded in the feature account
///
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Feature {
    pub activated_at: Option<Slot>,
}

impl Feature {
    pub fn size_of() -> usize {
        bincode::serialized_size(&Self {
            activated_at: Some(Slot::MAX),
        })
        .unwrap() as usize
    }
    pub fn from_account(account: &Account) -> Option<Self> {
        if account.owner != id() {
            None
        } else {
            bincode::deserialize(&account.data).ok()
        }
    }
    pub fn to_account(&self, account: &mut Account) -> Option<()> {
        bincode::serialize_into(&mut account.data[..], self).ok()
    }
    pub fn from_account_info(account_info: &AccountInfo) -> Result<Self, ProgramError> {
        bincode::deserialize(&account_info.data.borrow()).map_err(|_| ProgramError::InvalidArgument)
    }
    pub fn to_account_info(&self, account_info: &mut AccountInfo) -> Option<()> {
        bincode::serialize_into(&mut account_info.data.borrow_mut()[..], self).ok()
    }
    pub fn from_keyed_account(keyed_account: &KeyedAccount) -> Result<Self, InstructionError> {
        Self::from_account(&*keyed_account.try_account_ref()?)
            .ok_or(InstructionError::InvalidArgument)
    }
    pub fn create_account(&self, lamports: u64) -> Account {
        let data_len = Self::size_of().max(bincode::serialized_size(self).unwrap() as usize);
        let mut account = Account::new(lamports, data_len, &id());
        self.to_account(&mut account).unwrap();
        account
    }
}
