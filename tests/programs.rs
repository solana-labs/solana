use solana;
use solana_native_loader;

use solana::bank::Bank;

use solana::mint::Mint;
#[cfg(feature = "bpf_c")]
use solana_sdk::bpf_loader;
use solana_sdk::loader_transaction::LoaderTransaction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::transaction::Transaction;
#[cfg(feature = "bpf_c")]
use std::env;
#[cfg(feature = "bpf_c")]
use std::fs::File;
#[cfg(feature = "bpf_c")]
use std::io::Read;
#[cfg(feature = "bpf_c")]
use std::path::PathBuf;

/// BPF program file extension
#[cfg(feature = "bpf_c")]
const PLATFORM_FILE_EXTENSION_BPF: &str = "so";
/// Create a BPF program file name
#[cfg(feature = "bpf_c")]
fn create_bpf_path(name: &str) -> PathBuf {
    let mut pathbuf = {
        let current_exe = env::current_exe().unwrap();
        PathBuf::from(current_exe.parent().unwrap().parent().unwrap())
    };
    pathbuf.push("bpf/");
    pathbuf.push(name);
    pathbuf.set_extension(PLATFORM_FILE_EXTENSION_BPF);
    pathbuf
}

fn check_tx_results(bank: &Bank, tx: &Transaction, result: Vec<solana::bank::Result<()>>) {
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], Ok(()));
    assert_eq!(
        bank.get_signature(&tx.last_id, &tx.signatures[0]),
        Some(Ok(()))
    );
}

struct Loader {
    mint: Mint,
    bank: Bank,
    loader: Pubkey,
}

impl Loader {
    pub fn new_dynamic(loader_name: &str) -> Self {
        let mint = Mint::new(50);
        let bank = Bank::new(&mint);
        let loader = Keypair::new();

        // allocate, populate, finalize, and spawn loader

        let tx = Transaction::system_create(
            &mint.keypair(),
            loader.pubkey(),
            mint.last_id(),
            1,
            56, // TODO
            solana_native_loader::id(),
            0,
        );
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        let name = String::from(loader_name);
        let tx = Transaction::loader_write(
            &loader,
            solana_native_loader::id(),
            0,
            name.as_bytes().to_vec(),
            mint.last_id(),
            0,
        );
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        let tx =
            Transaction::loader_finalize(&loader, solana_native_loader::id(), mint.last_id(), 0);
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        let tx = Transaction::system_spawn(&loader, mint.last_id(), 0);
        check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

        Loader {
            mint,
            bank,
            loader: loader.pubkey(),
        }
    }

    pub fn new_native() -> Self {
        let mint = Mint::new(50);
        let bank = Bank::new(&mint);
        let loader = solana_native_loader::id();

        Loader { mint, bank, loader }
    }

    #[cfg(feature = "bpf_c")]
    pub fn new_bpf() -> Self {
        let mint = Mint::new(50);
        let bank = Bank::new(&mint);
        let loader = bpf_loader::id();

        Loader { mint, bank, loader }
    }
}

struct Program {
    program: Keypair,
}

impl Program {
    pub fn new(loader: &Loader, userdata: &Vec<u8>) -> Self {
        let program = Keypair::new();

        // allocate, populate, finalize and spawn program

        let tx = Transaction::system_create(
            &loader.mint.keypair(),
            program.pubkey(),
            loader.mint.last_id(),
            1,
            userdata.len() as u64,
            loader.loader,
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        let chunk_size = 256; // Size of chunk just needs to fit into tx
        let mut offset = 0;
        for chunk in userdata.chunks(chunk_size) {
            let tx = Transaction::loader_write(
                &program,
                loader.loader,
                offset,
                chunk.to_vec(),
                loader.mint.last_id(),
                0,
            );
            check_tx_results(
                &loader.bank,
                &tx,
                loader.bank.process_transactions(&vec![tx.clone()]),
            );
            offset += chunk_size as u32;
        }

        let tx = Transaction::loader_finalize(&program, loader.loader, loader.mint.last_id(), 0);
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        let tx = Transaction::system_spawn(&program, loader.mint.last_id(), 0);
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );

        Program { program }
    }
}

#[test]
fn test_program_native_noop() {
    solana_logger::setup();

    let loader = Loader::new_native();
    let name = String::from("noop");
    let userdata = name.as_bytes().to_vec();
    let program = Program::new(&loader, &userdata);

    // Call user program
    let tx = Transaction::new(
        &loader.mint.keypair(),
        &[],
        program.program.pubkey(),
        &1u8,
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
}

#[test]
fn test_program_lua_move_funds() {
    solana_logger::setup();

    let loader = Loader::new_dynamic("solana_lua_loader");
    let userdata = r#"
            print("Lua Script!")
            local tokens, _ = string.unpack("I", data)
            accounts[1].tokens = accounts[1].tokens - tokens
            accounts[2].tokens = accounts[2].tokens + tokens
        "#
    .as_bytes()
    .to_vec();
    let program = Program::new(&loader, &userdata);
    let from = Keypair::new();
    let to = Keypair::new().pubkey();

    // Call user program with two accounts

    let tx = Transaction::system_create(
        &loader.mint.keypair(),
        from.pubkey(),
        loader.mint.last_id(),
        10,
        0,
        program.program.pubkey(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );

    let tx = Transaction::system_create(
        &loader.mint.keypair(),
        to,
        loader.mint.last_id(),
        1,
        0,
        program.program.pubkey(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );

    let tx = Transaction::new(
        &from,
        &[to],
        program.program.pubkey(),
        &10,
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
    assert_eq!(loader.bank.get_balance(&from.pubkey()), 0);
    assert_eq!(loader.bank.get_balance(&to), 11);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_builtin_bpf_noop() {
    solana_logger::setup();

    let mut file = File::open(create_bpf_path("noop")).expect("file open failed");
    let mut elf = Vec::new();
    file.read_to_end(&mut elf).unwrap();

    let loader = Loader::new_bpf();
    let program = Program::new(&loader, &elf);

    // Call user program
    let tx = Transaction::new(
        &loader.mint.keypair(),
        &[],
        program.program.pubkey(),
        &vec![1u8],
        loader.mint.last_id(),
        0,
    );
    check_tx_results(
        &loader.bank,
        &tx,
        loader.bank.process_transactions(&vec![tx.clone()]),
    );
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_program_bpf_c() {
    solana_logger::setup();

    let programs = [
        "noop",
        "noop++",
        "struct_pass",
        "struct_ret",
        "multiple_static",
    ];
    for program in programs.iter() {
        println!("Test program: {:?}", program);
        let mut file = File::open(create_bpf_path(program)).expect("file open failed");
        let mut elf = Vec::new();
        file.read_to_end(&mut elf).unwrap();

        let loader = Loader::new_dynamic("solana_bpf_loader");
        let program = Program::new(&loader, &elf);

        // Call user program
        let tx = Transaction::new(
            &loader.mint.keypair(),
            &[],
            program.program.pubkey(),
            &vec![1u8],
            loader.mint.last_id(),
            0,
        );
        check_tx_results(
            &loader.bank,
            &tx,
            loader.bank.process_transactions(&vec![tx.clone()]),
        );
    }
}
