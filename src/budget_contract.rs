//! budget contract
use bank::Account;
use bincode::{self, deserialize, serialize_into, serialized_size};
use chrono::prelude::{DateTime, Utc};
use instruction::{Instruction, Plan};
use payment_plan::{PaymentPlan, Witness};
use signature::{Pubkey, Signature};
use std::collections::hash_map::Entry::Occupied;
use std::collections::HashMap;
use std::io;
use transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BudgetError {
    InsufficientFunds(Pubkey),
    ContractAlreadyExists(Pubkey),
    ContractNotPending(Pubkey),
    SourceIsPendingContract(Pubkey),
    UninitializedContract(Pubkey),
    NegativeTokens,
    DestinationMissing(Pubkey),
    FailedWitness(Signature),
    SignatureUnoccupied(Signature),
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct BudgetContract {
    pub initialized: bool,
    pub pending: HashMap<Signature, Plan>,
    pub last_error: Option<BudgetError>,
}

pub const BUDGET_CONTRACT_ID: [u8; 32] = [
    1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];
impl BudgetContract {
    fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub fn id() -> Pubkey {
        Pubkey::new(&BUDGET_CONTRACT_ID)
    }
    pub fn check_id(contract_id: &Pubkey) -> bool {
        contract_id.as_ref() == BUDGET_CONTRACT_ID
    }

    /// Process a Witness Signature. Any payment plans waiting on this signature
    /// will progress one step.
    fn apply_signature(
        &mut self,
        tx: &Transaction,
        signature: Signature,
        account: &mut [Account],
    ) -> Result<(), BudgetError> {
        if let Occupied(mut e) = self.pending.entry(signature) {
            e.get_mut().apply_witness(&Witness::Signature, &tx.keys[0]);
            if let Some(payment) = e.get().final_payment() {
                if tx.keys.len() > 1 && payment.to == tx.keys[2] {
                    trace!("apply_witness refund");
                    //move the tokens back to the from account
                    account[1].tokens -= payment.tokens;
                    account[2].tokens += payment.tokens;
                    e.remove_entry();
                } else {
                    trace!("destination is missing");
                    return Err(BudgetError::DestinationMissing(payment.to));
                }
            } else {
                trace!("failed apply_witness");
                return Err(BudgetError::FailedWitness(signature));
            }
        } else {
            trace!("apply_witness signature unoccupied");
            return Err(BudgetError::SignatureUnoccupied(signature));
        }
        Ok(())
    }

    /// Process a Witness Timestamp. Any payment plans waiting on this timestamp
    /// will progress one step.
    fn apply_timestamp(
        &mut self,
        tx: &Transaction,
        accounts: &mut [Account],
        dt: DateTime<Utc>,
    ) -> Result<(), BudgetError> {
        // Check to see if any timelocked transactions can be completed.
        let mut completed = vec![];

        // Hold 'pending' write lock until the end of this function. Otherwise another thread can
        // double-spend if it enters before the modified plan is removed from 'pending'.
        for (key, plan) in &mut self.pending {
            plan.apply_witness(&Witness::Timestamp(dt), &tx.keys[0]);
            if let Some(payment) = plan.final_payment() {
                if tx.keys.len() < 2 || payment.to != tx.keys[2] {
                    trace!("destination missing");
                    return Err(BudgetError::DestinationMissing(payment.to));
                }
                completed.push(key.clone());
                accounts[2].tokens += payment.tokens;
                accounts[1].tokens -= payment.tokens;
            }
        }
        for key in completed {
            self.pending.remove(&key);
        }
        Ok(())
    }

    /// Deduct tokens from the source account if it has sufficient funds and the contract isn't
    /// pending
    fn apply_debits_to_budget_state(
        tx: &Transaction,
        accounts: &mut [Account],
        instruction: &Instruction,
    ) -> Result<(), BudgetError> {
        {
            // if the source account userdata is not empty, this is a pending contract
            if !accounts[0].userdata.is_empty() {
                trace!("source is pending");
                return Err(BudgetError::SourceIsPendingContract(tx.keys[0]));
            }
            if let Instruction::NewContract(contract) = &instruction {
                if contract.tokens < 0 {
                    trace!("negative tokens");
                    return Err(BudgetError::NegativeTokens);
                }

                if accounts[0].tokens < contract.tokens {
                    trace!("insufficient funds");
                    return Err(BudgetError::InsufficientFunds(tx.keys[0]));
                } else {
                    accounts[0].tokens -= contract.tokens;
                }
            };
        }
        Ok(())
    }

    /// Apply only a transaction's credits.
    /// Note: It is safe to apply credits from multiple transactions in parallel.
    fn apply_credits_to_budget_state(
        tx: &Transaction,
        accounts: &mut [Account],
        instruction: &Instruction,
    ) -> Result<(), BudgetError> {
        match instruction {
            Instruction::NewContract(contract) => {
                let plan = contract.plan.clone();
                if let Some(payment) = plan.final_payment() {
                    accounts[1].tokens += payment.tokens;
                    Ok(())
                } else {
                    let existing = Self::deserialize(&accounts[1].userdata).ok();
                    if Some(true) == existing.map(|x| x.initialized) {
                        trace!("contract already exists");
                        Err(BudgetError::ContractAlreadyExists(tx.keys[1]))
                    } else {
                        let mut state = BudgetContract::default();
                        state.pending.insert(tx.signature, plan);
                        accounts[1].tokens += contract.tokens;
                        state.initialized = true;
                        state.serialize(&mut accounts[1].userdata);
                        Ok(())
                    }
                }
            }
            Instruction::ApplyTimestamp(dt) => {
                let mut state = Self::deserialize(&accounts[1].userdata).unwrap();
                if !state.is_pending() {
                    return Err(BudgetError::ContractNotPending(tx.keys[1]));
                }
                if !state.initialized {
                    trace!("contract is uninitialized");
                    Err(BudgetError::UninitializedContract(tx.keys[1]))
                } else {
                    state.apply_timestamp(tx, accounts, *dt)?;
                    trace!("apply timestamp committed");
                    state.serialize(&mut accounts[1].userdata);
                    Ok(())
                }
            }
            Instruction::ApplySignature(signature) => {
                let mut state = Self::deserialize(&accounts[1].userdata).unwrap();
                if !state.is_pending() {
                    return Err(BudgetError::ContractNotPending(tx.keys[1]));
                }
                if !state.initialized {
                    trace!("contract is uninitialized");
                    Err(BudgetError::UninitializedContract(tx.keys[1]))
                } else {
                    trace!("apply signature");
                    state.apply_signature(tx, *signature, accounts)?;
                    trace!("apply signature committed");
                    state.serialize(&mut accounts[1].userdata);
                    Ok(())
                }
            }
            Instruction::NewVote(_vote) => {
                // TODO: move vote instruction into a different contract
                trace!("GOT VOTE! last_id={}", tx.last_id);
                Ok(())
            }
        }
    }
    fn serialize(&self, output: &mut [u8]) {
        let len = serialized_size(self).unwrap() as u64;
        {
            let writer = io::BufWriter::new(&mut output[..8]);
            serialize_into(writer, &len).unwrap();
        }
        {
            let writer = io::BufWriter::new(&mut output[8..8 + len as usize]);
            serialize_into(writer, self).unwrap();
        }
    }

    pub fn deserialize(input: &[u8]) -> bincode::Result<Self> {
        if input.len() < 8 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        let len: u64 = deserialize(&input[..8]).unwrap();
        if len < 8 {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        if input.len() < 8 + len as usize {
            return Err(Box::new(bincode::ErrorKind::SizeLimit));
        }
        deserialize(&input[8..8 + len as usize])
    }

    fn save_error_to_budget_state(e: BudgetError, accounts: &mut [Account]) -> () {
        if let Ok(mut state) = BudgetContract::deserialize(&accounts[1].userdata) {
            trace!("saved error {:?}", e);
            state.last_error = Some(e);
            state.serialize(&mut accounts[1].userdata);
        } else {
            trace!("error in uninitialized contract {:?}", e,);
        }
    }

    /// Budget DSL contract interface
    /// * tx - the transaction
    /// * accounts[0] - The source of the tokens
    /// * accounts[1] - The contract context.  Once the contract has been completed, the tokens can
    /// be spent from this account .
    pub fn process_transaction(tx: &Transaction, accounts: &mut [Account]) -> () {
        let instruction = deserialize(&tx.userdata).unwrap();
        let _ = Self::apply_debits_to_budget_state(tx, accounts, &instruction)
            .and_then(|_| Self::apply_credits_to_budget_state(tx, accounts, &instruction))
            .map_err(|e| {
                trace!("saving error {:?}", e);
                Self::save_error_to_budget_state(e, accounts);
            });
    }

    //TODO the contract needs to provide a "get_balance" introspection call of the userdata
    pub fn get_balance(account: &Account) -> i64 {
        if let Ok(pending) = deserialize(&account.userdata) {
            let pending: BudgetContract = pending;
            if pending.is_pending() {
                0
            } else {
                account.tokens
            }
        } else {
            account.tokens
        }
    }
}
#[cfg(test)]
mod test {
    use bank::Account;
    use bincode::serialize;
    use budget_contract::{BudgetContract, BudgetError};
    use chrono::prelude::Utc;
    use hash::Hash;
    use signature::{Keypair, KeypairUtil};
    use transaction::Transaction;
    #[test]
    fn test_serializer() {
        let mut a = Account::new(0, 512, BudgetContract::id());
        let b = BudgetContract::default();
        b.serialize(&mut a.userdata);
        let buf = serialize(&b).unwrap();
        assert_eq!(a.userdata[8..8 + buf.len()], buf[0..]);
        let c = BudgetContract::deserialize(&a.userdata).unwrap();
        assert_eq!(b, c);
    }

    #[test]
    fn test_transfer_on_date() {
        let mut accounts = vec![
            Account::new(1, 0, BudgetContract::id()),
            Account::new(0, 512, BudgetContract::id()),
            Account::new(0, 0, BudgetContract::id()),
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
            1,
            Hash::default(),
        );
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(state.last_error, None);
        assert!(state.is_pending());

        // Attack! Try to payout to a rando key
        let tx = Transaction::budget_new_timestamp(
            &from,
            contract.pubkey(),
            rando.pubkey(),
            dt,
            Hash::default(),
        );
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        assert_eq!(accounts[to_account].tokens, 0);

        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(
            state.last_error,
            Some(BudgetError::DestinationMissing(to.pubkey()))
        );
        assert!(state.is_pending());

        // Now, acknowledge the time in the condition occurred and
        // that pubkey's funds are now available.
        let tx = Transaction::budget_new_timestamp(
            &from,
            contract.pubkey(),
            to.pubkey(),
            dt,
            Hash::default(),
        );
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[to_account].tokens, 1);

        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert!(!state.is_pending());

        // try to replay the timestamp contract
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[to_account].tokens, 1);
        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(
            state.last_error,
            Some(BudgetError::ContractNotPending(contract.pubkey()))
        );
    }
    #[test]
    fn test_cancel_transfer() {
        let mut accounts = vec![
            Account::new(1, 0, BudgetContract::id()),
            Account::new(0, 512, BudgetContract::id()),
            Account::new(0, 0, BudgetContract::id()),
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
            1,
            Hash::default(),
        );
        let sig = tx.signature;
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(state.last_error, None);
        assert!(state.is_pending());

        // Attack! try to put the tokens into the wrong account with cancel
        let tx = Transaction::budget_new_signature(
            &to,
            contract.pubkey(),
            to.pubkey(),
            sig,
            Hash::default(),
        );
        // unit test hack, the `from account` is passed instead of the `to` account to avoid
        // creating more account vectors
        BudgetContract::process_transaction(&tx, &mut accounts);
        // nothing should be changed because apply witness didn't finalize a payment
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        // this would be the `to.pubkey()` account
        assert_eq!(accounts[pay_account].tokens, 0);
        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(state.last_error, Some(BudgetError::FailedWitness(sig)));

        // Attack!  try canceling with a bad signature
        let badsig = tx.signature;
        let tx = Transaction::budget_new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            badsig,
            Hash::default(),
        );
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 1);
        assert_eq!(accounts[pay_account].tokens, 0);
        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(
            state.last_error,
            Some(BudgetError::SignatureUnoccupied(badsig))
        );

        // Now, cancel the transaction. from gets her funds back
        let tx = Transaction::budget_new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            sig,
            Hash::default(),
        );
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[pay_account].tokens, 1);

        // try to replay the signature contract
        let tx = Transaction::budget_new_signature(
            &from,
            contract.pubkey(),
            from.pubkey(),
            sig,
            Hash::default(),
        );
        BudgetContract::process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[from_account].tokens, 0);
        assert_eq!(accounts[contract_account].tokens, 0);
        assert_eq!(accounts[pay_account].tokens, 1);

        let state = BudgetContract::deserialize(&accounts[contract_account].userdata).unwrap();
        assert_eq!(
            state.last_error,
            Some(BudgetError::ContractNotPending(contract.pubkey()))
        );
    }
}
