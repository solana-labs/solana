//! system interpreter

use bincode::deserialize;
use dynamic_program::DynamicProgram;
use solana_program_interface::account::Account;
use solana_program_interface::pubkey::Pubkey;
use std::collections::HashMap;
use std::sync::RwLock;
use transaction::Transaction;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SystemProgram {
    /// Create a new account
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - new account key
    /// * tokens - number of tokens to transfer to the new account
    /// * space - memory to allocate if greater then zero
    /// * interpreter_id - the interpreter id of the new account
    CreateAccount {
        tokens: i64,
        space: u64,
        interpreter_id: Pubkey,
    },
    /// Assign account to an interpreter
    /// * Transaction::keys[0] - account to assign
    Assign { interpreter_id: Pubkey },
    /// Move tokens
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Move { tokens: i64 },
    /// Load an interpreter
    /// interpreter_id - id to associate this interpreter
    /// name - file path of the interpreter to load
    Load {
        interpreter_id: Pubkey,
        name: String,
    },
}

pub const SYSTEM_INTERPRETER_ID: [u8; 32] = [0u8; 32];

impl SystemProgram {
    pub fn check_id(interpreter_id: &Pubkey) -> bool {
        interpreter_id.as_ref() == SYSTEM_INTERPRETER_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&SYSTEM_INTERPRETER_ID)
    }
    pub fn get_balance(account: &Account) -> i64 {
        account.tokens
    }
    pub fn process_transaction(
        tx: &Transaction,
        pix: usize,
        accounts: &mut [&mut Account],
        loaded_interpreters: &RwLock<HashMap<Pubkey, DynamicProgram>>,
    ) {
        if let Ok(syscall) = deserialize(tx.userdata(pix)) {
            trace!("process_transaction: {:?}", syscall);
            match syscall {
                SystemProgram::CreateAccount {
                    tokens,
                    space,
                    interpreter_id,
                } => {
                    if !Self::check_id(&accounts[0].interpreter_id) {
                        return;
                    }
                    if space > 0
                        && (!accounts[1].userdata.is_empty()
                            || !Self::check_id(&accounts[1].interpreter_id))
                    {
                        return;
                    }
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                    accounts[1].interpreter_id = interpreter_id;
                    accounts[1].userdata = vec![0; space as usize];
                }
                SystemProgram::Assign { interpreter_id } => {
                    if !Self::check_id(&accounts[0].interpreter_id) {
                        return;
                    }
                    accounts[0].interpreter_id = interpreter_id;
                }
                SystemProgram::Move { tokens } => {
                    //bank should be verifying correctness
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                }
                SystemProgram::Load {
                    interpreter_id,
                    name,
                } => {
                    let mut hashmap = loaded_interpreters.write().unwrap();
                    hashmap.insert(interpreter_id, DynamicProgram::new_native(name));
                }
            }
        } else {
            info!("Invalid transaction userdata: {:?}", tx.userdata(pix));
        }
    }
}
#[cfg(test)]
mod test {
    use super::*;
    use hash::Hash;
    use signature::{Keypair, KeypairUtil};
    use solana_program_interface::account::Account;
    use solana_program_interface::pubkey::Pubkey;
    use std::collections::HashMap;
    use std::sync::RwLock;
    use system_interpreter::SystemProgram;
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    fn process_transaction(
        tx: &Transaction,
        accounts: &mut [Account],
        loaded_interpreters: &RwLock<HashMap<Pubkey, DynamicProgram>>,
    ) {
        let mut refs: Vec<&mut Account> = accounts.iter_mut().collect();
        SystemProgram::process_transaction(&tx, 0, &mut refs[..], loaded_interpreters)
    }
    #[test]
    fn test_create_noop() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        let tx = Transaction::system_new(&from, to.pubkey(), 0, Hash::default());
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
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
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }
    #[test]
    fn test_create_spend_wrong_source() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        accounts[0].interpreter_id = from.pubkey();
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
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
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
        assert!(accounts[0].userdata.is_empty());
        assert_eq!(accounts[1].userdata.len(), 1);
        assert_eq!(accounts[1].interpreter_id, to.pubkey());
    }
    #[test]
    fn test_create_allocate_wrong_dest_interpreter() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[1].interpreter_id = to.pubkey();
        let tx = Transaction::system_create(
            &from,
            to.pubkey(),
            Hash::default(),
            0,
            1,
            Pubkey::default(),
            0,
        );
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
        assert!(accounts[1].userdata.is_empty());
    }
    #[test]
    fn test_create_allocate_wrong_source_interpreter() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].interpreter_id = to.pubkey();
        let tx = Transaction::system_create(
            &from,
            to.pubkey(),
            Hash::default(),
            0,
            1,
            Pubkey::default(),
            0,
        );
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
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
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[1].userdata.len(), 3);
    }
    #[test]
    fn test_create_assign() {
        let from = Keypair::new();
        let interpreter = Keypair::new();
        let mut accounts = vec![Account::default()];
        let tx = Transaction::system_assign(&from, Hash::default(), interpreter.pubkey(), 0);
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[0].interpreter_id, interpreter.pubkey());
    }
    #[test]
    fn test_move() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        let hash = RwLock::new(HashMap::new());
        process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }
    /// Detect binary changes in the serialized interpreter userdata, which could have a downstream
    /// affect on SDKs and DApps
    #[test]
    fn test_sdk_serialize() {
        let keypair = Keypair::new();
        use budget_program::BUDGET_INTERPRETER_ID;

        // CreateAccount
        let tx = Transaction::system_create(
            &keypair,
            keypair.pubkey(),
            Hash::default(),
            111,
            222,
            Pubkey::new(&BUDGET_INTERPRETER_ID),
            0,
        );

        assert_eq!(
            tx.userdata(0).to_vec(),
            vec![
                0, 0, 0, 0, 111, 0, 0, 0, 0, 0, 0, 0, 222, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0,
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
        let tx = Transaction::system_assign(
            &keypair,
            Hash::default(),
            Pubkey::new(&BUDGET_INTERPRETER_ID),
            0,
        );
        assert_eq!(
            tx.userdata(0).to_vec(),
            vec![
                1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0
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
