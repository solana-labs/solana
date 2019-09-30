//! vest program
use crate::date_instruction::DateConfig;
use crate::{
    vest_instruction::{VestError, VestInstruction},
    vest_state::VestState,
};
use bincode::deserialize;
use chrono::prelude::*;
use solana_config_api::get_config_data;
use solana_sdk::{account::KeyedAccount, instruction::InstructionError, pubkey::Pubkey};

/// Return the date that is 'n' months from 'start'.
fn get_month(start: Date<Utc>, n: u32) -> Date<Utc> {
    let year = start.year() + (start.month0() + n) as i32 / 12;
    let month0 = (start.month0() + n) % 12;

    // For those that started on the 31st, pay out on the latest day of the month.
    let mut dt = None;
    let mut days_back = 0;
    while dt.is_none() {
        dt = Utc
            .ymd_opt(year, month0 + 1, start.day() - days_back)
            .single();
        days_back += 1;
    }
    dt.unwrap()
}

/// Integer division that also returns the remainer.
fn div(dividend: u64, divisor: u64) -> (u64, u64) {
    (dividend / divisor, dividend % divisor)
}

// Return a list of contract messages and a list of contract_id/to_id pairs.
pub fn create_vesting_schedule(start_date: Date<Utc>, mut lamports: u64) -> Vec<(Date<Utc>, u64)> {
    let mut schedule = vec![];

    // 1/3 vest after one year from start date.
    let (mut stipend, remainder) = div(lamports, 3);
    stipend += remainder;

    let dt = get_month(start_date, 12);
    schedule.push((dt, stipend));

    lamports -= stipend;

    // Remaining 66% vest monthly after one year.
    let payments = 24u32;
    let (stipend, remainder) = div(lamports, u64::from(payments));
    for n in 0..payments {
        let mut stipend = stipend;
        if u64::from(n) < remainder {
            stipend += 1;
        }
        let dt = get_month(start_date, n + 13);
        schedule.push((dt, stipend));
        lamports -= stipend;
    }
    assert_eq!(lamports, 0);

    schedule
}

/// Process an AccountData Witness and any payment waiting on it.
fn redeem_tokens(
    vest_state: &mut VestState,
    keyed_accounts: &mut [KeyedAccount],
) -> Result<(), VestError> {
    let oracle_keyed_account = &keyed_accounts[0];
    if oracle_keyed_account.account.owner != solana_config_api::id() {
        return Err(VestError::UnexpectedProgramId);
    }

    if *oracle_keyed_account.unsigned_key() != vest_state.oracle_pubkey {
        return Err(VestError::Unauthorized);
    }

    if &vest_state.payee_pubkey != keyed_accounts[2].unsigned_key() {
        return Err(VestError::DestinationMissing);
    }

    let date_config: DateConfig =
        deserialize(get_config_data(&oracle_keyed_account.account.data).unwrap()).unwrap();

    let schedule = create_vesting_schedule(vest_state.start_dt.date(), vest_state.lamports);

    let vested_lamports = schedule
        .into_iter()
        .take_while(|(dt, _)| *dt <= date_config.dt.date())
        .map(|(_, lamports)| lamports)
        .sum::<u64>();

    let redeemable_lamports = vested_lamports.saturating_sub(vest_state.redeemed_lamports);

    keyed_accounts[1].account.lamports -= redeemable_lamports;
    keyed_accounts[2].account.lamports += redeemable_lamports;

    vest_state.redeemed_lamports += redeemable_lamports;

    Ok(())
}

