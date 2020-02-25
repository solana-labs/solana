use crate::hash::Hash;

#[repr(C)]
#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct AccountsEpochHash {
    pub hash: Hash,
}
