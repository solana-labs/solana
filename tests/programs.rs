extern crate bincode;
extern crate elf;
extern crate solana;
extern crate solana_program_interface;

use bincode::serialize;
use solana::bank::Bank;
use solana::dynamic_instruction::DynamicInstruction;
#[cfg(feature = "bpf_c")]
use solana::dynamic_program::{
    DynamicProgram, ProgramPath, PLATFORM_SECTION_C, PLATFORM_SECTION_RS,
};
use solana::dynamic_transaction::ProgramTransaction;
#[cfg(feature = "bpf_c")]
use solana::hash::Hash;
use solana::logger;
use solana::mint::Mint;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
#[cfg(feature = "bpf_c")]
use solana::tictactoe_program::Command;
use solana::transaction::Transaction;
use solana_program_interface::account::Account;
use solana_program_interface::pubkey::Pubkey;
use std::mem;
#[cfg(feature = "bpf_c")]
use std::path::Path;
use std::thread;

fn check_tx_results(bank: &Bank, tx: &Transaction, result: Vec<solana::bank::Result<()>>) {
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], Ok(()));
    assert_eq!(bank.get_signature(&tx.last_id, &tx.signature), Some(Ok(())));
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_path_create() {
    let path = ProgramPath::Native {}.create("noop");
    assert_eq!(true, Path::new(&path).exists());
    let path = ProgramPath::Native {}.create("move_funds");
    assert_eq!(true, Path::new(&path).exists());
    let path = ProgramPath::Bpf {}.create("move_funds_c");
    assert_eq!(true, Path::new(&path).exists());
    let path = ProgramPath::Bpf {}.create("tictactoe_c");
    assert_eq!(true, Path::new(&path).exists());
}