/// Terminate the contract and return all tokens to the given pubkey.
fn terminate(
    vest_state: &mut VestState,
    keyed_accounts: &mut [KeyedAccount],
) -> Result<(), VestError> {
    let signer_key = keyed_accounts[0].signer_key().unwrap();
    if &vest_state.terminator_pubkey != signer_key {
        return Err(VestError::Unauthorized);
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
            oracle_pubkey,
            lamports,
        } => {
            let vest_state = VestState {
                terminator_pubkey,
                payee_pubkey,
                start_dt,
                oracle_pubkey,
                lamports,
                redeemed_lamports: 0,
            };
            vest_state.serialize(&mut keyed_accounts[0].account.data)
        }
        VestInstruction::RedeemTokens => {
            let mut vest_state = VestState::deserialize(&keyed_accounts[1].account.data)?;
            redeem_tokens(&mut vest_state, keyed_accounts)
                .map_err(|e| InstructionError::CustomError(e as u32))?;
            vest_state.serialize(&mut keyed_accounts[1].account.data)
        }
        VestInstruction::Terminate => {
            let mut vest_state = VestState::deserialize(&keyed_accounts[1].account.data)?;
            if keyed_accounts[0].signer_key().is_none() {
                return Err(InstructionError::MissingRequiredSignature);
            }
            terminate(&mut vest_state, keyed_accounts)
                .map_err(|e| InstructionError::CustomError(e as u32))?;
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
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};

    #[test]
    fn test_get_month() {
        let start = Utc.ymd(2018, 1, 31);
        assert_eq!(get_month(start, 0), Utc.ymd(2018, 1, 31));
        assert_eq!(get_month(start, 1), Utc.ymd(2018, 2, 28));
        assert_eq!(get_month(start, 2), Utc.ymd(2018, 3, 31));
    }

    #[test]
    fn test_create_vesting_schedule() {
        assert_eq!(
            create_vesting_schedule(Utc.ymd(2018, 1, 1), 36_000),
            vec![
                (Utc.ymd(2019, 1, 1), 12000),
                (Utc.ymd(2019, 2, 1), 1000),
                (Utc.ymd(2019, 3, 1), 1000),
                (Utc.ymd(2019, 4, 1), 1000),
                (Utc.ymd(2019, 5, 1), 1000),
                (Utc.ymd(2019, 6, 1), 1000),
                (Utc.ymd(2019, 7, 1), 1000),
                (Utc.ymd(2019, 8, 1), 1000),
                (Utc.ymd(2019, 9, 1), 1000),
                (Utc.ymd(2019, 10, 1), 1000),
                (Utc.ymd(2019, 11, 1), 1000),
                (Utc.ymd(2019, 12, 1), 1000),
                (Utc.ymd(2020, 1, 1), 1000),
                (Utc.ymd(2020, 2, 1), 1000),
                (Utc.ymd(2020, 3, 1), 1000),
                (Utc.ymd(2020, 4, 1), 1000),
                (Utc.ymd(2020, 5, 1), 1000),
                (Utc.ymd(2020, 6, 1), 1000),
                (Utc.ymd(2020, 7, 1), 1000),
                (Utc.ymd(2020, 8, 1), 1000),
                (Utc.ymd(2020, 9, 1), 1000),
                (Utc.ymd(2020, 10, 1), 1000),
                (Utc.ymd(2020, 11, 1), 1000),
                (Utc.ymd(2020, 12, 1), 1000),
                (Utc.ymd(2021, 1, 1), 1000),
            ]
        );

        // Ensure vesting date is sensible if start date was at the end of the month.
        assert_eq!(
            create_vesting_schedule(Utc.ymd(2018, 1, 31), 36_000),
            vec![
                (Utc.ymd(2019, 1, 31), 12000),
                (Utc.ymd(2019, 2, 28), 1000),
                (Utc.ymd(2019, 3, 31), 1000),
                (Utc.ymd(2019, 4, 30), 1000),
                (Utc.ymd(2019, 5, 31), 1000),
                (Utc.ymd(2019, 6, 30), 1000),
                (Utc.ymd(2019, 7, 31), 1000),
                (Utc.ymd(2019, 8, 31), 1000),
                (Utc.ymd(2019, 9, 30), 1000),
                (Utc.ymd(2019, 10, 31), 1000),
                (Utc.ymd(2019, 11, 30), 1000),
                (Utc.ymd(2019, 12, 31), 1000),
                (Utc.ymd(2020, 1, 31), 1000),
                (Utc.ymd(2020, 2, 29), 1000), // Leap year
                (Utc.ymd(2020, 3, 31), 1000),
                (Utc.ymd(2020, 4, 30), 1000),
                (Utc.ymd(2020, 5, 31), 1000),
                (Utc.ymd(2020, 6, 30), 1000),
                (Utc.ymd(2020, 7, 31), 1000),
                (Utc.ymd(2020, 8, 31), 1000),
                (Utc.ymd(2020, 9, 30), 1000),
                (Utc.ymd(2020, 10, 31), 1000),
                (Utc.ymd(2020, 11, 30), 1000),
                (Utc.ymd(2020, 12, 31), 1000),
                (Utc.ymd(2021, 1, 31), 1000),
            ]
        );

        // Awkward numbers
        assert_eq!(
            create_vesting_schedule(Utc.ymd(2018, 1, 1), 123_123),
            vec![
                (Utc.ymd(2019, 1, 1), 41041), // floor(123_123 / 3) + 123_123 % 3
                (Utc.ymd(2019, 2, 1), 3421),  // ceil(82_082 / 24)
                (Utc.ymd(2019, 3, 1), 3421),  // ceil(82_082 / 24)
                (Utc.ymd(2019, 4, 1), 3420),  // floor(82_082 / 24)
                (Utc.ymd(2019, 5, 1), 3420),
                (Utc.ymd(2019, 6, 1), 3420),
                (Utc.ymd(2019, 7, 1), 3420),
                (Utc.ymd(2019, 8, 1), 3420),
                (Utc.ymd(2019, 9, 1), 3420),
                (Utc.ymd(2019, 10, 1), 3420),
                (Utc.ymd(2019, 11, 1), 3420),
                (Utc.ymd(2019, 12, 1), 3420),
                (Utc.ymd(2020, 1, 1), 3420),
                (Utc.ymd(2020, 2, 1), 3420),
                (Utc.ymd(2020, 3, 1), 3420),
                (Utc.ymd(2020, 4, 1), 3420),
                (Utc.ymd(2020, 5, 1), 3420),
                (Utc.ymd(2020, 6, 1), 3420),
                (Utc.ymd(2020, 7, 1), 3420),
                (Utc.ymd(2020, 8, 1), 3420),
                (Utc.ymd(2020, 9, 1), 3420),
                (Utc.ymd(2020, 10, 1), 3420),
                (Utc.ymd(2020, 11, 1), 3420),
                (Utc.ymd(2020, 12, 1), 3420),
                (Utc.ymd(2021, 1, 1), 3420),
            ]
        );
    }

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

    #[test]
    fn test_redeem_tokens() {
        let (bank, alice_keypair) = create_bank(3);
        let bank_client = BankClient::new(bank);
        let alice_pubkey = alice_keypair.pubkey();

        let oracle_keypair = Keypair::new();
        let oracle_pubkey = oracle_keypair.pubkey();

        let future_dt = Utc.ymd(2019, 1, 1).and_hms(0, 0, 0);
        let mut instructions = date_instruction::create_account(&alice_pubkey, &oracle_pubkey, 1);
        instructions.push(date_instruction::store(&oracle_pubkey, future_dt));

        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair, &oracle_keypair], message)
            .unwrap();

        let vest_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let dt = Utc.ymd(2018, 1, 1).and_hms(0, 0, 0);

        let instructions = vest_instruction::create_account(
            &alice_pubkey,
            &bob_pubkey,
            &vest_pubkey,
            dt,
            &oracle_pubkey,
            1,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&vest_pubkey).unwrap(), 1);

        let instruction =
            vest_instruction::redeem_tokens(&oracle_pubkey, &vest_pubkey, &bob_pubkey);
        let message = Message::new_with_payer(vec![instruction], Some(&alice_pubkey));
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_account_data(&vest_pubkey).unwrap(), None);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 1);
    }

    #[test]
    fn test_cancel_payment() {
        let (bank, alice_keypair) = create_bank(3);
        let bank_client = BankClient::new(bank);
        let alice_pubkey = alice_keypair.pubkey();
        let vest_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let dt = Utc::now();

        let instructions = vest_instruction::create_account(
            &alice_pubkey,
            &bob_pubkey,
            &vest_pubkey,
            dt,
            &alice_pubkey,
            1,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 2);
        assert_eq!(bank_client.get_balance(&vest_pubkey).unwrap(), 1);

        // Now, terminate the transaction. alice gets her funds back
        let instruction = vest_instruction::terminate(&alice_pubkey, &vest_pubkey, &alice_pubkey);
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 3);
        assert_eq!(bank_client.get_account_data(&vest_pubkey).unwrap(), None);
        assert_eq!(bank_client.get_account_data(&bob_pubkey).unwrap(), None);
    }
}
