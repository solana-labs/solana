//! vest program
use crate::date_instruction::DateConfig;
use crate::{
    vest_instruction::{VestError, VestInstruction},
    vest_schedule::create_vesting_schedule,
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
    keyed_account: &KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<Date<Utc>, InstructionError> {
    if keyed_account.account.owner != solana_config_api::id() {
        return Err(InstructionError::IncorrectProgramId);
    }

    if *keyed_account.unsigned_key() != *expected_pubkey {
        return Err(VestError::Unauthorized.into());
    }

    let config_data = get_config_data(&keyed_account.account.data).unwrap();
    let date_config =
        deserialize::<DateConfig>(config_data).map_err(|_| InstructionError::InvalidAccountData)?;

    Ok(date_config.dt.date())
}

fn parse_payee_account<'a>(
    keyed_account: &'a mut KeyedAccount,
    expected_pubkey: &Pubkey,
) -> Result<&'a mut Account, InstructionError> {
    if *keyed_account.unsigned_key() != *expected_pubkey {
        return Err(VestError::DestinationMissing.into());
    }

    Ok(keyed_account.account)
}

/// Redeem vested tokens.
fn redeem_tokens(
    vest_state: &mut VestState,
    keyed_accounts: &mut [KeyedAccount],
) -> Result<(), InstructionError> {
    let (current_dt, payer_account, payee_account) = match keyed_accounts {
        [a0, a1, a2] => (
            parse_date_account(&a0, &vest_state.date_pubkey)?,
            &mut a1.account,
            parse_payee_account(a2, &vest_state.payee_pubkey)?,
        ),
        _ => return Err(InstructionError::InvalidArgument),
    };

    let schedule = create_vesting_schedule(vest_state.start_dt.date(), vest_state.lamports);

    let vested_lamports = schedule
        .into_iter()
        .take_while(|(dt, _)| *dt <= current_dt)
        .map(|(_, lamports)| lamports)
        .sum::<u64>();

    let redeemable_lamports = vested_lamports.saturating_sub(vest_state.redeemed_lamports);

    payer_account.lamports -= redeemable_lamports;
    payee_account.lamports += redeemable_lamports;

    vest_state.redeemed_lamports += redeemable_lamports;

    Ok(())
}

/// Terminate the contract and return all tokens to the given pubkey.
fn terminate(
    vest_state: &mut VestState,
    keyed_accounts: &mut [KeyedAccount],
) -> Result<(), InstructionError> {
    if keyed_accounts[0].signer_key().is_none() {
        return Err(InstructionError::MissingRequiredSignature);
    }
    let signer_key = keyed_accounts[0].signer_key().unwrap();
    if &vest_state.terminator_pubkey != signer_key {
        return Err(VestError::Unauthorized.into());
    }

    // If only 2 accounts provided, send tokens to the signer.
    let i = if keyed_accounts.len() < 3 { 0 } else { 2 };
    keyed_accounts[i].account.lamports += keyed_accounts[1].account.lamports;
    keyed_accounts[1].account.lamports = 0;
    Ok(())
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
            start_dt,
            date_pubkey,
            lamports,
        } => {
            let vest_state = VestState {
                terminator_pubkey,
                payee_pubkey,
                start_dt,
                date_pubkey,
                lamports,
                redeemed_lamports: 0,
            };
            vest_state.serialize(&mut keyed_accounts[0].account.data)
        }
        VestInstruction::RedeemTokens => {
            let mut vest_state = VestState::deserialize(&keyed_accounts[1].account.data)?;
            redeem_tokens(&mut vest_state, keyed_accounts)?;
            vest_state.serialize(&mut keyed_accounts[1].account.data)
        }
        VestInstruction::Terminate => {
            let mut vest_state = VestState::deserialize(&keyed_accounts[1].account.data)?;
            terminate(&mut vest_state, keyed_accounts)?;
            vest_state.serialize(&mut keyed_accounts[1].account.data)
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
        start_dt: Date<Utc>,
        date_pubkey: &Pubkey,
        lamports: u64,
    ) -> Result<Signature, TransportError> {
        let instructions = vest_instruction::create_account(
            &payer_keypair.pubkey(),
            &payee_pubkey,
            &contract_pubkey,
            start_dt,
            &date_pubkey,
            lamports,
        );
        let message = Message::new(instructions);
        bank_client.send_message(&[&payer_keypair], message)
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
    fn test_redeem_tokens() {
        let (bank, alice_keypair) = create_bank(38);
        let bank = Arc::new(bank);
        let bank_client = BankClient::new_shared(&bank);
        let alice_pubkey = alice_keypair.pubkey();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_dt = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &alice_keypair, &date_keypair, current_dt).unwrap();

        let contract_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let start_dt = Utc.ymd(2018, 1, 1);

        create_vest_account(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            start_dt,
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
        let start_dt = Utc::now().date();

        let date_keypair = Keypair::new();
        let date_pubkey = date_keypair.pubkey();

        let current_dt = Utc.ymd(2019, 1, 1);
        create_date_account(&bank_client, &alice_keypair, &date_keypair, current_dt).unwrap();

        create_vest_account(
            &bank_client,
            &alice_keypair,
            &bob_pubkey,
            &contract_pubkey,
            start_dt,
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
