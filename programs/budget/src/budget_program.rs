//! budget program
use bincode::{deserialize, serialize};
use chrono::prelude::{DateTime, Utc};
use log::*;
use solana_budget_api::budget_instruction::BudgetInstruction;
use solana_budget_api::budget_state::{BudgetError, BudgetState};
use solana_budget_api::payment_plan::Witness;
use solana_sdk::account::KeyedAccount;
use solana_sdk::native_program::ProgramError;
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
) -> Result<(), ProgramError> {
    let instruction = deserialize(data).map_err(|err| {
        info!("Invalid transaction data: {:?} {:?}", data, err);
        ProgramError::InvalidInstructionData
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
                return Err(ProgramError::AccountAlreadyInitialized);
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
                return Err(ProgramError::UninitializedAccount);
            }
            if keyed_accounts[0].signer_key().is_none() {
                return Err(ProgramError::MissingRequiredSignature);
            }
            trace!("apply timestamp");
            apply_timestamp(&mut budget_state, keyed_accounts, dt)
                .map_err(|e| ProgramError::CustomError(serialize(&e).unwrap()))?;
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
                return Err(ProgramError::UninitializedAccount);
            }
            if keyed_accounts[0].signer_key().is_none() {
                return Err(ProgramError::MissingRequiredSignature);
            }
            trace!("apply signature");
            apply_signature(&mut budget_state, keyed_accounts)
                .map_err(|e| ProgramError::CustomError(serialize(&e).unwrap()))?;
            trace!("apply signature committed");
            budget_state.serialize(&mut keyed_accounts[1].account.data)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_budget_api::budget_transaction::BudgetTransaction;
    use solana_budget_api::id;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::{InstructionError, Transaction, TransactionError};

    fn create_bank(lamports: u64) -> (Bank, Keypair) {
        let (genesis_block, mint_keypair) = GenesisBlock::new(lamports);
        let mut bank = Bank::new(&genesis_block);
        bank.add_entrypoint(id(), process_instruction);
        (bank, mint_keypair)
    }

    #[test]
    fn test_invalid_instruction() {
        let (bank, from) = create_bank(10_000);
        let blockhash = bank.last_blockhash();
        let contract = Keypair::new();
        let data = (1u8, 2u8, 3u8);
        let tx = Transaction::new_signed(&from, &[contract.pubkey()], &id(), &data, blockhash, 0);
        assert!(bank.process_transaction(&tx).is_err());
    }

    #[test]
    fn test_unsigned_witness_key() {
        let (bank, from) = create_bank(10_000);
        let blockhash = bank.last_blockhash();

        // Initialize BudgetState
        let contract = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();
        let witness = Keypair::new().pubkey();
        let tx =
            BudgetTransaction::new_when_signed(&from, &to, &contract, &witness, None, 1, blockhash);
        bank.process_transaction(&tx).unwrap();

        // Attack! Part 1: Sign a witness transaction with a random key.
        let rando = Keypair::new();
        bank.transfer(1, &from, &rando.pubkey(), blockhash).unwrap();
        let mut tx = BudgetTransaction::new_signature(&rando, &contract, &to, blockhash);

        // Attack! Part 2: Point the instruction to the expected, but unsigned, key.
        tx.account_keys.push(from.pubkey());
        tx.instructions[0].accounts[0] = 3;

        // Ensure the transaction fails because of the unsigned key.
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ProgramError(ProgramError::MissingRequiredSignature)
            ))
        );
    }

    #[test]
    fn test_unsigned_timestamp() {
        let (bank, from) = create_bank(10_000);
        let blockhash = bank.last_blockhash();

        // Initialize BudgetState
        let contract = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();
        let dt = Utc::now();
        let tx = BudgetTransaction::new_on_date(
            &from,
            &to,
            &contract,
            dt,
            &from.pubkey(),
            None,
            1,
            blockhash,
        );
        bank.process_transaction(&tx).unwrap();

        // Attack! Part 1: Sign a timestamp transaction with a random key.
        let rando = Keypair::new();
        bank.transfer(1, &from, &rando.pubkey(), blockhash).unwrap();
        let mut tx = BudgetTransaction::new_timestamp(&rando, &contract, &to, dt, blockhash);

        // Attack! Part 2: Point the instruction to the expected, but unsigned, key.
        tx.account_keys.push(from.pubkey());
        tx.instructions[0].accounts[0] = 3;

        // Ensure the transaction fails because of the unsigned key.
        assert_eq!(
            bank.process_transaction(&tx),
            Err(TransactionError::InstructionError(
                0,
                InstructionError::ProgramError(ProgramError::MissingRequiredSignature)
            ))
        );
    }

    #[test]
    fn test_transfer_on_date() {
        let (bank, from) = create_bank(2);
        let blockhash = bank.last_blockhash();
        let contract = Keypair::new();
        let to = Keypair::new();
        let rando = Keypair::new();
        let dt = Utc::now();
        let tx = BudgetTransaction::new_on_date(
            &from,
            &to.pubkey(),
            &contract.pubkey(),
            dt,
            &from.pubkey(),
            None,
            1,
            blockhash,
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(bank.get_balance(&from.pubkey()), 1);
        assert_eq!(bank.get_balance(&contract.pubkey()), 1);

        let contract_account = bank.get_account(&contract.pubkey()).unwrap();
        let budget_state = BudgetState::deserialize(&contract_account.data).unwrap();
        assert!(budget_state.is_pending());

        // Attack! Try to payout to a rando key
        let tx = BudgetTransaction::new_timestamp(
            &from,
            &contract.pubkey(),
            &rando.pubkey(),
            dt,
            blockhash,
        );
        assert_eq!(
            bank.process_transaction(&tx).unwrap_err(),
            TransactionError::InstructionError(
                0,
                InstructionError::ProgramError(ProgramError::CustomError(
                    serialize(&BudgetError::DestinationMissing).unwrap()
                ))
            )
        );
        assert_eq!(bank.get_balance(&from.pubkey()), 1);
        assert_eq!(bank.get_balance(&contract.pubkey()), 1);
        assert_eq!(bank.get_balance(&to.pubkey()), 0);

        let contract_account = bank.get_account(&contract.pubkey()).unwrap();
        let budget_state = BudgetState::deserialize(&contract_account.data).unwrap();
        assert!(budget_state.is_pending());

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        let tx = BudgetTransaction::new_timestamp(
            &from,
            &contract.pubkey(),
            &to.pubkey(),
            dt,
            blockhash,
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(bank.get_balance(&from.pubkey()), 1);
        assert_eq!(bank.get_balance(&contract.pubkey()), 0);
        assert_eq!(bank.get_balance(&to.pubkey()), 1);
        assert_eq!(bank.get_account(&contract.pubkey()), None);
    }

    #[test]
    fn test_cancel_transfer() {
        let (bank, from) = create_bank(3);
        let blockhash = bank.last_blockhash();
        let contract = Keypair::new();
        let to = Keypair::new();
        let dt = Utc::now();

        let tx = BudgetTransaction::new_on_date(
            &from,
            &to.pubkey(),
            &contract.pubkey(),
            dt,
            &from.pubkey(),
            Some(from.pubkey()),
            1,
            blockhash,
        );
        bank.process_transaction(&tx).unwrap();
        assert_eq!(bank.get_balance(&from.pubkey()), 2);
        assert_eq!(bank.get_balance(&contract.pubkey()), 1);

        let contract_account = bank.get_account(&contract.pubkey()).unwrap();
        let budget_state = BudgetState::deserialize(&contract_account.data).unwrap();
        assert!(budget_state.is_pending());

        // Attack! try to put the lamports into the wrong account with cancel
        let rando = Keypair::new();
        bank.transfer(1, &from, &rando.pubkey(), blockhash).unwrap();
        assert_eq!(bank.get_balance(&from.pubkey()), 1);

        let tx =
            BudgetTransaction::new_signature(&rando, &contract.pubkey(), &to.pubkey(), blockhash);
        // unit test hack, the `from account` is passed instead of the `to` account to avoid
        // creating more account vectors
        bank.process_transaction(&tx).unwrap();
        // nothing should be changed because apply witness didn't finalize a payment
        assert_eq!(bank.get_balance(&from.pubkey()), 1);
        assert_eq!(bank.get_balance(&contract.pubkey()), 1);
        assert_eq!(bank.get_account(&to.pubkey()), None);

        // Now, cancel the transaction. from gets her funds back
        let tx =
            BudgetTransaction::new_signature(&from, &contract.pubkey(), &from.pubkey(), blockhash);
        bank.process_transaction(&tx).unwrap();
        assert_eq!(bank.get_balance(&from.pubkey()), 2);
        assert_eq!(bank.get_account(&contract.pubkey()), None);
        assert_eq!(bank.get_account(&to.pubkey()), None);
    }
}
