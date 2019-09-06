//! Serialize clock support for AccountInfo
//!
use crate::account_info::AccountInfo;
use crate::sysvar::clock::Clock;
use bincode::serialized_size;

impl Clock {
    pub fn from(account: &AccountInfo) -> Option<Self> {
        account.deserialize_data().ok()
    }
    pub fn to(&self, account: &mut AccountInfo) -> Option<()> {
        account.serialize_data(self).ok()
    }

    pub fn size_of() -> usize {
        serialized_size(&Self::default()).unwrap() as usize
    }
}
