use {
    serde::{Deserialize, Serialize},
    solana_frozen_abi_macro::AbiExample,
    solana_sdk::{clock::Epoch, declare_id, pubkey::Pubkey, short_vec},
};

declare_id!("AddressMap111111111111111111111111111111111");

pub const DEACTIVATION_COOLDOWN: Epoch = 2;

/// Data structue of address map
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, AbiExample)]
pub struct AddressMap {
    // authority must sign for each addition and to close the map account
    pub authority: Pubkey,
    // record a deactivation epoch to help validators know when to remove
    // the map from their caches.
    pub deactivation_epoch: Epoch,
    // entries may not be modified once activated
    pub activated: bool,
    // list of entries, max capacity of u8::MAX
    #[serde(with = "short_vec")]
    pub entries: Vec<Pubkey>,
}
