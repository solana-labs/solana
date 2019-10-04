//! vest program
use crate::date_instruction::DateConfig;
use crate::{
    vest_instruction::{VestError, VestInstruction},
    vest_state::VestState,
};
use bincode::deserialize;
use chrono::prelude::*;
use solana_config_api::get_config_data;
use solana_sdk::{
    account::{Account, KeyedAccount},
    instruction::InstructionError,
    pubkey::Pubkey,
};

fn parse_date_account(
    keyed_account: &mut KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<Date<Utc>, InstructionError> {
    if keyed_account.account.owner != solana_config_api::id() {
        return Err(InstructionError::IncorrectProgramId);
    }

    let account = parse_account(keyed_account, expected_pubkey)?;

    let config_data =
        get_config_data(&account.data).map_err(|_| InstructionError::InvalidAccountData)?;
    let date_config =
        deserialize::<DateConfig>(config_data).map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(date_config.dt.date())
}

fn parse_account<'a>(
    keyed_account: &'a mut KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<&'a mut Account, InstructionError> {
    if keyed_account.unsigned_key() != expected_pubkey {
        return Err(VestError::Unauthorized.into());
    }

    Ok(keyed_account.account)
}

fn parse_signed_account<'a>(
    keyed_account: &'a mut KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<&'a mut Account, InstructionError> {
    if keyed_account.signer_key().is_none() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    parse_account(keyed_account, expected_pubkey)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let instruction = deserialize(data).map_err(|_| InstructionError::InvalidInstructionData)?;

    match instruction {
        VestInstruction::InitializeAccount {
            terminator_pubkey,
            payee_pubkey,
            start_date_time,
            date_pubkey,
            total_lamports,
        } => {
            let contract_account = &mut keyed_accounts[0].account;
            let vest_state = VestState {
                terminator_pubkey,
                payee_pubkey,
                start_date_time,
                date_pubkey,
                total_lamports,
                redeemed_lamports: 0,
            };
            vest_state.serialize(&mut contract_account.data)
        }
        VestInstruction::SetPayee(payee_pubkey) => {
            let (old_payee_keyed_account, contract_keyed_account) = match keyed_accounts {
                [ka0, ka1] => (ka0, ka1),
                _ => return Err(InstructionError::InvalidArgument),
            };
            let contract_account = &mut contract_keyed_account.account;
            let mut vest_state = VestState::deserialize(&contract_account.data)?;
            parse_signed_account(old_payee_keyed_account, &vest_state.payee_pubkey)?;
            vest_state.payee_pubkey = payee_pubkey;
            vest_state.serialize(&mut contract_account.data)
        }
        VestInstruction::RedeemTokens => {
            let (date_keyed_account, contract_keyed_account, payee_keyed_account) =
                match keyed_accounts {
                    [ka0, ka1, ka2] => (ka0, ka1, ka2),
                    _ => return Err(InstructionError::InvalidArgument),
                };
            let contract_account = &mut contract_keyed_account.account;
            let mut vest_state = VestState::deserialize(&contract_account.data)?;
            let current_date = parse_date_account(date_keyed_account, &vest_state.date_pubkey)?;
            let payee_account = parse_account(payee_keyed_account, &vest_state.payee_pubkey)?;

            vest_state.redeem_tokens(current_date, contract_account, payee_account);
            vest_state.serialize(&mut contract_account.data)
        }
        VestInstruction::Terminate => {
            let (terminator_keyed_account, contract_keyed_account, payee_keyed_account) =
                match keyed_accounts {
                    [ka0, ka1] => (ka0, ka1, None),
                    [ka0, ka1, ka2] => (ka0, ka1, Some(ka2)),
                    _ => return Err(InstructionError::InvalidArgument),
                };
            let contract_account = &mut contract_keyed_account.account;
            let mut vest_state = VestState::deserialize(&contract_account.data)?;
            let terminator_account =
                parse_signed_account(terminator_keyed_account, &vest_state.terminator_pubkey)?;
            let payee_account = if let Some(payee_keyed_account) = payee_keyed_account {
                &mut payee_keyed_account.account
            } else {
                terminator_account
            };
            vest_state.terminate(contract_account, payee_account);
            vest_state.serialize(&mut contract_account.data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date_instruction;
    use crate::id;
    use crate::vest_instruction;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::create_genesis_block;
    use solana_sdk::hash::hash;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::transport::TransportError;
    use std::sync::Arc;

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = create_genesis_block(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(
            solana_config_api::id(),
            solana_config_api::config_processor::process_instruction,
        );
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    fn create_bank_client(lamports: u64) -> (BankClient, Keypair) {
        let (bank, mint_keypair) = create_bank(lamports);
        (BankClient::new(bank), mint_keypair)
    }

    /// Create a config account and use it as a date oracle.
    fn create_date_account(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        date_keypair: &Keypair,
        dt: Date<Utc>,
    ) -> Result<Signature, TransportError> {
        let date_pubkey = date_keypair.pubkey();

        let mut instructions =
            date_instruction::create_account(&payer_keypair.pubkey(), &date_pubkey, 1);
        instructions.push(date_instruction::store(&date_pubkey, dt));

        let message = Message::new(instructions);
        bank_client.send_message(&[&payer_keypair, &date_keypair], message)
    }

    fn store_date(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        date_keypair: &Keypair,
        dt: Date<Utc>,
    ) -> Result<Signature, TransportError> {
        let date_pubkey = date_keypair.pubkey();
        let instruction = date_instruction::store(&date_pubkey, dt);
        let message = Message::new_with_payer(vec![instruction], Some(&payer_keypair.pubkey()));
        bank_client.send_message(&[&payer_keypair, &date_keypair], message)
    }

    fn create_vest_account(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        payee_pubkey: &Pubkey,
        contract_pubkey: &Pubkey,
        start_date: Date<Utc>,
        date_pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<Signature, TransportError> {
        let instructions = vest_instruction::create_account(
            &payer_keypair.pubkey(),
            &payee_pubkey,
            &contract_pubkey,
            start_date,
            &date_pubkey,
            lamports,
        );
        let message = Message::new(instructions);
        bank_client.send_message(&[&payer_keypair], message)
    }

    fn send_set_payee(
        bank_client: &BankClient,
        old_payee_keypair: &Keypair,
        contract_pubkey: &Pubkey,
        new_payee_pubkey: &Pubkey,
    ) -> Result<Signature, TransportError> {
        let instruction = vest_instruction::set_payee(
            &old_payee_keypair.pubkey(),
            &contract_pubkey,
            &new_payee_pubkey,
        );
        bank_client.send_instruction(&old_payee_keypair, instruction)
    }

    fn send_redeem_tokens(
        bank_client: &BankClient,
        payer_keypair: &Keypair,
        payee_pubkey: &Pubkey,
        contract_pubkey: &Pubkey,
        date_pubkey: &Pubkey,
    ) -> Result<Signature, TransportError> {
        let instruction =
            vest_instruction::redeem_tokens(&date_pubkey, &contract_pubkey, &payee_pubkey);
        let message = Message::new_with_payer(vec![instruction], Some(&payer_keypair.pubkey()));
        bank_client.send_message(&[&payer_keypair], message)
    }

    #[test]
    fn test_parse_account_unauthorized() {
        // Ensure client can't sneak in with an untrusted date account.
        let date_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 0, &solana_config_api::id());
        let mut keyed_account = KeyedAccount::new(&date_pubkey, false, &mut account);

        let mallory_pubkey = Pubkey::new_rand(); // <-- Attack! Not the expected account.
        assert_eq!(
            parse_account(&mut keyed_account, &mallory_pubkey).unwrap_err(),
            VestError::Unauthorized.into()
        );
    }

    #[test]
    fn test_parse_signed_account_missing_signature() {
        // Ensure client can't sneak in with an unsigned account.
        let date_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 0, &solana_config_api::id());
        let mut keyed_account = KeyedAccount::new(&date_pubkey, false, &mut account); // <-- Attack! Unsigned transaction.

        assert_eq!(
            parse_signed_account(&mut keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::MissingRequiredSignature.into()
        );
    }

    #[test]
    fn test_parse_date_account_incorrect_program_id() {
        // Ensure client can't sneak in with a non-Config account.
        let date_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 0, &id()); // <-- Attack! Pass Vest account where Config account is expected.
        let mut keyed_account = KeyedAccount::new(&date_pubkey, false, &mut account);
        assert_eq!(
            parse_date_account(&mut keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::IncorrectProgramId
        );
    }

    #[test]
    fn test_parse_date_account_uninitialized_config() {
        // Ensure no panic when `get_config_data()` returns an error.
        let date_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 0, &solana_config_api::id()); // <-- Attack! Zero space.
        let mut keyed_account = KeyedAccount::new(&date_pubkey, false, &mut account);
        assert_eq!(
            parse_date_account(&mut keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::InvalidAccountData
        );
    }

    #[test]
    fn test_parse_date_account_invalid_date_config() {
        // Ensure no panic when `deserialize::<DateConfig>()` returns an error.
        let date_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 1, &solana_config_api::id()); // Attack! 1 byte, enough to sneak by `get_config_data()`, but not DateConfig deserialize.
        let mut keyed_account = KeyedAccount::new(&date_pubkey, false, &mut account);
        assert_eq!(
            parse_date_account(&mut keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::InvalidAccountData
        );
    }

    #[test]
    fn test_parse_date_account_deserialize() {
        // Ensure no panic when `deserialize::<DateConfig>()` returns an error.
        let date_pubkey = Pubkey::new_rand();
        let mut account = Account::new(1, 1, &solana_config_api::id()); // Attack! 1 byte, enough to sneak by `get_config_data()`, but not DateConfig deserialize.
        let mut keyed_account = KeyedAccount::new(&date_pubkey, false, &mut account);
        assert_eq!(
            parse_date_account(&mut keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::InvalidAccountData
        );
    }

    #[test]
    fn test_set_payee() {
        let (bank_client, alice_keypair) = create_bank_client(38);
        let date_pubkey = Pubkey::new_rand();
        let contract_pubkey = Pubkey::new_rand();
        let bob_keypair = Keypair::new();
        let bob_pubkey = bob_keypair.pubkey();
        let start_date = Utc.ymd(2018, 1, 1);

        create_vest_account(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            start_date,
            &date_pubkey,
            36,
        )
        .unwrap();

        let new_bob_pubkey = Pubkey::new_rand();

        // Ensure some rando can't change the payee.
        // Transfer bob a token to pay the transaction fee.
        let mallory_keypair = Keypair::new();
        bank_client
            .transfer(1, &alice_keypair, &mallory_keypair.pubkey())
            .unwrap();
        send_set_payee(
            &bank_client,
            &mallory_keypair,
            &contract_pubkey,
            &new_bob_pubkey,
        )
        .unwrap_err();

        // Ensure bob can update which account he wants vested funds transfered to.
        bank_client
            .transfer(1, &alice_keypair, &bob_pubkey)
            .unwrap();
        send_set_payee(
            &bank_client,
            &bob_keypair,
            &contract_pubkey,
            &new_bob_pubkey,
        )
        .unwrap();
    }

    #[test]
    fn test_redeem_tokens() {
        let (bank, alice_keypair) = create_bank(38);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);
        let alice_pubkey = alice_keypair.pubkey();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_date = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &alice_keypair, &date_keypair, current_date).unwrap();

        let contract_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let start_date = Utc.ymd(2018, 1, 1);

        create_vest_account(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            start_date,
            &date_pubkey,
            36,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 36);

        send_redeem_tokens(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            &date_pubkey,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 24);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 12);

        // Update the date oracle and redeem more tokens
        store_date(
            &bank_client,
            &alice_keypair,
            &date_keypair,
            Utc.ymd(2019, 2, 1),
        )
        .unwrap();

        // Force a new blockhash so that there's not a duplicate signature.
        for _ in 0..bank.ticks_per_slot() {
            bank.register_tick(&hash(&[1]));
        }

        send_redeem_tokens(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            &date_pubkey,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 23);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 13);
    }

    #[test]
    fn test_cancel_payment() {
        let (bank_client, alice_keypair) = create_bank_client(3);
        let alice_pubkey = alice_keypair.pubkey();
        let contract_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let start_date = Utc::now().date();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_date = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &alice_keypair, &date_keypair, current_date).unwrap();

        create_vest_account(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            start_date,
            &date_pubkey,
            1,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 1);

        // Now, terminate the transaction. alice gets her funds back
        // Note: that tokens up until the oracle date are *not* redeemed automatically.
        let instruction =
            vest_instruction::terminate(&alice_pubkey, &contract_pubkey, &alice_pubkey);
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 2);
        assert_eq!(
            bank_client.get_account_data(&contract_pubkey).unwrap(),
            None
        );
        assert_eq!(bank_client.get_account_data(&bob_pubkey).unwrap(), None);
    }
}
