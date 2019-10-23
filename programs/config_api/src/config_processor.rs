//! Config program

use crate::ConfigKeys;
use bincode::deserialize;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::instruction_processor_utils::{limited_deserialize, next_keyed_account};
use solana_sdk::pubkey::Pubkey;

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let key_list: ConfigKeys = limited_deserialize(data)?;
    let keyed_accounts_iter = &mut keyed_accounts.iter_mut();
    let config_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
    let current_data: ConfigKeys =
        deserialize(&config_keyed_account.account.data).map_err(|err| {
            error!("Invalid data in account[0]: {:?} {:?}", data, err);
            InstructionError::InvalidAccountData
        })?;
    let current_signer_keys: Vec<Pubkey> = current_data
        .keys
        .iter()
        .filter(|(_, is_signer)| *is_signer)
        .map(|(pubkey, _)| *pubkey)
        .collect();

    if current_signer_keys.is_empty() {
        // Config account keypair must be a signer on account initilization,
        // or when no signers specified in Config data
        if config_keyed_account.signer_key().is_none() {
            error!("account[0].signer_key().is_none()");
            return Err(InstructionError::MissingRequiredSignature);
        }
    }

    let mut counter = 0;
    for (signer, _) in key_list.keys.iter().filter(|(_, is_signer)| *is_signer) {
        counter += 1;
        if signer != config_keyed_account.unsigned_key() {
            let signer_account = keyed_accounts_iter.next();
            if signer_account.is_none() {
                error!("account {:?} is not in account list", signer);
                return Err(InstructionError::MissingRequiredSignature);
            }
            let signer_key = signer_account.unwrap().signer_key();
            if signer_key.is_none() {
                error!("account {:?} signer_key().is_none()", signer);
                return Err(InstructionError::MissingRequiredSignature);
            }
            if signer_key.unwrap() != signer {
                error!(
                    "account[{:?}].signer_key() does not match Config data)",
                    counter + 1
                );
                return Err(InstructionError::MissingRequiredSignature);
            }
            // If Config account is already initialized, update signatures must match Config data
            if !current_data.keys.is_empty()
                && current_signer_keys
                    .iter()
                    .find(|&pubkey| pubkey == signer)
                    .is_none()
            {
                error!("account {:?} is not in stored signer list", signer);
                return Err(InstructionError::MissingRequiredSignature);
            }
        } else if config_keyed_account.signer_key().is_none() {
            error!("account[0].signer_key().is_none()");
            return Err(InstructionError::MissingRequiredSignature);
        }
    }

    // Check for Config data signers not present in incoming account update
    if current_signer_keys.len() > counter {
        error!(
            "too few signers: {:?}; expected: {:?}",
            counter,
            current_signer_keys.len()
        );
        return Err(InstructionError::MissingRequiredSignature);
    }

    if config_keyed_account.account.data.len() < data.len() {
        error!("instruction data too large");
        return Err(InstructionError::InvalidInstructionData);
    }

    config_keyed_account.account.data[0..data.len()].copy_from_slice(&data);
    Ok(())
}

#[cfg(test)]
mod tests {}
