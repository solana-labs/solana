//! ValidatorInfo program

use crate::validator_info_state::{ValidatorInfoError, ValidatorInfoState};
use bincode::serialized_size;
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::account_utils::State;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;
use std::{cmp, mem};

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    if keyed_accounts[0].signer_key().is_none() {
        error!("account[0].signer_key().is_none()");
        Err(InstructionError::MissingRequiredSignature)?;
    }

    let state_length = mem::size_of::<Pubkey>() + data.len();
    if keyed_accounts[1].account.data.len() < state_length {
        error!("validator info too long");
        Err(InstructionError::InvalidInstructionData)?;
    }

    let current_info: ValidatorInfoState = keyed_accounts[1].state()?;
    let current_info_length: usize = serialized_size(&current_info).unwrap() as usize;

    if current_info.validator_pubkey != Pubkey::default()
        && current_info.validator_pubkey != *keyed_accounts[0].signer_key().unwrap()
    {
        error!("validator pubkey does not match");
        Err(InstructionError::CustomError(
            ValidatorInfoError::ValidatorMismatch as u32,
        ))?;
    }

    if current_info.validator_pubkey == Pubkey::default() {
        let validator_pubkey = *keyed_accounts[0].unsigned_key();
        keyed_accounts[1].account.data[0..mem::size_of::<Pubkey>()]
            .copy_from_slice(validator_pubkey.as_ref());
    }
    let new_length = cmp::max(current_info_length, state_length);
    keyed_accounts[1].account.data[mem::size_of::<Pubkey>()..new_length].copy_from_slice(
        &[
            data,
            &vec![0; current_info_length.saturating_sub(state_length)],
        ]
        .concat(),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::validator_info_instruction;
    use bincode::serialize;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::TransactionError;
    use vcard::VCard;

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = create_genesis_block(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    #[test]
    fn test_initialize_validator_info() {
        let (bank, alice_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(bank);
        let alice_pubkey = alice_keypair.pubkey();
        let validator_info_pubkey = Pubkey::new_rand();
        let validator_info = VCard::from_formatted_name_str("Validator Name").unwrap();
        let instructions = validator_info_instruction::create_account(
            &alice_pubkey,
            &validator_info_pubkey,
            &validator_info,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();

        let expected_state = ValidatorInfoState {
            validator_pubkey: alice_pubkey,
            validator_info: validator_info.to_string(),
        };
        let mut expected = serialize(&expected_state).unwrap();
        expected.resize(512, 0);

        assert_eq!(
            bank_client
                .get_account_data(&validator_info_pubkey)
                .unwrap(),
            Some(expected),
        );
    }

    #[test]
    fn test_update_validator_info() {
        let (bank, alice_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(bank);
        let alice_pubkey = alice_keypair.pubkey();
        let mallory_keypair = Keypair::new();
        let validator_info_pubkey = Pubkey::new_rand();
        let validator_info = VCard::from_formatted_name_str("Validator Name").unwrap();
        let instructions = validator_info_instruction::create_account(
            &alice_pubkey,
            &validator_info_pubkey,
            &validator_info,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();

        // Attempt to update validator info using another keypair
        bank_client
            .transfer(1, &alice_keypair, &mallory_keypair.pubkey())
            .unwrap(); // Fund mallory_keypair to avoid ANF error
        let bad_validator_info = VCard::from_formatted_name_str("Malicious Mallory").unwrap();
        let instruction = validator_info_instruction::write_validator_info(
            &mallory_keypair.pubkey(),
            &validator_info_pubkey,
            &bad_validator_info,
        );
        let message = Message::new(vec![instruction]);
        assert_eq!(
            bank_client
                .send_message(&[&mallory_keypair], message)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::CustomError(0))
        );

        // Update validator info using original keypair
        let new_validator_info = VCard::from_formatted_name_str("Alice Update").unwrap();
        let instruction = validator_info_instruction::write_validator_info(
            &alice_pubkey,
            &validator_info_pubkey,
            &new_validator_info,
        );
        let message = Message::new(vec![instruction]);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();

        let expected_state = ValidatorInfoState {
            validator_pubkey: alice_pubkey,
            validator_info: new_validator_info.to_string(),
        };
        let mut expected = serialize(&expected_state).unwrap();
        expected.resize(512, 0);

        assert_eq!(
            bank_client
                .get_account_data(&validator_info_pubkey)
                .unwrap(),
            Some(expected),
        );
    }
}
