//! budget program
use crate::budget_state::{BudgetError, BudgetState};
use bincode::deserialize;
use chrono::prelude::{DateTime, Utc};
use log::*;
use solana_budget_api::budget_instruction::BudgetInstruction;
use solana_budget_api::payment_plan::Witness;
use solana_sdk::account::KeyedAccount;

/// Process a Witness Signature. Any payment plans waiting on this signature
/// will progress one step.
fn apply_signature(
    budget_state: &mut BudgetState,
    keyed_accounts: &mut [KeyedAccount],
) -> Result<(), BudgetError> {
    let mut final_payment = None;
    if let Some(ref mut expr) = budget_state.pending_budget {
        let key = match keyed_accounts[0].signer_key() {
            None => return Err(BudgetError::UnsignedKey),
            Some(key) => key,
        };
        expr.apply_witness(&Witness::Signature, key);
        final_payment = expr.final_payment();
    }

    if let Some(payment) = final_payment {
        if let Some(key) = keyed_accounts[0].signer_key() {
            if &payment.to == key {
                budget_state.pending_budget = None;
                keyed_accounts[1].account.tokens -= payment.tokens;
                keyed_accounts[0].account.tokens += payment.tokens;
                return Ok(());
            }
        }
        if &payment.to != keyed_accounts[2].unsigned_key() {
            trace!("destination missing");
            return Err(BudgetError::DestinationMissing);
        }
        budget_state.pending_budget = None;
        keyed_accounts[1].account.tokens -= payment.tokens;
        keyed_accounts[2].account.tokens += payment.tokens;
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
        let key = match keyed_accounts[0].signer_key() {
            None => return Err(BudgetError::UnsignedKey),
            Some(key) => key,
        };
        expr.apply_witness(&Witness::Timestamp(dt), key);
        final_payment = expr.final_payment();
    }

    if let Some(payment) = final_payment {
        if &payment.to != keyed_accounts[2].unsigned_key() {
            trace!("destination missing");
            return Err(BudgetError::DestinationMissing);
        }
        budget_state.pending_budget = None;
        keyed_accounts[1].account.tokens -= payment.tokens;
        keyed_accounts[2].account.tokens += payment.tokens;
    }
    Ok(())
}

fn apply_debits(
    keyed_accounts: &mut [KeyedAccount],
    instruction: &BudgetInstruction,
) -> Result<(), BudgetError> {
    if !keyed_accounts[0].account.userdata.is_empty() {
        trace!("source is pending");
        return Err(BudgetError::SourceIsPendingContract);
    }
    match instruction {
        BudgetInstruction::InitializeAccount(expr) => {
            let expr = expr.clone();
            if let Some(payment) = expr.final_payment() {
                keyed_accounts[1].account.tokens += payment.tokens;
                Ok(())
            } else {
                let existing = BudgetState::deserialize(&keyed_accounts[1].account.userdata).ok();
                if Some(true) == existing.map(|x| x.initialized) {
                    trace!("contract already exists");
                    Err(BudgetError::ContractAlreadyExists)
                } else {
                    let mut budget_state = BudgetState::default();
                    budget_state.pending_budget = Some(expr);
                    keyed_accounts[1].account.tokens += keyed_accounts[0].account.tokens;
                    keyed_accounts[0].account.tokens = 0;
                    budget_state.initialized = true;
                    budget_state.serialize(&mut keyed_accounts[1].account.userdata)
                }
            }
        }
        BudgetInstruction::ApplyTimestamp(dt) => {
            if let Ok(mut budget_state) =
                BudgetState::deserialize(&keyed_accounts[1].account.userdata)
            {
                if !budget_state.is_pending() {
                    Err(BudgetError::ContractNotPending)
                } else if !budget_state.initialized {
                    trace!("contract is uninitialized");
                    Err(BudgetError::UninitializedContract)
                } else {
                    trace!("apply timestamp");
                    apply_timestamp(&mut budget_state, keyed_accounts, *dt)?;
                    trace!("apply timestamp committed");
                    budget_state.serialize(&mut keyed_accounts[1].account.userdata)
                }
            } else {
                Err(BudgetError::UninitializedContract)
            }
        }
        BudgetInstruction::ApplySignature => {
            if let Ok(mut budget_state) =
                BudgetState::deserialize(&keyed_accounts[1].account.userdata)
            {
                if !budget_state.is_pending() {
                    Err(BudgetError::ContractNotPending)
                } else if !budget_state.initialized {
                    trace!("contract is uninitialized");
                    Err(BudgetError::UninitializedContract)
                } else {
                    trace!("apply signature");
                    apply_signature(&mut budget_state, keyed_accounts)?;
                    trace!("apply signature committed");
                    budget_state.serialize(&mut keyed_accounts[1].account.userdata)
                }
            } else {
                Err(BudgetError::UninitializedContract)
            }
        }
    }
}

