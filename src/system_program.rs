//! system program

use bincode::deserialize;
use program::ProgramError;
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_instruction::SystemInstruction;
use std;
use transaction::Transaction;

#[derive(Debug)]
pub enum Error {
    InvalidArgument,
    AssignOfUnownedAccount,
    AccountNotFinalized,
    ResultWithNegativeTokens,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "error")
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

pub fn check_id(program_id: &Pubkey) -> bool {
    program_id.as_ref() == SYSTEM_PROGRAM_ID
}

pub fn id() -> Pubkey {
    Pubkey::new(&SYSTEM_PROGRAM_ID)
}
pub fn get_balance(account: &Account) -> u64 {
    account.tokens
}
pub fn process_instruction(
    tx: &Transaction,
    pix: usize,
    accounts: &mut [&mut Account],
) -> Result<()> {
    if let Ok(syscall) = deserialize(tx.userdata(pix)) {
        trace!("process_instruction: {:?}", syscall);

        let from = tx.key_index(pix, 0).ok_or(Error::InvalidArgument)?;

        match syscall {
            SystemInstruction::CreateAccount {
                tokens,
                space,
                program_id,
            } => {
                let to = tx.key_index(pix, 1).ok_or(Error::InvalidArgument)?;

                if !check_id(&accounts[from].owner) {
                    info!("Invalid account[from] owner");
                    Err(Error::InvalidArgument)?;
                }
                if space > 0
                    && (!accounts[to].userdata.is_empty() || !check_id(&accounts[to].owner))
                {
                    info!("Invalid account[to]");
                    Err(Error::InvalidArgument)?;
                }
                if tokens > accounts[from].tokens {
                    info!(
                        "CreateAccount: insufficient tokens ({}, need {}) in account[{}] {}",
                        accounts[from].tokens, tokens, from, tx.account_keys[from]
                    );
                    Err(Error::ResultWithNegativeTokens)?;
                }
                accounts[from].tokens -= tokens;
                accounts[to].tokens += tokens;
                accounts[to].owner = program_id;
                accounts[to].userdata = vec![0; space as usize];
                accounts[to].executable = false;
                accounts[to].loader = Pubkey::default();
            }
            SystemInstruction::Assign { program_id } => {
                if !check_id(&accounts[from].owner) {
                    Err(Error::AssignOfUnownedAccount)?;
                }
                accounts[from].owner = program_id;
            }
            SystemInstruction::Move { tokens } => {
                let to = tx.key_index(pix, 1).ok_or(Error::InvalidArgument)?;

                //bank should be verifying correctness
                if tokens > accounts[from].tokens {
                    info!(
                        "Move: insufficient tokens ({}, need {}) in accounts[from] {}",
                        accounts[from].tokens, tokens, tx.account_keys[from]
                    );
                    Err(Error::ResultWithNegativeTokens)?;
                }
                accounts[from].tokens -= tokens;
                accounts[to].tokens += tokens;
            }
            SystemInstruction::Spawn => {
                if !accounts[from].executable || accounts[from].loader != Pubkey::default() {
                    Err(Error::AccountNotFinalized)?;
                }
                accounts[from].loader = accounts[from].owner;
                accounts[from].owner = tx.account_keys[from];
            }
        }
        Ok(())
    } else {
        info!("Invalid transaction userdata: {:?}", tx.userdata(pix));
        Err(Error::InvalidArgument)
    }
}

