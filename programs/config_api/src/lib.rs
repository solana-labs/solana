use bincode::{deserialize, serialize, serialized_size};
use serde_derive::{Deserialize, Serialize};
use solana_sdk::{account::Account, pubkey::Pubkey, short_vec};

pub mod config_instruction;
pub mod config_processor;

const CONFIG_PROGRAM_ID: [u8; 32] = [
    3, 6, 74, 163, 0, 47, 116, 220, 200, 110, 67, 49, 15, 12, 5, 42, 248, 197, 218, 39, 246, 16,
    64, 25, 163, 35, 239, 160, 0, 0, 0, 0,
];

solana_sdk::solana_name_id!(
    CONFIG_PROGRAM_ID,
    "Config1111111111111111111111111111111111111"
);

pub trait ConfigState: serde::Serialize + Default {
    /// Maximum space that the serialized representation will require
    fn max_space() -> u64;
}

/// A collection of keys to be stored in Config account data.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ConfigKeys {
    // Each key tuple comprises a unique `Pubkey` identifier,
    // and `bool` whether that key is a signer of the data
    #[serde(with = "short_vec")]
    pub keys: Vec<(Pubkey, bool)>,
}

impl ConfigKeys {
    pub fn serialized_size(keys: Vec<(Pubkey, bool)>) -> u64 {
        serialized_size(&ConfigKeys { keys }).unwrap()
    }
}

pub fn get_config_data(bytes: &[u8]) -> Result<&[u8], bincode::Error> {
    deserialize::<ConfigKeys>(bytes)
        .and_then(|keys| serialized_size(&keys))
        .map(|offset| &bytes[offset as usize..])
}

// utility for pre-made Accounts
pub fn create_config_account<T: ConfigState>(
    keys: Vec<(Pubkey, bool)>,
    config_data: &T,
    lamports: u64,
) -> Account {
    let mut data = serialize(&ConfigKeys { keys }).unwrap();
    data.extend_from_slice(&serialize(config_data).unwrap());
    Account {
        lamports,
        data,
        owner: id(),
        ..Account::default()
    }
}
