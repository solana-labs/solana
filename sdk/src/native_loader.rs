use crate::{account::Account, hash::Hash};
use num_derive::FromPrimitive;

crate::declare_id!("NativeLoader1111111111111111111111111111111");

#[derive(Debug, Clone, Deserialize, Eq, Hash, PartialEq, Serialize)]
pub struct Info {
    pub kind: Kind,
    pub name: String,
}
#[derive(Debug, Clone, Copy, Deserialize, Eq, FromPrimitive, Hash, PartialEq, Serialize)]
pub enum Kind {
    Program = 1,
    Loader = 2,
}

#[macro_export]
macro_rules! native_program_info(
    ($name:expr) => (
        $crate::native_loader::Info {
            kind: $crate::native_loader::Kind::Program,
            name: $name.to_string(),
        }
    )
);
#[macro_export]
macro_rules! native_loader_info(
    ($name:expr) => (
        $crate::native_loader::Info {
            kind: $crate::native_loader::Kind::Loader,
            name: $name.to_string(),
        }
    )
);

/// Create an executable account with the given shared object name.
pub fn create_loadable_account(info: &Info) -> Account {
    let mut data = vec![info.kind as u8];
    data.extend_from_slice(info.name.as_bytes());
    Account {
        lamports: 1,
        owner: id(),
        data,
        executable: true,
        rent_epoch: 0,
        hash: Hash::default(),
    }
}
