//! vest program
use crate::{
    vest_instruction::{VestError, VestInstruction},
    vest_state::VestState,
};
use chrono::prelude::*;
use solana_config_program::date_instruction::DateConfig;
use solana_config_program::get_config_data;
use solana_sdk::{
    account::{Account, KeyedAccount},
    instruction::InstructionError,
    program_utils::{limited_deserialize, next_keyed_account},
    pubkey::Pubkey,
};
use std::cell::RefMut;

fn verify_date_account(
    keyed_account: &KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<Date<Utc>, InstructionError> {
    if keyed_account.owner()? != solana_config_program::id() {
        return Err(InstructionError::IncorrectProgramId);
    }

    let account = verify_account(keyed_account, expected_pubkey)?;

    let config_data =
        get_config_data(&account.data).map_err(|_| InstructionError::InvalidAccountData)?;
    let date_config =
        DateConfig::deserialize(config_data).ok_or(InstructionError::InvalidAccountData)?;

    Ok(date_config.date_time.date())
}

fn verify_account<'a>(
    keyed_account: &'a KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<RefMut<'a, Account>, InstructionError> {
    if keyed_account.unsigned_key() != expected_pubkey {
        return Err(VestError::Unauthorized.into());
    }

    keyed_account.try_account_ref_mut()
}

fn verify_signed_account<'a>(
    keyed_account: &'a KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<RefMut<'a, Account>, InstructionError> {
    if keyed_account.signer_key().is_none() {
        return Err(InstructionError::MissingRequiredSignature);
    }

    verify_account(keyed_account, expected_pubkey)
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &[KeyedAccount],
    data: &[u8],
) -> Result<(), InstructionError> {
    let keyed_accounts_iter = &mut keyed_accounts.iter();
    let contract_account = &mut next_keyed_account(keyed_accounts_iter)?.try_account_ref_mut()?;

    let instruction = limited_deserialize(data)?;

    let mut vest_state = if let VestInstruction::InitializeAccount {
        terminator_pubkey,
        payee_pubkey,
        start_date_time,
        date_pubkey,
        total_lamports,
    } = instruction
    {
        VestState {
            terminator_pubkey,
            payee_pubkey,
            start_date_time,
            date_pubkey,
            total_lamports,
            ..VestState::default()
        }
    } else {
        VestState::deserialize(&contract_account.data)?
    };

    match instruction {
        VestInstruction::InitializeAccount { .. } => {}
        VestInstruction::SetTerminator(new_pubkey) => {
            verify_signed_account(
                next_keyed_account(keyed_accounts_iter)?,
                &vest_state.terminator_pubkey,
            )?;
            vest_state.terminator_pubkey = new_pubkey;
        }
        VestInstruction::SetPayee(new_pubkey) => {
            verify_signed_account(
                next_keyed_account(keyed_accounts_iter)?,
                &vest_state.payee_pubkey,
            )?;
            vest_state.payee_pubkey = new_pubkey;
        }
        VestInstruction::RedeemTokens => {
            let current_date = verify_date_account(
                next_keyed_account(keyed_accounts_iter)?,
                &vest_state.date_pubkey,
            )?;
            let mut payee_account = verify_account(
                next_keyed_account(keyed_accounts_iter)?,
                &vest_state.payee_pubkey,
            )?;
            vest_state.redeem_tokens(contract_account, current_date, &mut payee_account);
        }
        VestInstruction::Terminate | VestInstruction::Renege(_) => {
            let lamports = if let VestInstruction::Renege(lamports) = instruction {
                lamports
            } else {
                contract_account.lamports
            };
            let terminator_account = verify_signed_account(
                next_keyed_account(keyed_accounts_iter)?,
                &vest_state.terminator_pubkey,
            )?;
            let payee_keyed_account = keyed_accounts_iter.next();
            let mut payee_account = if let Some(payee_keyed_account) = payee_keyed_account {
                payee_keyed_account.try_account_ref_mut()?
            } else {
                terminator_account
            };
            vest_state.renege(contract_account, &mut payee_account, lamports);
        }
        VestInstruction::VestAll => {
            verify_signed_account(
                next_keyed_account(keyed_accounts_iter)?,
                &vest_state.terminator_pubkey,
            )?;
            vest_state.vest_all();
        }
    }

    vest_state.serialize(&mut contract_account.data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id;
    use crate::vest_instruction;
    use solana_config_program::date_instruction;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_config::create_genesis_config;
    use solana_sdk::hash::hash;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
    use solana_sdk::transaction::TransactionError;
    use solana_sdk::transport::Result;
    use std::sync::Arc;

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_config, mint_keypair) = create_genesis_config(lamports);
        let mut bank = Bank::new(&genesis_config);
        bank.add_instruction_processor(
            solana_config_program::id(),
            solana_config_program::config_processor::process_instruction,
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
        date_keypair: &Keypair,
        payer_keypair: &Keypair,
        date: Date<Utc>,
    ) -> Result<Signature> {
        let date_pubkey = date_keypair.pubkey();

        let mut instructions =
            date_instruction::create_account(&payer_keypair.pubkey(), &date_pubkey, 1);
        instructions.push(date_instruction::store(&date_pubkey, date));

        let message = Message::new(instructions);
        bank_client.send_message(&[payer_keypair, date_keypair], message)
    }

    fn store_date(
        bank_client: &BankClient,
        date_keypair: &Keypair,
        payer_keypair: &Keypair,
        date: Date<Utc>,
    ) -> Result<Signature> {
        let date_pubkey = date_keypair.pubkey();
        let instruction = date_instruction::store(&date_pubkey, date);
        let message = Message::new_with_payer(vec![instruction], Some(&payer_keypair.pubkey()));
        bank_client.send_message(&[payer_keypair, date_keypair], message)
    }

    fn create_vest_account(
        bank_client: &BankClient,
        contract_keypair: &Keypair,
        payer_keypair: &Keypair,
        terminator_pubkey: &Pubkey,
        payee_pubkey: &Pubkey,
        start_date: Date<Utc>,
        date_pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<Signature> {
        let instructions = vest_instruction::create_account(
            &payer_keypair.pubkey(),
            &terminator_pubkey,
            &contract_keypair.pubkey(),
            &payee_pubkey,
            start_date,
            &date_pubkey,
            lamports,
        );
        let message = Message::new(instructions);
        bank_client.send_message(&[payer_keypair, contract_keypair], message)
    }

    fn send_set_terminator(
        bank_client: &BankClient,
        contract_pubkey: &Pubkey,
        old_keypair: &Keypair,
        new_pubkey: &Pubkey,
    ) -> Result<Signature> {
        let instruction =
            vest_instruction::set_terminator(&contract_pubkey, &old_keypair.pubkey(), &new_pubkey);
        bank_client.send_instruction(&old_keypair, instruction)
    }

    fn send_set_payee(
        bank_client: &BankClient,
        contract_pubkey: &Pubkey,
        old_keypair: &Keypair,
        new_pubkey: &Pubkey,
    ) -> Result<Signature> {
        let instruction =
            vest_instruction::set_payee(&contract_pubkey, &old_keypair.pubkey(), &new_pubkey);
        bank_client.send_instruction(&old_keypair, instruction)
    }

    fn send_redeem_tokens(
        bank_client: &BankClient,
        contract_pubkey: &Pubkey,
        payer_keypair: &Keypair,
        payee_pubkey: &Pubkey,
        date_pubkey: &Pubkey,
    ) -> Result<Signature> {
        let instruction =
            vest_instruction::redeem_tokens(&contract_pubkey, &date_pubkey, &payee_pubkey);
        let message = Message::new_with_payer(vec![instruction], Some(&payer_keypair.pubkey()));
        bank_client.send_message(&[payer_keypair], message)
    }

    #[test]
    fn test_verify_account_unauthorized() {
        // Ensure client can't sneak in with an untrusted date account.
        let date_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 0, &solana_config_program::id());
        let keyed_account = KeyedAccount::new(&date_pubkey, false, &account);

        let mallory_pubkey = Pubkey::new_rand(); // <-- Attack! Not the expected account.
        assert_eq!(
            verify_account(&keyed_account, &mallory_pubkey).unwrap_err(),
            VestError::Unauthorized.into()
        );
    }

    #[test]
    fn test_verify_signed_account_missing_signature() {
        // Ensure client can't sneak in with an unsigned account.
        let date_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 0, &solana_config_program::id());
        let keyed_account = KeyedAccount::new(&date_pubkey, false, &account); // <-- Attack! Unsigned transaction.

        assert_eq!(
            verify_signed_account(&keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::MissingRequiredSignature.into()
        );
    }

    #[test]
    fn test_verify_date_account_incorrect_program_id() {
        // Ensure client can't sneak in with a non-Config account.
        let date_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 0, &id()); // <-- Attack! Pass Vest account where Config account is expected.
        let keyed_account = KeyedAccount::new(&date_pubkey, false, &account);
        assert_eq!(
            verify_date_account(&keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::IncorrectProgramId
        );
    }

    #[test]
    fn test_verify_date_account_uninitialized_config() {
        // Ensure no panic when `get_config_data()` returns an error.
        let date_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 0, &solana_config_program::id()); // <-- Attack! Zero space.
        let keyed_account = KeyedAccount::new(&date_pubkey, false, &account);
        assert_eq!(
            verify_date_account(&keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::InvalidAccountData
        );
    }

    #[test]
    fn test_verify_date_account_invalid_date_config() {
        // Ensure no panic when `deserialize::<DateConfig>()` returns an error.
        let date_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 1, &solana_config_program::id()); // Attack! 1 byte, enough to sneak by `get_config_data()`, but not DateConfig deserialize.
        let keyed_account = KeyedAccount::new(&date_pubkey, false, &account);
        assert_eq!(
            verify_date_account(&keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::InvalidAccountData
        );
    }

    #[test]
    fn test_verify_date_account_deserialize() {
        // Ensure no panic when `deserialize::<DateConfig>()` returns an error.
        let date_pubkey = Pubkey::new_rand();
        let account = Account::new_ref(1, 1, &solana_config_program::id()); // Attack! 1 byte, enough to sneak by `get_config_data()`, but not DateConfig deserialize.
        let keyed_account = KeyedAccount::new(&date_pubkey, false, &account);
        assert_eq!(
            verify_date_account(&keyed_account, &date_pubkey).unwrap_err(),
            InstructionError::InvalidAccountData
        );
    }

    #[test]
    fn test_initialize_no_panic() {
        let (bank_client, alice_keypair) = create_bank_client(3);

        let contract_keypair = Keypair::new();

        let mut instructions = vest_instruction::create_account(
            &alice_keypair.pubkey(),
            &Pubkey::new_rand(),
            &contract_keypair.pubkey(),
            &Pubkey::new_rand(),
            Utc::now().date(),
            &Pubkey::new_rand(),
            1,
        );
        instructions[1].accounts = vec![]; // <!-- Attack! Prevent accounts from being passed into processor.

        let message = Message::new(instructions);
        assert_eq!(
            bank_client
                .send_message(&[&alice_keypair, &contract_keypair], message)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(1, InstructionError::NotEnoughAccountKeys)
        );
    }
    #[test]
    fn test_set_payee_and_terminator() {
        let (bank_client, alice_keypair) = create_bank_client(39);
        let alice_pubkey = alice_keypair.pubkey();
        let date_pubkey = Pubkey::new_rand();
        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let bob_keypair = Keypair::new();
        let bob_pubkey = bob_keypair.pubkey();
        let start_date = Utc.ymd(2018, 1, 1);

        create_vest_account(
            &bank_client,
            &contract_keypair,
            &alice_keypair,
            &alice_pubkey,
            &bob_pubkey,
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
            &contract_pubkey,
            &mallory_keypair,
            &new_bob_pubkey,
        )
        .unwrap_err();

        // Ensure bob can update which account he wants vested funds transfered to.
        bank_client
            .transfer(1, &alice_keypair, &bob_pubkey)
            .unwrap();
        send_set_payee(
            &bank_client,
            &contract_pubkey,
            &bob_keypair,
            &new_bob_pubkey,
        )
        .unwrap();

        // Ensure the rando can't change the terminator either.
        let new_alice_pubkey = Pubkey::new_rand();
        send_set_terminator(
            &bank_client,
            &contract_pubkey,
            &mallory_keypair,
            &new_alice_pubkey,
        )
        .unwrap_err();

        // Ensure alice can update which pubkey she uses to terminate contracts.
        send_set_terminator(
            &bank_client,
            &contract_pubkey,
            &alice_keypair,
            &new_alice_pubkey,
        )
        .unwrap();
    }

    #[test]
    fn test_set_payee() {
        let (bank_client, alice_keypair) = create_bank_client(38);
        let alice_pubkey = alice_keypair.pubkey();
        let date_pubkey = Pubkey::new_rand();
        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let bob_keypair = Keypair::new();
        let bob_pubkey = bob_keypair.pubkey();
        let start_date = Utc.ymd(2018, 1, 1);

        create_vest_account(
            &bank_client,
            &contract_keypair,
            &alice_keypair,
            &alice_pubkey,
            &bob_pubkey,
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
            &contract_pubkey,
            &mallory_keypair,
            &new_bob_pubkey,
        )
        .unwrap_err();

        // Ensure bob can update which account he wants vested funds transfered to.
        bank_client
            .transfer(1, &alice_keypair, &bob_pubkey)
            .unwrap();
        send_set_payee(
            &bank_client,
            &contract_pubkey,
            &bob_keypair,
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
        create_date_account(&bank_client, &date_keypair, &alice_keypair, current_date).unwrap();

        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let bob_pubkey = Pubkey::new_rand();
        let start_date = Utc.ymd(2018, 1, 1);

        create_vest_account(
            &bank_client,
            &contract_keypair,
            &alice_keypair,
            &alice_pubkey,
            &bob_pubkey,
            start_date,
            &date_pubkey,
            36,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 36);

        send_redeem_tokens(
            &bank_client,
            &contract_pubkey,
            &alice_keypair,
            &bob_pubkey,
            &date_pubkey,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 24);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 12);

        // Update the date oracle and redeem more tokens
        store_date(
            &bank_client,
            &date_keypair,
            &alice_keypair,
            Utc.ymd(2019, 2, 1),
        )
        .unwrap();

        // Force a new blockhash so that there's not a duplicate signature.
        for _ in 0..bank.ticks_per_slot() {
            bank.register_tick(&hash(&[1]));
        }

        send_redeem_tokens(
            &bank_client,
            &contract_pubkey,
            &alice_keypair,
            &bob_pubkey,
            &date_pubkey,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 23);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 13);
    }

    #[test]
    fn test_terminate_and_refund() {
        let (bank_client, alice_keypair) = create_bank_client(3);
        let alice_pubkey = alice_keypair.pubkey();
        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let bob_pubkey = Pubkey::new_rand();
        let start_date = Utc::now().date();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_date = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &date_keypair, &alice_keypair, current_date).unwrap();

        create_vest_account(
            &bank_client,
            &contract_keypair,
            &alice_keypair,
            &alice_pubkey,
            &bob_pubkey,
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
            vest_instruction::terminate(&contract_pubkey, &alice_pubkey, &alice_pubkey);
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

    #[test]
    fn test_terminate_and_send_funds() {
        let (bank_client, alice_keypair) = create_bank_client(3);
        let alice_pubkey = alice_keypair.pubkey();
        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let bob_pubkey = Pubkey::new_rand();
        let start_date = Utc::now().date();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_date = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &date_keypair, &alice_keypair, current_date).unwrap();

        create_vest_account(
            &bank_client,
            &contract_keypair,
            &alice_keypair,
            &alice_pubkey,
            &bob_pubkey,
            start_date,
            &date_pubkey,
            1,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 1);

        // Now, terminate the transaction. carol gets the funds.
        let carol_pubkey = Pubkey::new_rand();
        let instruction =
            vest_instruction::terminate(&contract_pubkey, &alice_pubkey, &carol_pubkey);
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&carol_pubkey).unwrap(), 1);
        assert_eq!(
            bank_client.get_account_data(&contract_pubkey).unwrap(),
            None
        );
        assert_eq!(bank_client.get_account_data(&bob_pubkey).unwrap(), None);
    }

    #[test]
    fn test_renege_and_send_funds() {
        let (bank_client, alice_keypair) = create_bank_client(3);
        let alice_pubkey = alice_keypair.pubkey();
        let contract_keypair = Keypair::new();
        let contract_pubkey = contract_keypair.pubkey();
        let bob_pubkey = Pubkey::new_rand();
        let start_date = Utc::now().date();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_date = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &date_keypair, &alice_keypair, current_date).unwrap();

        create_vest_account(
            &bank_client,
            &contract_keypair,
            &alice_keypair,
            &alice_pubkey,
            &bob_pubkey,
            start_date,
            &date_pubkey,
            1,
        )
        .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&contract_pubkey).unwrap(), 1);

        // Now, renege on a token. carol gets it.
        let carol_pubkey = Pubkey::new_rand();
        let instruction =
            vest_instruction::renege(&contract_pubkey, &alice_pubkey, &carol_pubkey, 1);
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&carol_pubkey).unwrap(), 1);
        assert_eq!(
            bank_client.get_account_data(&contract_pubkey).unwrap(),
            None
        );
        assert_eq!(bank_client.get_account_data(&bob_pubkey).unwrap(), None);
    }
}
