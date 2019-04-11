//! budget program
use crate::budget_expr::Witness;
use crate::budget_instruction::BudgetInstruction;
use crate::budget_state::{BudgetError, BudgetState};
use bincode::{deserialize, serialize};
use chrono::prelude::{DateTime, Utc};
use log::*;
use solana_sdk::account::KeyedAccount;
use solana_sdk::instruction::InstructionError;
use solana_sdk::pubkey::Pubkey;

/// Process a Witness Signature. Any payment plans waiting on this signature
/// will progress one step.
fn apply_signature(
    budget_state: &mut BudgetState,
    keyed_accounts: &mut [KeyedAccount],
) -> Result<(), BudgetError> {
    let mut final_payment = None;
    if let Some(ref mut expr) = budget_state.pending_budget {
        let key = keyed_accounts[0].signer_key().unwrap();
        expr.apply_witness(&Witness::Signature, key);
        final_payment = expr.final_payment();
    }

    if let Some(payment) = final_payment {
        if let Some(key) = keyed_accounts[0].signer_key() {
            if &payment.to == key {
                budget_state.pending_budget = None;
                keyed_accounts[1].account.lamports -= payment.lamports;
                keyed_accounts[0].account.lamports += payment.lamports;
                return Ok(());
            }
        }
        if &payment.to != keyed_accounts[2].unsigned_key() {
            trace!("destination missing");
            return Err(BudgetError::DestinationMissing);
        }
        budget_state.pending_budget = None;
        keyed_accounts[1].account.lamports -= payment.lamports;
        keyed_accounts[2].account.lamports += payment.lamports;
    }
    Ok(())
}

/// Process a Witness Timestamp. Any payment plans waiting on this timestamp
/// will progress one step.
fn apply_timestamp(
    budget_state: &mut BudgetState,
    keyed_accounts: &mut [KeyedAccount],
    dt: DateTime<Utc>,
) -> Result<(), BudgetError> {
    // Check to see if any timelocked transactions can be completed.
    let mut final_payment = None;

    if let Some(ref mut expr) = budget_state.pending_budget {
        let key = keyed_accounts[0].signer_key().unwrap();
        expr.apply_witness(&Witness::Timestamp(dt), key);
        final_payment = expr.final_payment();
    }

    if let Some(payment) = final_payment {
        if &payment.to != keyed_accounts[2].unsigned_key() {
            trace!("destination missing");
            return Err(BudgetError::DestinationMissing);
        }
        budget_state.pending_budget = None;
        keyed_accounts[1].account.lamports -= payment.lamports;
        keyed_accounts[2].account.lamports += payment.lamports;
    }
    Ok(())
}

