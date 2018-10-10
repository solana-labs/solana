extern crate bincode;
extern crate elf;
extern crate solana;
extern crate solana_program_interface;

use bincode::serialize;
use solana::bank::Bank;
use solana::dynamic_instruction::DynamicInstruction;
use solana::dynamic_program::DynamicProgram;
#[cfg(feature = "bpf_c")]
use solana::dynamic_program::{ProgramPath, PLATFORM_SECTION_C, PLATFORM_SECTION_RS};
use solana::dynamic_transaction::ProgramTransaction;
use solana::hash::Hash;
use solana::logger;
use solana::mint::Mint;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
#[cfg(feature = "bpf_c")]
use solana::tictactoe_program::Command;
use solana::transaction::Transaction;
use solana_program_interface::account::Account;
#[cfg(feature = "bpf_c")]
use solana_program_interface::pubkey::Pubkey;
use std::mem;
#[cfg(feature = "bpf_c")]
use std::path::Path;
use std::thread;

// TODO test modified user data
// TODO test failure if account tokens decrease but not assigned to program

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

/// test_transaction* test dynamic programs via the Transaction interface

#[test]
fn test_transaction_load_native() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let program = Keypair::new().pubkey();

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
    let name = "noop".to_string();
    let instruction = DynamicInstruction::LoadNative { name };
    let tx =
        Transaction::program_new_load(&mint.keypair(), program, &instruction, mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx =
        Transaction::program_new_call(&mint.keypair(), program, &[], vec![1u8], mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
}

fn transaction_call_move_funds(instruction: &DynamicInstruction) {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let program = Keypair::new().pubkey();
    let from = Keypair::new();
    let to = Keypair::new().pubkey();

    // println!("from pubkey {:?}", from.pubkey());
    // println!("to pubkey {:?}", to);
    // println!("program pubkey {:?}", program);

    // mint allocate 'from' account
    let tx = Transaction::system_new(&mint.keypair(), from.pubkey(), 11, mint.last_id());
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(39, bank.get_balance(&mint.keypair().pubkey()));

    // mint allocate 'to' account
    let tx = Transaction::system_new(&mint.keypair(), to, 1, mint.last_id());
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(38, bank.get_balance(&mint.keypair().pubkey()));

    // from allocate program bits account
    // TODO if from is not assigned to SystemProgram this fails with no indication that it did
    let tx = Transaction::system_create(
        &from,
        program,
        mint.last_id(),
        1,
        mem::size_of_val(instruction) as u64,
        DynamicProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // from populate program bits account
    let tx = Transaction::program_new_load(&from, program, instruction, mint.last_id(), 0);
    let results = bank.process_transactions(&vec![tx.clone()]);
    check_tx_results(&bank, &tx, results);

    // from assign itself to DynamicProgram
    let tx = Transaction::system_assign(&from, mint.last_id(), DynamicProgram::id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // call the program
    let tokens: i64 = 10;
    let input = serialize(&tokens).unwrap();
    let tx = Transaction::program_new_call(&from, program, &[to], input, mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    assert_eq!(38, bank.get_balance(&mint.keypair().pubkey()));
    assert_eq!(0, bank.get_balance(&from.pubkey()));
    assert_eq!(11, bank.get_balance(&to));
    assert_eq!(1, bank.get_balance(&program));
}

#[test]
fn test_transaction_native_move_funds() {
    let name = "move_funds".to_string();
    let instruction = DynamicInstruction::LoadNative { name };
    transaction_call_move_funds(&instruction);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_transaction_bpffile_move_funds() {
    let name = "move_funds_c".to_string();
    let instruction = DynamicInstruction::LoadBpfFile { name };
    transaction_call_move_funds(&instruction);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_transaction_bpf_move_funds() {
    let name = "move_funds_c".to_string();

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

    let instruction = DynamicInstruction::LoadBpf {
        offset: 0,
        prog: prog.clone(),
    };
    transaction_call_move_funds(&instruction);
}

/// test_transaction* test dynamic programs via the DynamicProgram

fn dynamic_propgram_process_transaction(tx: &Transaction, accounts: &mut [Account]) {
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

    dynamic_propgram_process_transaction(&tx, &mut accounts);
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

    dynamic_propgram_process_transaction(&tx, &mut accounts);
    assert_eq!(0, accounts[0].tokens);
    assert_eq!(11, accounts[2].tokens);
}

#[test]
fn test_program_program_native_move_funds_succes_many_threads() {
    let num_threads = 42; // number of threads to spawn
    let num_iters = 100; // number of iterations of test in each thread
    let mut threads = Vec::new();
    for _t in 0..num_threads {
        threads.push(thread::spawn(move || {
            for _i in 0..num_iters {
                {
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
                    let mut accounts =
                        vec![Account::default(), Account::default(), Account::default()];
                    accounts[0].tokens = 10;
                    accounts[1].tokens = 1;
                    accounts[1].userdata = serialize(&dp_native).unwrap();
                    accounts[2].tokens = 1;

                    dynamic_propgram_process_transaction(&tx, &mut accounts);
                    assert_eq!(0, accounts[0].tokens);
                    assert_eq!(11, accounts[2].tokens);
                }
            }
        }));
    }

    for thread in threads {
        thread.join().unwrap();
    }
}

#[test]
fn test_program_native_file_move_funds_insufficident_funds() {
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
    accounts[0].tokens = 9;
    accounts[1].tokens = 1;
    accounts[1].userdata = serialize(&dp_native).unwrap();
    accounts[2].tokens = 1;

    dynamic_propgram_process_transaction(&tx, &mut accounts);
    assert_eq!(9, accounts[0].tokens);
    assert_eq!(1, accounts[2].tokens);
}

#[cfg(feature = "bpf_c")]
#[test]
#[ignore]
fn test_program_bpf_file_noop_rust() {
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
        name: "noop_rust".to_string(),
    };
    let mut accounts = vec![Account::default(), Account::default(), Account::default()];
    accounts[0].tokens = 10;
    accounts[1].tokens = 1;
    accounts[1].userdata = serialize(&dp_bpf).unwrap();
    accounts[2].tokens = 1;

    dynamic_propgram_process_transaction(&tx, &mut accounts);
    assert_eq!(0, accounts[0].tokens);
    assert_eq!(11, accounts[2].tokens);
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

    dynamic_propgram_process_transaction(&tx, &mut accounts);
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
    dynamic_propgram_process_transaction(&tx, accounts);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_file_tictactoe_c() {
    let dp_bpf = DynamicProgram::BpfFile {
        name: "tictactoe_c".to_string(),
    };
    let game_size = 0x78; // corresponds to the C structure size
    let mut accounts = vec![
        Account::new(0, 0, DynamicProgram::id()),
        Account::new(0, 0, DynamicProgram::id()),
        Account::new(0, game_size, DynamicProgram::id()),
        Account::new(0, 0, DynamicProgram::id()),
    ];
    accounts[1].userdata = serialize(&dp_bpf).unwrap();

    tictactoe_command(Command::Init, &mut accounts, Pubkey::new(&[0xA; 32]));
    tictactoe_command(
        Command::Join(0xAABBCCDD),
        &mut accounts,
        Pubkey::new(&[0xB; 32]),
    );
    tictactoe_command(Command::Move(1, 1), &mut accounts, Pubkey::new(&[0xA; 32]));
    tictactoe_command(Command::Move(0, 0), &mut accounts, Pubkey::new(&[0xB; 32]));
    tictactoe_command(Command::Move(2, 0), &mut accounts, Pubkey::new(&[0xA; 32]));
    tictactoe_command(Command::Move(0, 2), &mut accounts, Pubkey::new(&[0xB; 32]));
    tictactoe_command(Command::Move(2, 2), &mut accounts, Pubkey::new(&[0xA; 32]));
    tictactoe_command(Command::Move(0, 1), &mut accounts, Pubkey::new(&[0xB; 32]));

    assert_eq!([0xAu8; 32], accounts[2].userdata[0..32]); // validate x's key
    assert_eq!([0xBu8; 32], accounts[2].userdata[32..64]); // validate o's key
    assert_eq!([4, 0, 0, 0], accounts[2].userdata[64..68]); // validate that o won
}
