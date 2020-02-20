//! Ownable program

use crate::ownable_instruction::OwnableError;
use bincode::serialize_into;
use solana_sdk::{
    account::KeyedAccount,
    instruction::InstructionError,
    program_utils::{limited_deserialize, next_keyed_account},
    pubkey::Pubkey,
};

fn set_owner(
    account_owner_pubkey: &mut Pubkey,
    new_owner_pubkey: Pubkey,
    owner_keyed_account: &KeyedAccount,
) -> Result<(), InstructionError> {
    match owner_keyed_account.signer_key() {
        None => return Err(InstructionError::MissingRequiredSignature),
        Some(signer_key) => {
            if account_owner_pubkey != signer_key {
                return Err(OwnableError::IncorrectOwner.into());
            }
            *account_owner_pubkey = new_owner_pubkey;
        }
    }
    Ok(())
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let new_owner_pubkey: Pubkey = limited_deserialize(data)?;
    let keyed_accounts_iter = &mut keyed_accounts.iter();
    let account_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
    let mut account_owner_pubkey: Pubkey =
        limited_deserialize(&account_keyed_account.try_account_ref()?.data)?;

    if account_owner_pubkey == Pubkey::default() {
        account_owner_pubkey = new_owner_pubkey;
    } else {
        let owner_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
        set_owner(
            &mut account_owner_pubkey,
            new_owner_pubkey,
            &owner_keyed_account,
        )?;
    }

    let mut account = account_keyed_account.try_account_ref_mut()?;
    serialize_into(&mut account.data[..], &account_owner_pubkey)
        .map_err(|_| InstructionError::AccountDataTooSmall)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ownable_instruction;
    use solana_runtime::{bank::Bank, bank_client::BankClient};
    use solana_sdk::{
        account::Account,
        client::SyncClient,
        genesis_config::create_genesis_config,
        message::Message,
        signature::{Keypair, KeypairUtil, Signature},
        system_program,
        transport::Result,
    };

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_config, mint_keypair) = create_genesis_config(lamports);
        let mut bank = Bank::new(&genesis_config);
        bank.add_instruction_processor(crate::id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_bank_client(lamports: u64) -> (BankClient, Keypair) {
        let (bank, mint_keypair) = create_bank(lamports);
        (BankClient::new(bank), mint_keypair)
    }

    fn create_ownable_account(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        account_keypair: &Keypair,
        owner_pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<Signature> {
        let instructions = ownable_instruction::create_account(
            &payer_keypair.pubkey(),
            &account_keypair.pubkey(),
            owner_pubkey,
            lamports,
        );
        let message = Message::new(instructions);
        bank_client.send_message(&[payer_keypair, account_keypair], message)
    }

    fn send_set_owner(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        account_pubkey: &Pubkey,
        old_owner_keypair: &Keypair,
        new_owner_pubkey: &Pubkey,
    ) -> Result<Signature> {
        let instruction = ownable_instruction::set_owner(
            account_pubkey,
            &old_owner_keypair.pubkey(),
            new_owner_pubkey,
        );
        let message = Message::new_with_payer(vec![instruction], Some(&payer_keypair.pubkey()));
        bank_client.send_message(&[payer_keypair, old_owner_keypair], message)
    }

    #[test]
    fn test_ownable_set_owner() {
        let (bank_client, payer_keypair) = create_bank_client(2);
        let account_keypair = Keypair::new();
        let account_pubkey = account_keypair.pubkey();
        let owner_keypair = Keypair::new();
        let owner_pubkey = owner_keypair.pubkey();

        create_ownable_account(
            &bank_client,
            &payer_keypair,
            &account_keypair,
            &owner_pubkey,
            1,
        )
        .unwrap();

        let new_owner_keypair = Keypair::new();
        let new_owner_pubkey = new_owner_keypair.pubkey();
        send_set_owner(
            &bank_client,
            &payer_keypair,
            &account_pubkey,
            &owner_keypair,
            &new_owner_pubkey,
        )
        .unwrap();

        let account_data = bank_client
            .get_account_data(&account_pubkey)
            .unwrap()
            .unwrap();
        let account_owner_pubkey: Pubkey = limited_deserialize(&account_data).unwrap();
        assert_eq!(account_owner_pubkey, new_owner_pubkey);
    }

    #[test]
    fn test_ownable_missing_owner_signature() {
        let mut account_owner_pubkey = Pubkey::new_rand();
        let owner_pubkey = account_owner_pubkey;
        let new_owner_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 0, &system_program::id());
        let owner_keyed_account = KeyedAccount::new(&owner_pubkey, false, &account); // <-- Attack! Setting owner without the original owner's signature.
        let err = set_owner(
            &mut account_owner_pubkey,
            new_owner_pubkey,
            &owner_keyed_account,
        )
        .unwrap_err();
        assert_eq!(err, InstructionError::MissingRequiredSignature);
    }

    #[test]
    fn test_ownable_incorrect_owner() {
        let mut account_owner_pubkey = Pubkey::new_rand();
        let new_owner_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 0, &system_program::id());
        let mallory_pubkey = Pubkey::new_rand(); // <-- Attack! Signing with wrong pubkey
        let owner_keyed_account = KeyedAccount::new(&mallory_pubkey, true, &account);
        let err = set_owner(
            &mut account_owner_pubkey,
            new_owner_pubkey,
            &owner_keyed_account,
        )
        .unwrap_err();
        assert_eq!(err, OwnableError::IncorrectOwner.into());
    }
}
