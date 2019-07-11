use crate::id;
use crate::ConfigState;
use bincode::serialize;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::short_vec;
use solana_sdk::system_instruction;

/// A collection of keys to be stored in Config account data.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ConfigKeys {
    // Each key tuple comprises a unique `Pubkey` identifier,
    // and `bool` whether that key is a signer of the data
    #[serde(with = "short_vec")]
    pub keys: Vec<(Pubkey, bool)>,
}

impl ConfigKeys {
    pub fn serialized_size(keys: Vec<(Pubkey, bool)>) -> usize {
        serialize(&ConfigKeys { keys })
            .unwrap_or_else(|_| vec![])
            .len()
    }
}

/// Create a new, empty configuration account
pub fn create_account<T: ConfigState>(
    from_account_pubkey: &Pubkey,
    config_account_pubkey: &Pubkey,
    lamports: u64,
    keys: Vec<(Pubkey, bool)>,
) -> Instruction {
    let space = T::max_space() + ConfigKeys::serialized_size(keys) as u64;
    system_instruction::create_account(
        from_account_pubkey,
        config_account_pubkey,
        lamports,
        space,
        &id(),
    )
}

/// Store new data in a configuration account
pub fn store<T: ConfigState>(
    config_account_pubkey: &Pubkey,
    is_config_signer: bool,
    keys: Vec<(Pubkey, bool)>,
    data: &T,
) -> Instruction {
    let mut account_metas = vec![AccountMeta::new(*config_account_pubkey, is_config_signer)];
    for (signer_pubkey, _) in keys.iter().filter(|(_, is_signer)| *is_signer) {
        if signer_pubkey != config_account_pubkey {
            account_metas.push(AccountMeta::new(*signer_pubkey, true));
        }
    }
    let account_data = (ConfigKeys { keys }, data);
    Instruction::new(id(), &account_data, account_metas)
}
