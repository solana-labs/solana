//! Non-fungible token program

use crate::{
    nft_instruction::{InitializeAccountParams, NftError, NftInstruction},
    nft_state::NftState,
};
use solana_sdk::{
    account::KeyedAccount,
    instruction::InstructionError,
    instruction_processor_utils::{limited_deserialize, next_keyed_account},
    pubkey::Pubkey,
};

fn initialize_account(
    nft_state: &mut NftState,
    params: InitializeAccountParams,
    issuer_keyed_account: &KeyedAccount,
) -> Result<(), InstructionError> {
    if issuer_keyed_account.signer_key().is_none() {
        return Err(InstructionError::MissingRequiredSignature);
    }
    if nft_state.owner_pubkey != Pubkey::default() {
        return Err(InstructionError::AccountAlreadyInitialized);
    }
    nft_state.owner_pubkey = params.owner_pubkey;
    nft_state.issuer_pubkey = params.issuer_pubkey;
    nft_state.id = params.id;
    Ok(())
}

fn set_owner(
    nft_state: &mut NftState,
    new_pubkey: Pubkey,
    owner_keyed_account: &KeyedAccount,
) -> Result<(), InstructionError> {
    match owner_keyed_account.signer_key() {
        None => return Err(InstructionError::MissingRequiredSignature),
        Some(signer_key) => {
            if nft_state.owner_pubkey != *signer_key {
                return Err(NftError::IncorrectOwner.into());
            }
            nft_state.owner_pubkey = new_pubkey;
        }
    }
    Ok(())
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let instruction: NftInstruction = limited_deserialize(data)?;
    let keyed_accounts_iter = &mut keyed_accounts.iter_mut();
    let nft_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
    let mut nft_state: NftState = limited_deserialize(&nft_keyed_account.account.data)?;

    match instruction {
        NftInstruction::InitializeAccount(params) => {
            let issuer_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
            initialize_account(&mut nft_state, params, &issuer_keyed_account)?;
        }
        NftInstruction::SetOwner(new_pubkey) => {
            let owner_keyed_account = &mut next_keyed_account(keyed_accounts_iter)?;
            set_owner(&mut nft_state, new_pubkey, &owner_keyed_account)?;
        }
    }
    nft_state.serialize(&mut nft_keyed_account.account.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nft_instruction;
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

    fn create_nft_account(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        contract_keypair: &Keypair,
        issuer_keypair: &Keypair,
        owner_pubkey: &Pubkey,
        id: Vec<u8>,
        lamports: u64,
    ) -> Result<Signature> {
        let instructions = nft_instruction::create_account(
            &payer_keypair.pubkey(),
            &contract_keypair.pubkey(),
            &issuer_keypair.pubkey(),
            owner_pubkey,
            id,
            lamports,
        );
        let message = Message::new(instructions);
        bank_client.send_message(
            &[&payer_keypair, &contract_keypair, &issuer_keypair],
            message,
        )
    }

    fn send_set_owner(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        contract_pubkey: &Pubkey,
        old_owner_keypair: &Keypair,
        new_owner_pubkey: &Pubkey,
    ) -> Result<Signature> {
        let instruction = nft_instruction::set_owner(
            contract_pubkey,
            &old_owner_keypair.pubkey(),
            new_owner_pubkey,
        );
        let message = Message::new_with_payer(vec![instruction], Some(&payer_keypair.pubkey()));
        bank_client.send_message(&[&payer_keypair, &old_owner_keypair], message)
    }

    #[test]
    fn test_nft_set_owner() {
        let (bank_client, payer_keypair) = create_bank_client(2);
        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let owner_keypair = Keypair::new();
        let owner_pubkey = owner_keypair.pubkey();
        let issuer_keypair = Keypair::new();
        let id = vec![1, 2, 3];

        create_nft_account(
            &bank_client,
            &payer_keypair,
            &contract_keypair,
            &issuer_keypair,
            &owner_pubkey,
            id,
            1,
        )
        .unwrap();

        let new_owner_keypair = Keypair::new();
        let new_owner_pubkey = new_owner_keypair.pubkey();
        send_set_owner(
            &bank_client,
            &payer_keypair,
            &contract_pubkey,
            &owner_keypair,
            &new_owner_pubkey,
        )
        .unwrap();

        let nft_state_data = bank_client
            .get_account_data(&contract_pubkey)
            .unwrap()
            .unwrap();
        let nft_state: NftState = limited_deserialize(&nft_state_data).unwrap();
        assert_eq!(nft_state.owner_pubkey, new_owner_pubkey);
    }

    #[test]
    fn test_nft_already_initialized() {
        let issuer_pubkey = Pubkey::new_rand();
        let params = InitializeAccountParams {
            issuer_pubkey,
            owner_pubkey: Pubkey::new_rand(),
            id: vec![],
        };
        let mut nft_state = NftState {
            owner_pubkey: Pubkey::new_rand(), // Attack! Attempt to overwrite an existing NFT
            ..NftState::default()
        };
        let mut account = Account::new(1, 0, &system_program::id());
        let issuer_keyed_account = KeyedAccount::new(&issuer_pubkey, true, &mut account);

        let err = initialize_account(&mut nft_state, params, &issuer_keyed_account).unwrap_err();
        assert_eq!(err, InstructionError::AccountAlreadyInitialized);
    }

    #[test]
    fn test_nft_missing_issuer_signature() {
        let issuer_pubkey = Pubkey::new_rand();
        let params = InitializeAccountParams {
            issuer_pubkey,
            owner_pubkey: Pubkey::new_rand(),
            id: vec![],
        };
        let mut nft_state = NftState::default();
        let mut account = Account::new(1, 0, &system_program::id());
        let issuer_keyed_account = KeyedAccount::new(&issuer_pubkey, false, &mut account); // Attack! Attempt to create an NFT without the issuer's signature.

        let err = initialize_account(&mut nft_state, params, &issuer_keyed_account).unwrap_err();
        assert_eq!(err, InstructionError::MissingRequiredSignature);
    }

    #[test]
    fn test_nft_missing_owner_signature() {
        let owner_pubkey = Pubkey::new_rand();
        let mut nft_state = NftState {
            owner_pubkey,
            ..NftState::default()
        };
        let new_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 0, &system_program::id());
        let owner_keyed_account = KeyedAccount::new(&owner_pubkey, false, &mut account); // <-- Attack! Setting owner without the original owner's signature.
        let err = set_owner(&mut nft_state, new_pubkey, &owner_keyed_account).unwrap_err();
        assert_eq!(err, InstructionError::MissingRequiredSignature);
    }

    #[test]
    fn test_nft_incorrect_owner() {
        let owner_pubkey = Pubkey::new_rand();
        let mut nft_state = NftState {
            owner_pubkey,
            ..NftState::default()
        };
        let new_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 0, &system_program::id());
        let mallory_pubkey = Pubkey::new_rand(); // <-- Attack! Signing with wrong pubkey
        let owner_keyed_account = KeyedAccount::new(&mallory_pubkey, true, &mut account);
        let err = set_owner(&mut nft_state, new_pubkey, &owner_keyed_account).unwrap_err();
        assert_eq!(err, NftError::IncorrectOwner.into());
    }
}
