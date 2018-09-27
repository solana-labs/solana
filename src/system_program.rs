//! system program

use account::Account;
use bincode::deserialize;
use dynamic_program::DynamicProgram;
use pubkey::Pubkey;
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
    /// * program_id - the program id of the new account
    CreateAccount {
        tokens: i64,
        space: u64,
        program_id: Pubkey,
    },
    /// Assign account to a program
    /// * Transaction::keys[0] - account to assign
    Assign { program_id: Pubkey },
    /// Move tokens
    /// * Transaction::keys[0] - source
    /// * Transaction::keys[1] - destination
    Move { tokens: i64 },
    /// Load a program
    /// program_id - id to associate this program
    /// nanme - file path of the program to load
    Load { program_id: Pubkey, name: String },
}

pub const SYSTEM_PROGRAM_ID: [u8; 32] = [0u8; 32];

impl SystemProgram {
    pub fn check_id(program_id: &Pubkey) -> bool {
        program_id.as_ref() == SYSTEM_PROGRAM_ID
    }

    pub fn id() -> Pubkey {
        Pubkey::new(&SYSTEM_PROGRAM_ID)
    }
    pub fn get_balance(account: &Account) -> i64 {
        account.tokens
    }
    pub fn process_transaction(
        tx: &Transaction,
        accounts: &mut [Account],
        loaded_programs: &RwLock<HashMap<Pubkey, DynamicProgram>>,
    ) {
        if let Ok(syscall) = deserialize(&tx.userdata) {
            trace!("process_transaction: {:?}", syscall);
            match syscall {
                SystemProgram::CreateAccount {
                    tokens,
                    space,
                    program_id,
                } => {
                    if !Self::check_id(&accounts[0].program_id) {
                        return;
                    }
                    if space > 0
                        && (!accounts[1].userdata.is_empty()
                            || !Self::check_id(&accounts[1].program_id))
                    {
                        return;
                    }
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                    accounts[1].program_id = program_id;
                    accounts[1].userdata = vec![0; space as usize];
                }
                SystemProgram::Assign { program_id } => {
                    if !Self::check_id(&accounts[0].program_id) {
                        return;
                    }
                    accounts[0].program_id = program_id;
                }
                SystemProgram::Move { tokens } => {
                    //bank should be verifying correctness
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                }
                SystemProgram::Load { program_id, name } => {
                    let mut hashmap = loaded_programs.write().unwrap();
                    hashmap.insert(program_id, DynamicProgram::new(name));
                }
            }
        } else {
            info!("Invalid transaction userdata: {:?}", tx.userdata);
        }
    }
}
#[cfg(test)]
mod test {
    use account::{Account, KeyedAccount};
    use bincode::serialize;
    use hash::Hash;
    use pubkey::Pubkey;
    use signature::{Keypair, KeypairUtil};
    use std::collections::HashMap;
    use std::sync::RwLock;
    use std::thread;
    use system_program::SystemProgram;
    use system_transaction::SystemTransaction;
    use transaction::Transaction;

