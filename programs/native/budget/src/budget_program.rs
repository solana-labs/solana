//! budget program
use bincode::{self, deserialize, serialize_into, serialized_size};
use chrono::prelude::{DateTime, Utc};
use solana_sdk::account::KeyedAccount;
use solana_sdk::budget_expr::BudgetExpr;
use solana_sdk::budget_instruction::Instruction;
use solana_sdk::payment_plan::Witness;
use std::io;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BudgetError {
    InsufficientFunds,
    ContractAlreadyExists,
    ContractNotPending,
    SourceIsPendingContract,
    UninitializedContract,
    NegativeTokens,
    DestinationMissing,
    FailedWitness,
    UserdataTooSmall,
    UserdataDeserializeFailure,
    UnsignedKey,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BudgetProgram {
    pub initialized: bool,
    pub pending_budget: Option<BudgetExpr>,
}

fn apply_debits(
    keyed_accounts: &mut [KeyedAccount],
    instruction: &Instruction,
) -> Result<(), BudgetError> {
    if !keyed_accounts[0].account.userdata.is_empty() {
        trace!("source is pending");
        return Err(BudgetError::SourceIsPendingContract);
    }
    match instruction {
        Instruction::NewBudget(expr) => {
            let expr = expr.clone();
            if let Some(payment) = expr.final_payment() {
                keyed_accounts[1].account.tokens += payment.tokens;
                Ok(())
            } else {
                let existing = BudgetProgram::deserialize(&keyed_accounts[1].account.userdata).ok();
                if Some(true) == existing.map(|x| x.initialized) {
                    trace!("contract already exists");
                    Err(BudgetError::ContractAlreadyExists)
                } else {
                    let mut program = BudgetProgram::default();
                    program.pending_budget = Some(expr);
                    keyed_accounts[1].account.tokens += keyed_accounts[0].account.tokens;
                    keyed_accounts[0].account.tokens = 0;
                    program.initialized = true;
                    program.serialize(&mut keyed_accounts[1].account.userdata)
                }
            }
        }
        Instruction::ApplyTimestamp(dt) => {
            if let Ok(mut program) = BudgetProgram::deserialize(&keyed_accounts[1].account.userdata)
            {
                if !program.is_pending() {
                    Err(BudgetError::ContractNotPending)
                } else if !program.initialized {
                    trace!("contract is uninitialized");
                    Err(BudgetError::UninitializedContract)
                } else {
                    trace!("apply timestamp");
                    program.apply_timestamp(keyed_accounts, *dt)?;
                    trace!("apply timestamp committed");
                    program.serialize(&mut keyed_accounts[1].account.userdata)
                }
            } else {
                Err(BudgetError::UninitializedContract)
            }
        }
        Instruction::ApplySignature => {
            if let Ok(mut program) = BudgetProgram::deserialize(&keyed_accounts[1].account.userdata)
            {
                if !program.is_pending() {
                    Err(BudgetError::ContractNotPending)
                } else if !program.initialized {
                    trace!("contract is uninitialized");
                    Err(BudgetError::UninitializedContract)
                } else {
                    trace!("apply signature");
                    program.apply_signature(keyed_accounts)?;
                    trace!("apply signature committed");
                    program.serialize(&mut keyed_accounts[1].account.userdata)
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

// TODO: Re-instate budget_program special case in bank.rs?
/*
pub fn get_balance(account: &Account) -> u64 {
    if let Ok(program) = deserialize(&account.userdata) {
        let program: BudgetProgram = program;
        if program.is_pending() {
            0
        } else {
            account.tokens
        }
    } else {
        account.tokens
    }
}
*/

impl BudgetProgram {
    fn is_pending(&self) -> bool {
        self.pending_budget != None
    }
    /// Process a Witness Signature. Any payment plans waiting on this signature
    /// will progress one step.
    fn apply_signature(&mut self, keyed_accounts: &mut [KeyedAccount]) -> Result<(), BudgetError> {
        let mut final_payment = None;
        if let Some(ref mut expr) = self.pending_budget {
            let key = match keyed_accounts[0].signer_key() {
                None => return Err(BudgetError::UnsignedKey),
                Some(key) => key,
            };
            expr.apply_witness(&Witness::Signature, key);
            final_payment = expr.final_payment();
        }

        if let Some(payment) = final_payment {
            if &payment.to != keyed_accounts[2].unsigned_key() {
                trace!("destination missing");
                return Err(BudgetError::DestinationMissing);
            }
            self.pending_budget = None;
            keyed_accounts[1].account.tokens -= payment.tokens;
            keyed_accounts[2].account.tokens += payment.tokens;
        }
        Ok(())
    }

    /// Process a Witness Timestamp. Any payment plans waiting on this timestamp
    /// will progress one step.
    fn apply_timestamp(
        &mut self,
        keyed_accounts: &mut [KeyedAccount],
        dt: DateTime<Utc>,
    ) -> Result<(), BudgetError> {
        // Check to see if any timelocked transactions can be completed.
        let mut final_payment = None;

        if let Some(ref mut expr) = self.pending_budget {
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
            self.pending_budget = None;
            keyed_accounts[1].account.tokens -= payment.tokens;
            keyed_accounts[2].account.tokens += payment.tokens;
        }
        Ok(())
    }

    fn serialize(&self, output: &mut [u8]) -> Result<(), BudgetError> {
        let len = serialized_size(self).unwrap() as u64;
        if output.len() < len as usize {
            warn!(
                "{} bytes required to serialize, only have {} bytes",
                len,
                output.len()
            );
            return Err(BudgetError::UserdataTooSmall);
        }
        {
            let writer = io::BufWriter::new(&mut output[..8]);
            serialize_into(writer, &len).unwrap();
        }

        {
            let writer = io::BufWriter::new(&mut output[8..8 + len as usize]);
            serialize_into(writer, self).unwrap();
        }
        Ok(())
    }

    pub fn deserialize(input: &[u8]) -> bincode::Result<Self> {
        if input.len() < 8 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        let len: u64 = deserialize(&input[..8]).unwrap();
        if len < 2 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        if input.len() < 8 + len as usize {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        deserialize(&input[8..8 + len as usize])
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bincode::serialize;
    use solana_sdk::account::Account;
    use solana_sdk::budget_program::*;
    use solana_sdk::budget_transaction::BudgetTransaction;
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
    fn test_serializer() {
        let mut a = Account::new(0, 512, id());
        let b = BudgetProgram::default();
        b.serialize(&mut a.userdata).unwrap();
        let buf = serialize(&b).unwrap();
        assert_eq!(a.userdata[8..8 + buf.len()], buf[0..]);
        let c = BudgetProgram::deserialize(&a.userdata).unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn test_serializer_userdata_too_small() {
        let mut a = Account::new(0, 1, id());
        let b = BudgetProgram::default();
        assert_eq!(
            b.serialize(&mut a.userdata),
            Err(BudgetError::UserdataTooSmall)
        );
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

        // Initialize BudgetProgram
        let from = Keypair::new();
        let contract = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();
        let witness = Keypair::new().pubkey();
        let tx = Transaction::budget_new_when_signed(
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
        let mut tx = Transaction::budget_new_signature(&rando, contract, to, Hash::default());

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

        // Initialize BudgetProgram
        let from = Keypair::new();
        let contract = Keypair::new().pubkey();
        let to = Keypair::new().pubkey();
        let dt = Utc::now();
        let tx = Transaction::budget_new_on_date(
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
        let mut tx = Transaction::budget_new_timestamp(&rando, contract, to, dt, Hash::default());

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
        let tx = Transaction::budget_new_on_date(
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
        let program = BudgetProgram::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(program.is_pending());

        // Attack! Try to payout to a rando key
        let tx = Transaction::budget_new_timestamp(
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

        let program = BudgetProgram::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(program.is_pending());

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        let tx = Transaction::budget_new_timestamp(
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

        let program = BudgetProgram::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(!program.is_pending());

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
        let tx = Transaction::budget_new_on_date(
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
        let program = BudgetProgram::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(program.is_pending());

        // Attack! try to put the tokens into the wrong account with cancel
        let tx =
            Transaction::budget_new_signature(&to, contract.pubkey(), to.pubkey(), Hash::default());
        // unit test hack, the `from account` is passed instead of the `to` account to avoid
        // creating more account vectors
        process_transaction(&tx, &mut accounts).unwrap();
        // nothing should be changed because apply witness didn't finalize a payment
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        // this would be the `to.pubkey()` account
        assert_eq!(accounts[pay_account].tokens, 0);

        // Now, cancel the transaction. from gets her funds back
        let tx = Transaction::budget_new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            Hash::default(),
        );
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[pay_account].tokens, 1);

        // try to replay the signature contract
        let tx = Transaction::budget_new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            Hash::default(),
        );
        assert_eq!(
            process_transaction(&tx, &mut accounts),
            Err(BudgetError::ContractNotPending)
        );
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[pay_account].tokens, 1);
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
        let tx = Transaction::budget_new_on_date(
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
        assert!(BudgetProgram::deserialize(&accounts[1].userdata).is_err());

        let tx = Transaction::budget_new_timestamp(
            &from,
            contract.pubkey(),
            to.pubkey(),
            Utc::now(),
            Hash::default(),
        );
        assert!(process_transaction(&tx, &mut accounts).is_err());
        assert!(BudgetProgram::deserialize(&accounts[1].userdata).is_err());

        // Success if there was no panic...
    }
}
