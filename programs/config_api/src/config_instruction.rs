use crate::id;
use crate::ConfigState;
use bincode::serialize;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::short_vec;
use solana_sdk::system_instruction;
use std::mem;

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct ConfigSigners {
    #[serde(with = "short_vec")]
    pub additional_signers: Vec<Pubkey>,
}

impl ConfigSigners {
    pub fn serialized_size(additional_signers: Vec<Pubkey>) -> usize {
        serialize(&ConfigSigners { additional_signers })
            .unwrap_or_else(|_| vec![])
            .len()
    }
}

/// Create a new, empty configuration account
pub fn create_account<T: ConfigState>(
    from_account_pubkey: &Pubkey,
    config_account_pubkey: &Pubkey,
    lamports: u64,
    additional_signers: Vec<Pubkey>,
) -> Instruction {
    let space = T::max_space()
        + mem::size_of::<u32>() as u64
        + ConfigSigners::serialized_size(additional_signers) as u64;
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
    account_type: u32,
    additional_signers: Vec<Pubkey>,
    data: &T,
) -> Instruction {
    let mut account_metas = vec![AccountMeta::new(*config_account_pubkey, true)];
    for signer_pubkey in additional_signers.iter() {
        account_metas.push(AccountMeta::new(*signer_pubkey, true));
    }
    let account_data = (account_type, ConfigSigners { additional_signers }, data);
    Instruction::new(id(), &account_data, account_metas)
}
