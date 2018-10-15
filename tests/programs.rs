extern crate bincode;
extern crate elf;
extern crate solana;
extern crate solana_program_interface;

use solana::bank::Bank;
#[cfg(feature = "bpf_c")]
use solana::dynamic_program::{NativeProgram};
use solana::loader_transaction::LoaderTransaction;
use solana::logger;
use solana::mint::Mint;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
#[cfg(feature = "bpf_c")]
use solana::transaction::Transaction;
#[cfg(feature = "bpf_c")]

#[cfg(feature = "bpf_c")]

// TODO test modified user data
// TODO test failure if account tokens decrease but not assigned to program

fn check_tx_results(bank: &Bank, tx: &Transaction, result: Vec<solana::bank::Result<()>>) {
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], Ok(()));
    assert_eq!(bank.get_signature(&tx.last_id, &tx.signature), Some(Ok(())));
}

#[test]
fn test_transaction_load_native() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let program = Keypair::new();

    println!("mint {:?}", mint.pubkey());
    println!("NativeLoader::id() {:?}", NativeProgram::id());
    println!("program {:?}", program.pubkey());

    // allocate, populate, finalize user program

    let tx = Transaction::system_create(
        &mint.keypair(),
        program.pubkey(),
        mint.last_id(),
        1,
        56, // TODO How does the user know how much space to allocate, this is really an internally known size
        NativeProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let name = String::from("noop");
    let tx = Transaction::write(
        &program,
        NativeProgram::id(),
        0,
        name.as_bytes().to_vec(),
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::finalize(&program, NativeProgram::id(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // Call user program

    let tx = Transaction::new(
        &mint.keypair(), // TODO
        &[],
        program.pubkey(),
        vec![1u8],
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
}

#[test]
fn test_transaction_load_bpf() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let loader = Keypair::new();
    let program = Keypair::new();

    println!("mint {:?}", mint.pubkey());
    println!("NativeLoader::id() {:?}", NativeProgram::id());
    println!("loader {:?}", loader.pubkey());
    println!("program {:?}", program.pubkey());

    // allocate, populate, finalize BPF loader

    let tx = Transaction::system_create(
        &mint.keypair(),
        loader.pubkey(),
        mint.last_id(),
        1,
        56, // TODO How does the user know how much space to allocate for what should be an internally known size
        NativeProgram::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let name = String::from("sobpf");
    let tx = Transaction::write(
        &loader,
        NativeProgram::id(),
        0,
        name.as_bytes().to_vec(),
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::finalize(&loader, NativeProgram::id(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // allocate, populate, and finalize user program

    let tx = Transaction::system_create(
        &mint.keypair(),
        program.pubkey(),
        mint.last_id(),
        1,
        56, // TODO How does the user know how much space to allocate for what should be an internally known size
        loader.pubkey(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let name = String::from("noop_c");
    let tx = Transaction::write(
        &program,
        loader.pubkey(),
        0,
        name.as_bytes().to_vec(),
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::finalize(&program, loader.pubkey(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // Call user program

    let tx = Transaction::new(
        &mint.keypair(), // TODO
        &[],
        program.pubkey(),
        vec![1u8],
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
}