pub fn process(
    tx: &Transaction,
    instruction_index: usize,
    accounts: &mut [&mut Account],
) -> std::result::Result<(), ProgramError> {
    process_instruction(&tx, instruction_index, accounts).map_err(|err| match err {
        Error::ResultWithNegativeTokens => ProgramError::ResultWithNegativeTokens,
        _ => ProgramError::RuntimeError,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::account::Account;
    use solana_sdk::hash::Hash;
    use solana_sdk::pubkey::Pubkey;
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    fn process_transaction(tx: &Transaction, accounts: &mut [Account]) -> Result<()> {
        let mut refs: Vec<&mut Account> = accounts.iter_mut().collect();

        for (idx, _) in tx.instructions.iter().enumerate() {
            super::process_instruction(&tx, idx, &mut refs[..])?;
        }
        Ok(())
    }

    #[test]
    fn test_create_noop() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        let tx = Transaction::system_new(&from, to.pubkey(), 0, Hash::default());
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 0);
    }
    #[test]
    fn test_create_spend() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }

    #[test]
    fn test_create_spend_wrong_source() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        accounts[0].owner = from.pubkey();
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        if let Ok(()) = process_transaction(&tx, &mut accounts) {
            panic!("Account not owned by SystemProgram");
        }
        assert_eq!(accounts[0].tokens, 1);
        assert_eq!(accounts[1].tokens, 0);
    }
    #[test]
    fn test_create_assign_and_allocate() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        let tx =
            Transaction::system_create(&from, to.pubkey(), Hash::default(), 0, 1, to.pubkey(), 0);
        process_transaction(&tx, &mut accounts).unwrap();
        assert!(accounts[0].userdata.is_empty());
        assert_eq!(accounts[1].userdata.len(), 1);
        assert_eq!(accounts[1].owner, to.pubkey());
    }
    #[test]
    fn test_create_allocate_wrong_dest_program() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[1].owner = to.pubkey();
        let tx = Transaction::system_create(
            &from,
            to.pubkey(),
            Hash::default(),
            0,
            1,
            Pubkey::default(),
            0,
        );
        assert!(process_transaction(&tx, &mut accounts).is_err());
        assert!(accounts[1].userdata.is_empty());
    }
    #[test]
    fn test_create_allocate_wrong_source_program() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].owner = to.pubkey();
        let tx = Transaction::system_create(
            &from,
            to.pubkey(),
            Hash::default(),
            0,
            1,
            Pubkey::default(),
            0,
        );
        assert!(process_transaction(&tx, &mut accounts).is_err());
        assert!(accounts[1].userdata.is_empty());
    }
    #[test]
    fn test_create_allocate_already_allocated() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[1].userdata = vec![0, 0, 0];
        let tx = Transaction::system_create(
            &from,
            to.pubkey(),
            Hash::default(),
            0,
            2,
            Pubkey::default(),
            0,
        );
        assert!(process_transaction(&tx, &mut accounts).is_err());
        assert_eq!(accounts[1].userdata.len(), 3);
    }
    #[test]
    fn test_create_assign() {
        let from = Keypair::new();
        let program = Keypair::new();
        let mut accounts = vec![Account::default()];
        let tx = Transaction::system_assign(&from, Hash::default(), program.pubkey(), 0);
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[0].owner, program.pubkey());
    }
    #[test]
    fn test_move() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }

    #[test]
    fn test_move_many() {
        use logger;
        logger::setup();
        let from = Keypair::new();
        let tos = [(Keypair::new().pubkey(), 1), (Keypair::new().pubkey(), 1)];
        let mut accounts = vec![Account::default(), Account::default(), Account::default()];
        accounts[0].tokens = 2;
        let tx = Transaction::system_move_many(&from, &tos, Hash::default(), 0);
        process_transaction(&tx, &mut accounts).unwrap();
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
        assert_eq!(accounts[2].tokens, 1);
    }

    /// Detect binary changes in the serialized program userdata, which could have a downstream
    /// affect on SDKs and DApps
    #[test]
    fn test_sdk_serialize() {
        let keypair = Keypair::new();
        use budget_program;

        // CreateAccount
        let tx = Transaction::system_create(
            &keypair,
            keypair.pubkey(),
            Hash::default(),
            111,
            222,
            budget_program::id(),
            0,
        );

        assert_eq!(
            tx.userdata(0).to_vec(),
            vec![
                0, 0, 0, 0, 111, 0, 0, 0, 0, 0, 0, 0, 222, 0, 0, 0, 0, 0, 0, 0, 129, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // CreateAccount
        let tx = Transaction::system_create(
            &keypair,
            keypair.pubkey(),
            Hash::default(),
            111,
            222,
            Pubkey::default(),
            0,
        );

        assert_eq!(
            tx.userdata(0).to_vec(),
            vec![
                0, 0, 0, 0, 111, 0, 0, 0, 0, 0, 0, 0, 222, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // Assign
        let tx = Transaction::system_assign(&keypair, Hash::default(), budget_program::id(), 0);
        assert_eq!(
            tx.userdata(0).to_vec(),
            vec![
                1, 0, 0, 0, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // Move
        let tx = Transaction::system_move(&keypair, keypair.pubkey(), 123, Hash::default(), 0);
        assert_eq!(
            tx.userdata(0).to_vec(),
            vec![2, 0, 0, 0, 123, 0, 0, 0, 0, 0, 0, 0]
        );
    }
}