#[test]
fn test_program_load_native() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let program = Keypair::new().pubkey();
    let state = Keypair::new().pubkey();
    let name = "noop".to_string();

    println!("program pubkey {:?}", program);
    println!("state pubkey {:?}", state);

    // allocate program bits account
    let tx = Transaction::system_create(
        &mint.keypair(),
        program,
        mint.last_id(),
        1,
        56, // How does the user know how much space to allocate for what should be an internally known size
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // populate program bits account
    let tx = Transaction::program_new_load(
        &mint.keypair(),
        program,
        DynamicInstruction::LoadNative { name },
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // allocate program state account
    let tx = Transaction::system_create(
        &mint.keypair(),
        state,
        mint.last_id(),
        1,
        10, // user known size of program state, kinda weird
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // populate program state account
    let tx = Transaction::program_new_load_state(
        &mint.keypair(),
        state,
        0,
        vec![8u8; 10],
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::program_new_call(
        &mint.keypair(),
        program,
        state,
        &[],
        vec![1u8],
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::program_new_call(
        &mint.keypair(),
        program,
        state,
        &[],
        vec![2u8], // TODO had to use '2' this time since bank complains of dup tx signature, is last_id not changing?
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // TODO need program that modifies userdata
    //assert_eq!(vec![11u8; 10], bank.get_account(&state).unwrap().userdata);
}

#[test]
fn test_program_native_move_funds() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let program = Keypair::new().pubkey();
    let state = Keypair::new().pubkey();
    let name = "move_funds".to_string();

    println!("program pubkey {:?}", program);
    println!("state pubkey {:?}", state);

    // allocate program bits account
    let tx = Transaction::system_create(
        &mint.keypair(),
        program,
        mint.last_id(),
        1,
        mem::size_of_val(&DynamicInstruction::LoadNative { name: name.clone() }) as u64,
        DynamicProgram::id(),
        0,
    );
    let results = bank.process_transactions(&vec![tx.clone()]);
    check_tx_results(&bank, &tx, results);
    assert_eq!(49, bank.get_balance(&mint.keypair().pubkey()));

    // populate program bits account
    let tx = Transaction::program_new_load(
        &mint.keypair(),
        program,
        DynamicInstruction::LoadNative { name },
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // allocate program 'to' account
    let tx = Transaction::system_create(
        &mint.keypair(),
        state,
        mint.last_id(),
        1,
        10, // user known size of program state, kinda weird
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(48, bank.get_balance(&mint.keypair().pubkey()));

    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let tx = Transaction::program_new_call(
        &mint.keypair(),
        program,
        state,
        &[],
        input,
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(38, bank.get_balance(&mint.keypair().pubkey()));
}

#[cfg(feature = "bpf_c")]
fn tictactoe_command(command: Command, accounts: &mut Vec<Account>, player: Pubkey) {
    let p = &command as *const Command as *const u8;
    let data: &[u8] = unsafe { std::slice::from_raw_parts(p, std::mem::size_of::<Command>()) };

    // allocate program bits account
    let tx = Transaction::system_create(
        &mint.keypair(),
        program,
        mint.last_id(),
        1,
        mem::size_of_val(&DynamicInstruction::LoadBpfFile { name: name.clone() }) as u64,
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(49, bank.get_balance(&mint.keypair().pubkey()));

    // populate program bits account
    let tx = Transaction::program_new_load(
        &mint.keypair(),
        program,
        DynamicInstruction::LoadBpfFile { name },
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut *accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let tx = Transaction::program_new_call(
        &mint.keypair(),
        program,
        state,
        &[],
        input,
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(38, bank.get_balance(&mint.keypair().pubkey()));
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_move_funds() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let program = Keypair::new().pubkey();
    let state = Keypair::new().pubkey();
    let name = "move_funds_c".to_string();

    println!("program pubkey {:?}", program);
    println!("state pubkey {:?}", state);

    // read program bits
    let path = ProgramPath::Bpf {}.create(&name);
    let file = match elf::File::open_path(&path) {
        Ok(f) => f,
        Err(e) => panic!("Error opening ELF {:?}: {:?}", path, e),
    };

    let text_section = match file.get_section(PLATFORM_SECTION_RS) {
        Some(s) => s,
        None => match file.get_section(PLATFORM_SECTION_C) {
            Some(s) => s,
            None => panic!("Failed to find text section"),
        },
    };
    let prog = text_section.data.clone();

    // TODO break up into smaller chunks to fit into a tx

    // allocate program bits account
    let tx = Transaction::system_create(
        &mint.keypair(),
        program,
        mint.last_id(),
        1,
        mem::size_of_val(&DynamicInstruction::LoadBpf {
            offset: 0,
            prog: prog.clone(),
        }) as u64,
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(49, bank.get_balance(&mint.keypair().pubkey()));

    // populate program bits account
    let tx = Transaction::program_new_load(
        &mint.keypair(),
        program,
        DynamicInstruction::LoadBpf { offset: 0, prog },
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // allocate program 'to' account
    let tx = Transaction::system_create(
        &mint.keypair(),
        state,
        mint.last_id(),
        1,
        10, // user known size of program state, kinda weird
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(48, bank.get_balance(&mint.keypair().pubkey()));

    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let tx = Transaction::program_new_call(
        &mint.keypair(),
        program,
        state,
        &[],
        input,
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(38, bank.get_balance(&mint.keypair().pubkey()));
}

fn process_transaction(tx: &Transaction, accounts: &mut [Account]) {
    let mut refs: Vec<&mut Account> = accounts.iter_mut().collect();
    assert_eq!(
        Ok(()),
        DynamicProgram::process_transaction(&tx, 0, &mut refs[..])
    );
}

#[test]
fn test_program_native_file_noop() {
    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let inst = DynamicInstruction::Call { input };
    let tx = Transaction::new(
        &Keypair::new(),
        &[Keypair::new().pubkey(), Keypair::new().pubkey()],
        DynamicProgram::id(),
        serialize(&inst).unwrap(),
        Hash::default(),
        0,
    );

    let dp_native = DynamicProgram::Native {
        name: "noop".to_string(),
    };
    let mut accounts = vec![Account::default(), Account::default()];
    accounts[0].tokens = 10;
    accounts[1].tokens = 1;
    accounts[1].userdata = serialize(&dp_native).unwrap();

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

        let dp = DynamicProgram::new_native("noop".to_string()).unwrap();
        assert!(dp.call(&mut infos, &data));
    }
}

#[test]
fn test_program_native_file_move_funds_success() {
    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let inst = DynamicInstruction::Call { input };
    let tx = Transaction::new(
        &Keypair::new(),
        &[
            Keypair::new().pubkey(),
            Keypair::new().pubkey(),
            Keypair::new().pubkey(),
        ],
        DynamicProgram::id(),
        serialize(&inst).unwrap(),
        Hash::default(),
        0,
    );

    let dp_native = DynamicProgram::Native {
        name: "move_funds".to_string(),
    };
    let mut accounts = vec![Account::default(), Account::default(), Account::default()];
    accounts[0].tokens = 10;
    accounts[1].tokens = 1;
    accounts[1].userdata = serialize(&dp_native).unwrap();
    accounts[2].tokens = 1;

        let dp = DynamicProgram::new_native("move_funds".to_string()).unwrap();
        assert!(dp.call(&mut infos, &data));
    }
    assert_eq!(0, accounts[0].tokens);
    assert_eq!(11, accounts[2].tokens);
}

// #[test]
// fn test_program_native_file_move_funds_insufficient_funds() {
//     let tokens: i64 = 100;
//     let data: Vec<u8> = serialize(&tokens).unwrap();
//     let keys = vec![Pubkey::default(); 2];
//     let mut accounts = vec![Account::default(), Account::default()];
//     accounts[0].tokens = 10;
//     accounts[1].tokens = 1;

//     {
//         let mut infos: Vec<_> = (&keys)
//             .into_iter()
//             .zip(&mut accounts)
//             .map(|(key, account)| KeyedAccount { key, account })
//             .collect();

//         let dp = DynamicProgram::new_native("move_funds".to_string());
//         dp.call(&mut infos, &data);
//     }
//     assert_eq!(10, accounts[0].tokens);
//     assert_eq!(1, accounts[1].tokens);
// }

// #[test]
// fn test_program_program_native_move_funds_succes_many_threads() {
//     let num_threads = 42; // number of threads to spawn
//     let num_iters = 100; // number of iterations of test in each thread
//     let mut threads = Vec::new();
//     for _t in 0..num_threads {
//         threads.push(thread::spawn(move || {
//             for _i in 0..num_iters {
//                 {
//                     let tokens: i64 = 100;
//                     let data: Vec<u8> = serialize(&tokens).unwrap();
//                     let keys = vec![Pubkey::default(); 2];
//                     let mut accounts = vec![Account::default(), Account::default()];
//                     accounts[0].tokens = 100;
//                     accounts[1].tokens = 1;

//                     {
//                         let mut infos: Vec<_> = (&keys)
//                             .into_iter()
//                             .zip(&mut accounts)
//                             .map(|(key, account)| KeyedAccount { key, account })
//                             .collect();

//                         let dp = DynamicProgram::new_native("move_funds".to_string());
//                         dp.call(&mut infos, &data);
//                     }
//                     assert_eq!(0, accounts[0].tokens);
//                     assert_eq!(101, accounts[1].tokens);
//                 }
//             }
//         }));
//     }

//     for thread in threads {
//         thread.join().unwrap();
//     }
// }

#[cfg(feature = "bpf_c")]
#[test]
#[ignore]
fn test_program_bpf_file_noop_rust() {
    let tx = Transaction::new(
        &Keypair::new(),
        &[Keypair::new().pubkey()],
        DynamicProgram::id(),
        vec![0u8],
        Hash::default(),
        0,
    );

    let dp = DynamicProgram::BpfFile {
        name: "noop_rust".to_string(),
    };
    let mut accounts = vec![Account::default()];
    accounts[0].tokens = 1;
    accounts[0].userdata = serialize(&dp).unwrap();

    {
        let mut infos: Vec<_> = (&keys)
            .into_iter()
            .zip(&mut accounts)
            .map(|(key, account)| KeyedAccount { key, account })
            .collect();

        let dp = DynamicProgram::new_native("move_funds".to_string()).unwrap();
        assert!(!dp.call(&mut infos, &data));
    }
    assert_eq!(10, accounts[0].tokens);
    assert_eq!(1, accounts[1].tokens);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_file_move_funds_c() {
    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let inst = DynamicInstruction::Call { input };
    let tx = Transaction::new(
        &Keypair::new(),
        &[
            Keypair::new().pubkey(),
            Keypair::new().pubkey(),
            Keypair::new().pubkey(),
        ],
        DynamicProgram::id(),
        serialize(&inst).unwrap(),
        Hash::default(),
        0,
    );

    let dp_bpf = DynamicProgram::BpfFile {
        name: "move_funds_c".to_string(),
    };
    let mut accounts = vec![Account::default(), Account::default(), Account::default()];
    accounts[0].tokens = 10;
    accounts[1].tokens = 1;
    accounts[1].userdata = serialize(&dp_bpf).unwrap();
    accounts[2].tokens = 1;

    process_transaction(&tx, &mut accounts);
    assert_eq!(0, accounts[0].tokens);
    assert_eq!(11, accounts[2].tokens);
}

#[cfg(feature = "bpf_c")]
fn tictactoe_command(command: Command, accounts: &mut Vec<Account>, player: Pubkey) {
    let p = &command as *const Command as *const u8;
    let input: &[u8] = unsafe { std::slice::from_raw_parts(p, std::mem::size_of::<Command>()) };
    //let input = serialize(data).unwrap();
    let inst = DynamicInstruction::Call {
        input: input.to_vec(),
    };
    let tx = Transaction::new(
        &Keypair::new(),
        &vec![Pubkey::default(), Pubkey::default(), player],
        DynamicProgram::id(),
        serialize(&inst).unwrap(),
        Hash::default(),
        0,
    );
    process_transaction(&tx, accounts);
}

#[cfg(feature = "bpf_c")]
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

                                assert!(dp.call(&mut infos, &data));
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