/// Budget DSL contract interface
/// * accounts[0] - The source of the tokens
/// * accounts[1] - The contract context.  Once the contract has been completed, the tokens can
/// be spent from this account .
pub fn process_instruction(
    keyed_accounts: &mut [KeyedAccount],
    data: &[u8],
) -> Result<(), BudgetError> {
    if let Ok(instruction) = deserialize(data) {
        trace!("process_instruction: {:?}", instruction);
        apply_debits(keyed_accounts, &instruction)
    } else {
        info!("Invalid transaction userdata: {:?}", data);
        Err(BudgetError::UserdataDeserializeFailure)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use solana_budget_api::budget_transaction::BudgetTransaction;
    use solana_budget_api::id;
    use solana_sdk::account::Account;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::{Instruction, Transaction};

    fn process_transaction(
        tx: &Transaction,
        program_accounts: &mut [Account],
    ) -> Result<(), BudgetError> {
        assert_eq!(tx.instructions.len(), 1);
        let Instruction {
            ref accounts,
            ref userdata,
            ..
        } = tx.instructions[0];

        let mut keyed_accounts: Vec<_> = accounts
            .iter()
            .map(|&index| {
                let index = index as usize;
                let key = &tx.account_keys[index];
                (key, index < tx.signatures.len())
            })
            .zip(program_accounts.iter_mut())
            .map(|((key, is_signer), account)| KeyedAccount::new(key, is_signer, account))
            .collect();

        super::process_instruction(&mut keyed_accounts, &userdata)
    }
    #[test]
    fn test_invalid_instruction() {
        let mut accounts = vec![Account::new(1, 0, id()), Account::new(0, 512, id())];
        let from = Keypair::new();
        let contract = Keypair::new();
        let userdata = (1u8, 2u8, 3u8);
        let tx = Transaction::new(
            &from,
            &[contract.pubkey()],
            id(),
            &userdata,
            Hash::default(),
            0,
        );
        assert!(process_transaction(&tx, &mut accounts).is_err());
    }

    #[test]
    fn test_unsigned_witness_key() {
        let mut accounts = vec![
            Account::new(1, 0, id()),
            Account::new(0, 512, id()),
            Account::new(0, 0, id()),
        ];

        // Initialize BudgetState
        let from = Keypair::new();
        let contract = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();
        let witness = Keypair::new().pubkey();
        let tx = BudgetTransaction::new_when_signed(
            &from,
            to,
            contract,
            witness,
            None,
            1,
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();

        // Attack! Part 1: Sign a witness transaction with a random key.
        let rando = Keypair::new();
        let mut tx = BudgetTransaction::new_signature(&rando, contract, to, Hash::default());

        // Attack! Part 2: Point the instruction to the expected, but unsigned, key.
        tx.account_keys.push(from.pubkey());
        tx.instructions[0].accounts[0] = 3;

        // Ensure the transaction fails because of the unsigned key.
        assert_eq!(
            process_transaction(&tx, &mut accounts),
            Err(BudgetError::UnsignedKey)
        );
    }

    #[test]
    fn test_unsigned_timestamp() {
        let mut accounts = vec![
            Account::new(1, 0, id()),
            Account::new(0, 512, id()),
            Account::new(0, 0, id()),
        ];

        // Initialize BudgetState
        let from = Keypair::new();
        let contract = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();
        let dt = Utc::now();
        let tx = BudgetTransaction::new_on_date(
            &from,
            to,
            contract,
            dt,
            from.pubkey(),
            None,
            1,
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();

        // Attack! Part 1: Sign a timestamp transaction with a random key.
        let rando = Keypair::new();
        let mut tx = BudgetTransaction::new_timestamp(&rando, contract, to, dt, Hash::default());

        // Attack! Part 2: Point the instruction to the expected, but unsigned, key.
        tx.account_keys.push(from.pubkey());
        tx.instructions[0].accounts[0] = 3;

        // Ensure the transaction fails because of the unsigned key.
        assert_eq!(
            process_transaction(&tx, &mut accounts),
            Err(BudgetError::UnsignedKey)
        );
    }

    #[test]
    fn test_transfer_on_date() {
        let mut accounts = vec![
            Account::new(1, 0, id()),
            Account::new(0, 512, id()),
            Account::new(0, 0, id()),
        ];
        let from_account = 0;
        let contract_account = 1;
        let to_account = 2;
        let from = Keypair::new();
        let contract = Keypair::new();
        let to = Keypair::new();
        let rando = Keypair::new();
        let dt = Utc::now();
        let tx = BudgetTransaction::new_on_date(
            &from,
            to.pubkey(),
            contract.pubkey(),
            dt,
            from.pubkey(),
            None,
            1,
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        let budget_state = BudgetState::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(budget_state.is_pending());

        // Attack! Try to payout to a rando key
        let tx = BudgetTransaction::new_timestamp(
            &from,
            contract.pubkey(),
            rando.pubkey(),
            dt,
            Hash::default(),
        );
        assert_eq!(
            process_transaction(&tx, &mut accounts),
            Err(BudgetError::DestinationMissing)
        );
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        assert_eq!(accounts[to_account].tokens, 0);

        let budget_state = BudgetState::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(budget_state.is_pending());

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        let tx = BudgetTransaction::new_timestamp(
            &from,
            contract.pubkey(),
            to.pubkey(),
            dt,
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[to_account].tokens, 1);

        let budget_state = BudgetState::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(!budget_state.is_pending());

        // try to replay the timestamp contract
        assert_eq!(
            process_transaction(&tx, &mut accounts),
            Err(BudgetError::ContractNotPending)
        );
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[to_account].tokens, 1);
    }
    #[test]
    fn test_cancel_transfer() {
        let mut accounts = vec![
            Account::new(1, 0, id()),
            Account::new(0, 512, id()),
            Account::new(0, 0, id()),
        ];
        let from_account = 0;
        let contract_account = 1;
        let pay_account = 2;
        let from = Keypair::new();
        let contract = Keypair::new();
        let to = Keypair::new();
        let dt = Utc::now();
        let tx = BudgetTransaction::new_on_date(
            &from,
            to.pubkey(),
            contract.pubkey(),
            dt,
            from.pubkey(),
            Some(from.pubkey()),
            1,
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        let budget_state = BudgetState::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(budget_state.is_pending());

        // Attack! try to put the tokens into the wrong account with cancel
        let tx =
            BudgetTransaction::new_signature(&to, contract.pubkey(), to.pubkey(), Hash::default());
        // unit test hack, the `from account` is passed instead of the `to` account to avoid
        // creating more account vectors
        process_transaction(&tx, &mut accounts).unwrap();
        // nothing should be changed because apply witness didn't finalize a payment
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        // this is the `to.pubkey()` account
        assert_eq!(accounts[pay_account].tokens, 0);

        // Now, cancel the transaction. from gets her funds back
        let tx = BudgetTransaction::new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[from_account].tokens, 1);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[pay_account].tokens, 0);

        // try to replay the cancel contract
        let tx = BudgetTransaction::new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            Hash::default(),
        );
        assert_eq!(
            process_transaction(&tx, &mut accounts),
            Err(BudgetError::ContractNotPending)
        );
        assert_eq!(accounts[from_account].tokens, 1);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[pay_account].tokens, 0);
    }

    #[test]
    fn test_userdata_too_small() {
        let mut accounts = vec![
            Account::new(1, 0, id()),
            Account::new(1, 0, id()), // <== userdata is 0, which is not enough
            Account::new(1, 0, id()),
        ];
        let from = Keypair::new();
        let contract = Keypair::new();
        let to = Keypair::new();
        let tx = BudgetTransaction::new_on_date(
            &from,
            to.pubkey(),
            contract.pubkey(),
            Utc::now(),
            from.pubkey(),
            None,
            1,
            Hash::default(),
        );

        assert!(process_transaction(&tx, &mut accounts).is_err());
        assert!(BudgetState::deserialize(&accounts[1].userdata).is_err());

        let tx = BudgetTransaction::new_timestamp(
            &from,
            contract.pubkey(),
            to.pubkey(),
            Utc::now(),
            Hash::default(),
        );
        assert!(process_transaction(&tx, &mut accounts).is_err());
        assert!(BudgetState::deserialize(&accounts[1].userdata).is_err());

        // Success if there was no panic...
    }
}
