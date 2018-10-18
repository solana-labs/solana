extern crate bincode;
extern crate elf;
extern crate solana;
extern crate solana_program_interface;

use bincode::serialize;
use solana::bank::Bank;
use solana::loader_transaction::LoaderTransaction;
use solana::logger;
use solana::mint::Mint;
use solana::native_loader;
use solana::signature::{Keypair, KeypairUtil};
use solana::system_transaction::SystemTransaction;
use solana::transaction::Transaction;

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

    // allocate, populate, finalize user program

    let tx = Transaction::system_create(
        &mint.keypair(),
        program.pubkey(),
        mint.last_id(),
        1,
        56, // TODO How does the user know how much space to allocate, this is really an internally known size
        native_loader::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    println!("id: {:?}", native_loader::id());
    let name = String::from("noop");
    let tx = Transaction::write(
        &program,
        native_loader::id(),
        0,
        name.as_bytes().to_vec(),
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    println!("id after: {:?}", native_loader::id());

    let tx = Transaction::finalize(&program, native_loader::id(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::system_spawn(&program, mint.last_id(), 0);
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
fn test_transaction_load_lua() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let loader = Keypair::new();
    let program = Keypair::new();
    let from = Keypair::new();
    let to = Keypair::new().pubkey();

    // allocate, populate, and finalize Lua loader

    let tx = Transaction::system_create(
        &mint.keypair(),
        loader.pubkey(),
        mint.last_id(),
        1,
        56, // TODO How does the user know how much space to allocate for what should be an internally known size
        native_loader::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let name = String::from("lua_loader");
    let tx = Transaction::write(
        &loader,
        native_loader::id(),
        0,
        name.as_bytes().to_vec(),
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::finalize(&loader, native_loader::id(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::system_spawn(&loader, mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // allocate, populate, and finalize user program

    let bytes = r#"
            print("Lua Script!")
            local tokens, _ = string.unpack("I", data)
            accounts[1].tokens = accounts[1].tokens - tokens
            accounts[2].tokens = accounts[2].tokens + tokens
        "#.as_bytes()
    .to_vec();

    let tx = Transaction::system_create(
        &mint.keypair(),
        program.pubkey(),
        mint.last_id(),
        1,
        300, // TODO How does the user know how much space to allocate for what should be an internally known size
        loader.pubkey(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::write(&program, loader.pubkey(), 0, bytes, mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::finalize(&program, loader.pubkey(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::system_spawn(&program, mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    // Call user program with two accounts

    let tx = Transaction::system_create(
        &mint.keypair(),
        from.pubkey(),
        mint.last_id(),
        10,
        0,
        program.pubkey(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::system_create(
        &mint.keypair(),
        to,
        mint.last_id(),
        1,
        0,
        program.pubkey(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let data = serialize(&10).unwrap();
    let tx = Transaction::new(&from, &[to], program.pubkey(), data, mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));
    assert_eq!(bank.get_balance(&from.pubkey()), 0);
    assert_eq!(bank.get_balance(&to), 11);
}

#[cfg(feature = "bpf_c")]
#[test]
fn test_transaction_load_bpf() {
    logger::setup();

    let mint = Mint::new(50);
    // TODO in a test like this how should the last_id be incremented, as used here it is always the same
    //      which leads to duplicate tx signature errors
    let bank = Bank::new(&mint);
    let loader = Keypair::new();
    let program = Keypair::new();

    // allocate, populate, finalize BPF loader

    let tx = Transaction::system_create(
        &mint.keypair(),
        loader.pubkey(),
        mint.last_id(),
        1,
        56, // TODO How does the user know how much space to allocate for what should be an internally known size
        native_loader::id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let name = String::from("bpf_loader");
    let tx = Transaction::write(
        &loader,
        native_loader::id(),
        0,
        name.as_bytes().to_vec(),
        mint.last_id(),
        0,
    );
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::finalize(&loader, native_loader::id(), mint.last_id(), 0);
    check_tx_results(&bank, &tx, bank.process_transactions(&vec![tx.clone()]));

    let tx = Transaction::system_spawn(&loader, mint.last_id(), 0);
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

    let tx = Transaction::system_spawn(&program, mint.last_id(), 0);
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
