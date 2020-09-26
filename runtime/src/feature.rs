use solana_sdk::{account::Account, clock::Slot};

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
    pub fn create_account(&self, lamports: u64) -> Account {
        let data_len = Self::size_of().max(bincode::serialized_size(self).unwrap() as usize);
        let mut account = Account::new(lamports, data_len, &id());
        self.to_account(&mut account).unwrap();
        account
    }
}
