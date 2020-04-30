//! Lamports credited to this account will be removed from the total supply (burned)

use crate::sysvar::Sysvar;

#[derive(Serialize, Deserialize, Default)]
struct Incinerator {}

crate::declare_sysvar_id!("Sysvar1nc1nerator11111111111111111111111111", Incinerator);

impl Sysvar for Incinerator {}
