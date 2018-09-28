extern crate bincode;
extern crate solana;
extern crate solana_program_interface;

use std::collections::HashMap;
use std::sync::RwLock;
use std::thread;

use bincode::serialize;

use solana::dynamic_program::DynamicProgram;
use solana::hash::Hash;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_program::SystemProgram;
use solana::system_transaction::SystemTransaction;
use solana::transaction::Transaction;
use solana_program_interface::account::{Account, KeyedAccount};
use solana_program_interface::pubkey::Pubkey;

#[test]
fn test_native_noop() {
    let data: Vec<u8> = vec![0];
    let keys = vec![Pubkey::default(); 2];
    let mut accounts = vec![Account::default(), Account::default()];
    accounts[0].tokens = 100;
    accounts[1].tokens = 1;

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

        let dp = DynamicProgram::new("noop".to_string());
        dp.call(&mut infos, &data);
    }
}

#[test]
#[ignore]
fn test_native_print() {
    let data: Vec<u8> = vec![0];
    let keys = vec![Pubkey::default(); 2];
    let mut accounts = vec![Account::default(), Account::default()];
    accounts[0].tokens = 100;
    accounts[1].tokens = 1;

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

        let dp = DynamicProgram::new("print".to_string());
        dp.call(&mut infos, &data);
    }
}

#[test]
fn test_native_move_funds_success() {
    let tokens: i64 = 100;
    let data: Vec<u8> = serialize(&tokens).unwrap();
    let keys = vec![Pubkey::default(); 2];
    let mut accounts = vec![Account::default(), Account::default()];
    accounts[0].tokens = 100;
    accounts[1].tokens = 1;

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

        let dp = DynamicProgram::new("move_funds".to_string());
        dp.call(&mut infos, &data);
    }
    assert_eq!(0, accounts[0].tokens);
    assert_eq!(101, accounts[1].tokens);
}

#[test]
fn test_native_move_funds_insufficient_funds() {
    let tokens: i64 = 100;
    let data: Vec<u8> = serialize(&tokens).unwrap();
    let keys = vec![Pubkey::default(); 2];
    let mut accounts = vec![Account::default(), Account::default()];
    accounts[0].tokens = 10;
    accounts[1].tokens = 1;

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

        let dp = DynamicProgram::new("move_funds".to_string());
        dp.call(&mut infos, &data);
    }
    assert_eq!(10, accounts[0].tokens);
    assert_eq!(1, accounts[1].tokens);
}

#[test]
fn test_native_move_funds_succes_many_threads() {
    let num_threads = 42; // number of threads to spawn
    let num_iters = 100; // number of iterations of test in each thread
    let mut threads = Vec::new();
    for _t in 0..num_threads {
        threads.push(thread::spawn(move || {
            for _i in 0..num_iters {
                {
                    let tokens: i64 = 100;
                    let data: Vec<u8> = serialize(&tokens).unwrap();
                    let keys = vec![Pubkey::default(); 2];
                    let mut accounts = vec![Account::default(), Account::default()];
                    accounts[0].tokens = 100;
                    accounts[1].tokens = 1;

                    {
                        let mut infos: Vec<_> = (&keys)
                            .into_iter()
                            .zip(&mut accounts)
                            .map(|(key, account)| KeyedAccount { key, account })
                            .collect();

                        let dp = DynamicProgram::new("move_funds".to_string());
                        dp.call(&mut infos, &data);
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
fn process_transaction(
    tx: &Transaction,
    accounts: &mut [Account],
    loaded_programs: &RwLock<HashMap<Pubkey, DynamicProgram>>,
) {
    let mut refs: Vec<&mut Account> = accounts.iter_mut().collect();
    SystemProgram::process_transaction(&tx, 0, &mut refs[..], loaded_programs)
}

#[test]
fn test_system_program_load_call() {
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

        process_transaction(&tx, &mut accounts, &loaded_programs);
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
fn test_system_program_load_call_many_threads() {
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

                    process_transaction(&tx, &mut accounts, &loaded_programs);
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