pub fn process_instruction(
    _program_id: &Pubkey,
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
    _tick_height: u64,
) -> Result<(), InstructionError> {
    let instruction = deserialize(data).map_err(|err| {
        info!("Invalid transaction data: {:?} {:?}", data, err);
        InstructionError::InvalidInstructionData
    })?;

    trace!("process_instruction: {:?}", instruction);

    match instruction {
        BudgetInstruction::InitializeAccount(expr) => {
            let expr = expr.clone();
            if let Some(payment) = expr.final_payment() {
                keyed_accounts[1].account.lamports = 0;
                keyed_accounts[0].account.lamports += payment.lamports;
                return Ok(());
            }
            let existing = BudgetState::deserialize(&keyed_accounts[0].account.data).ok();
            if Some(true) == existing.map(|x| x.initialized) {
                trace!("contract already exists");
                return Err(InstructionError::AccountAlreadyInitialized);
            }
            let mut budget_state = BudgetState::default();
            budget_state.pending_budget = Some(expr);
            budget_state.initialized = true;
            budget_state.serialize(&mut keyed_accounts[0].account.data)
        }
        BudgetInstruction::ApplyTimestamp(dt) => {
            let mut budget_state = BudgetState::deserialize(&keyed_accounts[1].account.data)?;
            if !budget_state.is_pending() {
                return Ok(()); // Nothing to do here.
            }
            if !budget_state.initialized {
                trace!("contract is uninitialized");
                return Err(InstructionError::UninitializedAccount);
            }
            if keyed_accounts[0].signer_key().is_none() {
                return Err(InstructionError::MissingRequiredSignature);
            }
            trace!("apply timestamp");
            apply_timestamp(&mut budget_state, keyed_accounts, dt)
                .map_err(|e| InstructionError::CustomError(serialize(&e).unwrap()))?;
            trace!("apply timestamp committed");
            budget_state.serialize(&mut keyed_accounts[1].account.data)
        }
        BudgetInstruction::ApplySignature => {
            let mut budget_state = BudgetState::deserialize(&keyed_accounts[1].account.data)?;
            if !budget_state.is_pending() {
                return Ok(()); // Nothing to do here.
            }
            if !budget_state.initialized {
                trace!("contract is uninitialized");
                return Err(InstructionError::UninitializedAccount);
            }
            if keyed_accounts[0].signer_key().is_none() {
                return Err(InstructionError::MissingRequiredSignature);
            }
            trace!("apply signature");
            apply_signature(&mut budget_state, keyed_accounts)
                .map_err(|e| InstructionError::CustomError(serialize(&e).unwrap()))?;
            trace!("apply signature committed");
            budget_state.serialize(&mut keyed_accounts[1].account.data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget_instruction;
    use crate::id;
    use solana_runtime::bank::Bank;
    use solana_runtime::bank_client::BankClient;
    use solana_sdk::client::SyncClient;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::message::Message;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::TransactionError;

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_instruction_processor(id(), process_instruction);
        (bank, mint_keypair)
    }

    #[test]
    fn test_budget_payment() {
        let (bank, alice_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(&bank);
        let alice_pubkey = alice_keypair.pubkey();
        let bob_pubkey = Pubkey::new_rand();
        let instructions = budget_instruction::payment(&alice_pubkey, &bob_pubkey, 100);
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 100);
    }

    #[test]
    fn test_unsigned_witness_key() {
        let (bank, alice_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(&bank);
        let alice_pubkey = alice_keypair.pubkey();

        // Initialize BudgetState
        let budget_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let witness = Pubkey::new_rand();
        let instructions = budget_instruction::when_signed(
            &alice_pubkey,
            &bob_pubkey,
            &budget_pubkey,
            &witness,
            None,
            1,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();

        // Attack! Part 1: Sign a witness transaction with a random key.
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();
        bank_client
            .transfer(1, &alice_keypair, &mallory_pubkey)
            .unwrap();
        let instruction =
            budget_instruction::apply_signature(&mallory_pubkey, &budget_pubkey, &bob_pubkey);
        let mut message = Message::new(vec![instruction]);

        // Attack! Part 2: Point the instruction to the expected, but unsigned, key.
        message.account_keys.push(alice_pubkey);
        message.instructions[0].accounts[0] = 3;

        // Ensure the transaction fails because of the unsigned key.
        assert_eq!(
            bank_client
                .send_message(&[&mallory_keypair], message)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
        );
    }

    #[test]
    fn test_unsigned_timestamp() {
        let (bank, alice_keypair) = create_bank(10_000);
        let bank_client = BankClient::new(&bank);
        let alice_pubkey = alice_keypair.pubkey();

        // Initialize BudgetState
        let budget_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let dt = Utc::now();
        let instructions = budget_instruction::on_date(
            &alice_pubkey,
            &bob_pubkey,
            &budget_pubkey,
            dt,
            &alice_pubkey,
            None,
            1,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();

        // Attack! Part 1: Sign a timestamp transaction with a random key.
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();
        bank_client
            .transfer(1, &alice_keypair, &mallory_pubkey)
            .unwrap();
        let instruction =
            budget_instruction::apply_timestamp(&mallory_pubkey, &budget_pubkey, &bob_pubkey, dt);
        let mut message = Message::new(vec![instruction]);

        // Attack! Part 2: Point the instruction to the expected, but unsigned, key.
        message.account_keys.push(alice_pubkey);
        message.instructions[0].accounts[0] = 3;

        // Ensure the transaction fails because of the unsigned key.
        assert_eq!(
            bank_client
                .send_message(&[&mallory_keypair], message)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(0, InstructionError::MissingRequiredSignature)
        );
    }

    #[test]
    fn test_pay_on_date() {
        let (bank, alice_keypair) = create_bank(2);
        let bank_client = BankClient::new(&bank);
        let alice_pubkey = alice_keypair.pubkey();
        let budget_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let mallory_pubkey = Pubkey::new_rand();
        let dt = Utc::now();
        let instructions = budget_instruction::on_date(
            &alice_pubkey,
            &bob_pubkey,
            &budget_pubkey,
            dt,
            &alice_pubkey,
            None,
            1,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&budget_pubkey).unwrap(), 1);

        let contract_account = bank_client
            .get_account_data(&budget_pubkey)
            .unwrap()
            .unwrap();
        let budget_state = BudgetState::deserialize(&contract_account).unwrap();
        assert!(budget_state.is_pending());

        // Attack! Try to payout to mallory_pubkey
        let instruction =
            budget_instruction::apply_timestamp(&alice_pubkey, &budget_pubkey, &mallory_pubkey, dt);
        assert_eq!(
            bank_client
                .send_instruction(&alice_keypair, instruction)
                .unwrap_err()
                .unwrap(),
            TransactionError::InstructionError(
                0,
                InstructionError::CustomError(serialize(&BudgetError::DestinationMissing).unwrap())
            )
        );
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&budget_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 0);

        let contract_account = bank_client
            .get_account_data(&budget_pubkey)
            .unwrap()
            .unwrap();
        let budget_state = BudgetState::deserialize(&contract_account).unwrap();
        assert!(budget_state.is_pending());

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        let instruction =
            budget_instruction::apply_timestamp(&alice_pubkey, &budget_pubkey, &bob_pubkey, dt);
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&budget_pubkey).unwrap(), 0);
        assert_eq!(bank_client.get_balance(&bob_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_account_data(&budget_pubkey).unwrap(), None);
    }

    #[test]
    fn test_cancel_payment() {
        let (bank, alice_keypair) = create_bank(3);
        let bank_client = BankClient::new(&bank);
        let alice_pubkey = alice_keypair.pubkey();
        let budget_pubkey = Pubkey::new_rand();
        let bob_pubkey = Pubkey::new_rand();
        let dt = Utc::now();

        let instructions = budget_instruction::on_date(
            &alice_pubkey,
            &bob_pubkey,
            &budget_pubkey,
            dt,
            &alice_pubkey,
            Some(alice_pubkey),
            1,
        );
        let message = Message::new(instructions);
        bank_client
            .send_message(&[&alice_keypair], message)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 2);
        assert_eq!(bank_client.get_balance(&budget_pubkey).unwrap(), 1);

        let contract_account = bank_client
            .get_account_data(&budget_pubkey)
            .unwrap()
            .unwrap();
        let budget_state = BudgetState::deserialize(&contract_account).unwrap();
        assert!(budget_state.is_pending());

        // Attack! try to put the lamports into the wrong account with cancel
        let mallory_keypair = Keypair::new();
        let mallory_pubkey = mallory_keypair.pubkey();
        bank_client
            .transfer(1, &alice_keypair, &mallory_pubkey)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);

        let instruction =
            budget_instruction::apply_signature(&mallory_pubkey, &budget_pubkey, &bob_pubkey);
        bank_client
            .send_instruction(&mallory_keypair, instruction)
            .unwrap();
        // nothing should be changed because apply witness didn't finalize a payment
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_balance(&budget_pubkey).unwrap(), 1);
        assert_eq!(bank_client.get_account_data(&bob_pubkey).unwrap(), None);

        // Now, cancel the transaction. mint gets her funds back
        let instruction =
            budget_instruction::apply_signature(&alice_pubkey, &budget_pubkey, &alice_pubkey);
        bank_client
            .send_instruction(&alice_keypair, instruction)
            .unwrap();
        assert_eq!(bank_client.get_balance(&alice_pubkey).unwrap(), 2);
        assert_eq!(bank_client.get_account_data(&budget_pubkey).unwrap(), None);
        assert_eq!(bank_client.get_account_data(&bob_pubkey).unwrap(), None);
    }
}
