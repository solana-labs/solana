//! system program

use bank::Account;
use bincode::deserialize;
use signature::Pubkey;

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
    // TODO move this to the loader contract
    // /// Load a program
    // /// program_id - id to associate this program
    // /// nanme - file path of the program to load
    // Load { program_id: Pubkey, name: String },
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
        _keys: &[&Pubkey],
        accounts: &mut [&mut Account],
        userdata: &[u8],
    ) -> Result<(), ()> {
        if let Ok(syscall) = deserialize(&userdata) {
            trace!("process_transaction: {:?}", syscall);
            match syscall {
                SystemProgram::CreateAccount {
                    tokens,
                    space,
                    program_id,
                } => {
                    if !Self::check_id(&accounts[0].program_id) {
                        //TODO:  add system error codes
                        return Ok(());
                    }
                    if space > 0
                        && (!accounts[1].userdata.is_empty()
                            || !Self::check_id(&accounts[1].program_id))
                    {
                        //TODO:  add system error codes
                        return Ok(());
                    }
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                    accounts[1].program_id = program_id;
                    accounts[1].userdata = vec![0; space as usize];
                }
                SystemProgram::Assign { program_id } => {
                    if !Self::check_id(&accounts[0].program_id) {
                        //TODO:  add system error codes
                        return Ok(());
                    }
                    accounts[0].program_id = program_id;
                }
                SystemProgram::Move { tokens } => {
                    // TODO: when signature sets are added, we need to check that keys[0] is the signer
                    // bank should be verifying correctness
                    accounts[0].tokens -= tokens;
                    accounts[1].tokens += tokens;
                } // TODO:
                  //1. move load into its own contract
                  //2. It should operate with the default contract signature
                  //3. client creates an account with system, and assigns it to the loader contract
                  //4. client uploads the bytes
                  //5. calls "Loader::mark_executable"
                  //6. Dynamic transactions put the public key of the account used to laod the program into `Transaction::program_keys`
                  // 7.  execute_transactions asks the loader to interpret the account userdata and call
                  //SystemProgram::Load { program_id, name } => {
                  //    let mut hashmap = loaded_programs.write().unwrap();
                  //    hashmap.insert(program_id, DynamicProgram::new(name));
                  //}
            }
        } else {
            info!("Invalid transaction userdata: {:?}", userdata);
        }
        //TODO:  add system error codes
        Ok(())
    }
}
#[cfg(test)]
mod test {
    use bank::Account;
    use hash::Hash;
    use signature::{Keypair, KeypairUtil, Pubkey};
    use system_program::SystemProgram;
    use transaction::Transaction;
    fn to_refs<'a, T>(keys: &'a [T]) -> Vec<&'a T> {
        keys.iter().collect()
    }
    fn to_mut_refs<'a, T>(accounts: &'a mut [T]) -> Vec<&'a mut T> {
        accounts.iter_mut().collect()
    }
    fn process_transaction(tx: &Transaction, accounts: &mut [Account]) {
        assert!(
            SystemProgram::process_transaction(
                &to_refs(&tx.keys),
                &mut to_mut_refs(accounts),
                &tx.programs[0].userdata,
            ).is_ok()
        );
    }
    #[test]
    fn test_create_noop() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        let tx = Transaction::system_new(&from, to.pubkey(), 0, Hash::default());
        process_transaction(&tx, &mut accounts);
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
        process_transaction(&tx, &mut accounts);
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
        process_transaction(&tx, &mut accounts);
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
        process_transaction(&tx, &mut accounts);
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
        process_transaction(&tx, &mut accounts);
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
        process_transaction(&tx, &mut accounts);
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
        process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[1].userdata.len(), 3);
    }
    // TODO: move this to the laoder contract
    //#[test]
    //fn test_load_call() {
    //    // first load the program
    //    let loaded_programs = RwLock::new(HashMap::new());
    //    {
    //        let from = Keypair::new();
    //        let mut accounts = vec![Account::default(), Account::default()];
    //        let program_id = Pubkey::default(); // same program id for both
    //        let tx = Transaction::system_load(
    //            &from,
    //            Hash::default(),
    //            0,
    //            program_id,
    //            "move_funds".to_string(),
    //        );

    //        SystemProgram::process_transaction(&tx, &mut accounts, &loaded_programs);
    //    }
    //    // then call the program
    //    {
    //        let program_id = Pubkey::default(); // same program id for both
    //        let keys = vec![Pubkey::default(), Pubkey::default()];
    //        let mut accounts = vec![Account::default(), Account::default()];
    //        accounts[0].tokens = 100;
    //        accounts[1].tokens = 1;
    //        let tokens: i64 = 100;
    //        let data: Vec<u8> = serialize(&tokens).unwrap();
    //        {
    //            let hash = loaded_programs.write().unwrap();
    //            match hash.get(&program_id) {
    //                Some(dp) => {
    //                    let mut infos: Vec<_> = (&keys)
    //                        .into_iter()
    //                        .zip(&mut accounts)
    //                        .map(|(key, account)| KeyedAccount { key, account })
    //                        .collect();

    //                    dp.call(&mut infos, &data);
    //                }
    //                None => panic!("failed to find program in hash"),
    //            }
    //        }
    //        assert_eq!(0, accounts[0].tokens);
    //        assert_eq!(101, accounts[1].tokens);
    //    }
    //}
    //#[test]
    //fn test_load_call_many_threads() {
    //    let num_threads = 42;
    //    let num_iters = 100;
    //    let mut threads = Vec::new();
    //    for _t in 0..num_threads {
    //        threads.push(thread::spawn(move || {
    //            let _tid = thread::current().id();
    //            for _i in 0..num_iters {
    //                // first load the program
    //                let loaded_programs = RwLock::new(HashMap::new());
    //                {
    //                    let from = Keypair::new();
    //                    let mut accounts = vec![Account::default(), Account::default()];
    //                    let program_id = Pubkey::default(); // same program id for both
    //                    let tx = Transaction::system_load(
    //                        &from,
    //                        Hash::default(),
    //                        0,
    //                        program_id,
    //                        "move_funds".to_string(),
    //                    );

    //                    SystemProgram::process_transaction(&tx, &mut accounts, &loaded_programs);
    //                }
    //                // then call the program
    //                {
    //                    let program_id = Pubkey::default(); // same program id for both
    //                    let keys = vec![Pubkey::default(), Pubkey::default()];
    //                    let mut accounts = vec![Account::default(), Account::default()];
    //                    accounts[0].tokens = 100;
    //                    accounts[1].tokens = 1;
    //                    let tokens: i64 = 100;
    //                    let data: Vec<u8> = serialize(&tokens).unwrap();
    //                    {
    //                        let hash = loaded_programs.write().unwrap();
    //                        match hash.get(&program_id) {
    //                            Some(dp) => {
    //                                let mut infos: Vec<_> = (&keys)
    //                                    .into_iter()
    //                                    .zip(&mut accounts)
    //                                    .map(|(key, account)| KeyedAccount { key, account })
    //                                    .collect();

    //                                dp.call(&mut infos, &data);
    //                            }
    //                            None => panic!("failed to find program in hash"),
    //                        }
    //                    }
    //                    assert_eq!(0, accounts[0].tokens);
    //                    assert_eq!(101, accounts[1].tokens);
    //                }
    //            }
    //        }));
    //    }

    //    for thread in threads {
    //        thread.join().unwrap();
    //    }
    //}
    #[test]
    fn test_create_assign() {
        let from = Keypair::new();
        let program = Keypair::new();
        let mut accounts = vec![Account::default()];
        let tx = Transaction::system_assign(&from, Hash::default(), program.pubkey(), 0);
        process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[0].program_id, program.pubkey());
    }
    #[test]
    fn test_move() {
        let from = Keypair::new();
        let to = Keypair::new();
        let mut accounts = vec![Account::default(), Account::default()];
        accounts[0].tokens = 1;
        let tx = Transaction::new(&from, to.pubkey(), 1, Hash::default());
        process_transaction(&tx, &mut accounts);
        assert_eq!(accounts[0].tokens, 0);
        assert_eq!(accounts[1].tokens, 1);
    }

    /// Detect binary changes in the serialized program programs[0].userdata, which could have a downstream
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
            tx.programs[0].userdata,
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
            tx.programs[0].userdata,
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
            tx.programs[0].userdata,
            vec![
                1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0
            ]
        );

        // Move
        let tx = Transaction::system_move(&keypair, keypair.pubkey(), 123, Hash::default(), 0);
        assert_eq!(
            tx.programs[0].userdata,
            vec![2, 0, 0, 0, 123, 0, 0, 0, 0, 0, 0, 0]
        );
    }
}