    #[test]
    fn test_create_noop() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        let tx = Transaction::system_new(&from, to.pubkey(), 0, Hash::default());
        let hash = RwLock::new(HashMap::new());
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
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
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }
    #[test]
    fn test_create_spend_wrong_source() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        accounts[0].program_id = from.pubkey();
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        let hash = RwLock::new(HashMap::new());
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
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
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
        assert!(accounts[0].userdata.is_empty());
        assert_eq!(accounts[1].userdata.len(), 1);
        assert_eq!(accounts[1].program_id, to.pubkey());
    }
    #[test]
    fn test_create_allocate_wrong_dest_program() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[1].program_id = to.pubkey();
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
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
        assert!(accounts[1].userdata.is_empty());
    }
    #[test]
    fn test_create_allocate_wrong_source_program() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].program_id = to.pubkey();
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
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
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
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[1].userdata.len(), 3);
    }
    #[test]
    fn test_load_call() {
        // first load the program
        let loaded_programs = RwLock::new(HashMap::new());
        {
            let from = Keypair::new();
            let mut accounts = vec![Account::default(), Account::default()];
            let program_id = Pubkey::default(); // same program id for both
            let tx = Transaction::system_load(
                &from,
                Hash::default(),
                0,
                program_id,
                "move_funds".to_string(),
            );

            SystemProgram::process_transaction(&tx, &mut accounts, &loaded_programs);
        }
        // then call the program
        {
            let program_id = Pubkey::default(); // same program id for both
            let keys = vec![Pubkey::default(), Pubkey::default()];
            let mut accounts = vec![Account::default(), Account::default()];
            accounts[0].tokens = 100;
            accounts[1].tokens = 1;
            let tokens: i64 = 100;
            let data: Vec<u8> = serialize(&tokens).unwrap();
            {
                let hash = loaded_programs.write().unwrap();
                match hash.get(&program_id) {
                    Some(dp) => {
                        let mut infos: Vec<_> = (&keys)
                            .into_iter()
                            .zip(&mut accounts)
                            .map(|(key, account)| KeyedAccount { key, account })
                            .collect();

                        dp.call(&mut infos, &data);
                    }
                    None => panic!("failed to find program in hash"),
                }
            }
            assert_eq!(0, accounts[0].tokens);
            assert_eq!(101, accounts[1].tokens);
        }
    }
    #[test]
    fn test_load_call_many_threads() {
        let num_threads = 42;
        let num_iters = 100;
        let mut threads = Vec::new();
        for _t in 0..num_threads {
            threads.push(thread::spawn(move || {
                let _tid = thread::current().id();
                for _i in 0..num_iters {
                    // first load the program
                    let loaded_programs = RwLock::new(HashMap::new());
                    {
                        let from = Keypair::new();
                        let mut accounts = vec![Account::default(), Account::default()];
                        let program_id = Pubkey::default(); // same program id for both
                        let tx = Transaction::system_load(
                            &from,
                            Hash::default(),
                            0,
                            program_id,
                            "move_funds".to_string(),
                        );

                        SystemProgram::process_transaction(&tx, &mut accounts, &loaded_programs);
                    }
                    // then call the program
                    {
                        let program_id = Pubkey::default(); // same program id for both
                        let keys = vec![Pubkey::default(), Pubkey::default()];
                        let mut accounts = vec![Account::default(), Account::default()];
                        accounts[0].tokens = 100;
                        accounts[1].tokens = 1;
                        let tokens: i64 = 100;
                        let data: Vec<u8> = serialize(&tokens).unwrap();
                        {
                            let hash = loaded_programs.write().unwrap();
                            match hash.get(&program_id) {
                                Some(dp) => {
                                    let mut infos: Vec<_> = (&keys)
                                        .into_iter()
                                        .zip(&mut accounts)
                                        .map(|(key, account)| KeyedAccount { key, account })
                                        .collect();

                                    dp.call(&mut infos, &data);
                                }
                                None => panic!("failed to find program in hash"),
                            }
                        }
                        assert_eq!(0, accounts[0].tokens);
                        assert_eq!(101, accounts[1].tokens);
                    }
                }
            }));
        }

        for thread in threads {
            thread.join().unwrap();
        }
    }
    #[test]
    fn test_create_assign() {
        let from = Keypair::new();
        let program = Keypair::new();
        let mut accounts = vec![Account::default()];
        let tx = Transaction::system_assign(&from, Hash::default(), program.pubkey(), 0);
        let hash = RwLock::new(HashMap::new());
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[0].program_id, program.pubkey());
    }
    #[test]
    fn test_move() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        let tx = Transaction::system_new(&from, to.pubkey(), 1, Hash::default());
        let hash = RwLock::new(HashMap::new());
        SystemProgram::process_transaction(&tx, &mut accounts, &hash);
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }
    /// Detect binary changes in the serialized program userdata, which could have a downstream
    /// affect on SDKs and DApps
    #[test]
    fn test_sdk_serialize() {
        let keypair = Keypair::new();
        use budget_program::BUDGET_PROGRAM_ID;

        // CreateAccount
        let tx = Transaction::system_create(
            &keypair,
            keypair.pubkey(),
            Hash::default(),
            111,
            222,
            Pubkey::new(&BUDGET_PROGRAM_ID),
            0,
        );

        assert_eq!(
            tx.userdata,
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
            tx.userdata,
            vec![
                0, 0, 0, 0, 111, 0, 0, 0, 0, 0, 0, 0, 222, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // Assign
        let tx = Transaction::system_assign(
            &keypair,
            Hash::default(),
            Pubkey::new(&BUDGET_PROGRAM_ID),
            0,
        );
        assert_eq!(
            tx.userdata,
            vec![
                1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // Move
        let tx = Transaction::system_move(&keypair, keypair.pubkey(), 123, Hash::default(), 0);
        assert_eq!(tx.userdata, vec![2, 0, 0, 0, 123, 0, 0, 0, 0, 0, 0, 0]);
    }
}
