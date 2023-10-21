#![feature(test)]
#![allow(clippy::arithmetic_side_effects)]

extern crate test;

use {
    log::*,
    solana_program_runtime::declare_process_instruction,
    solana_runtime::{
        bank::{test_utils::goto_end_of_slot, *},
        bank_client::BankClient,
        loader_utils::create_invoke_instruction,
    },
    solana_sdk::{
        client::{AsyncClient, SyncClient},
        clock::MAX_RECENT_BLOCKHASHES,
        genesis_config::create_genesis_config,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    },
    std::{sync::Arc, thread::sleep, time::Duration},
    test::Bencher,
};

const BUILTIN_PROGRAM_ID: [u8; 32] = [
    98, 117, 105, 108, 116, 105, 110, 95, 112, 114, 111, 103, 114, 97, 109, 95, 105, 100, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
];

const NOOP_PROGRAM_ID: [u8; 32] = [
    98, 117, 105, 108, 116, 105, 110, 95, 112, 114, 111, 103, 114, 97, 109, 95, 105, 100, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1,
];

pub fn create_builtin_transactions(
    bank_client: &BankClient,
    mint_keypair: &Keypair,
) -> Vec<Transaction> {
    let program_id = Pubkey::from(BUILTIN_PROGRAM_ID);

    (0..4096)
        .map(|_| {
            // Seed the signer account
            let rando0 = Keypair::new();
            bank_client
                .transfer_and_confirm(10_000, mint_keypair, &rando0.pubkey())
                .unwrap_or_else(|_| panic!("{}:{}", line!(), file!()));

            let instruction = create_invoke_instruction(rando0.pubkey(), program_id, &1u8);
            let blockhash = bank_client.get_latest_blockhash().unwrap();
            let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
            Transaction::new(&[&rando0], message, blockhash)
        })
        .collect()
}

pub fn create_native_loader_transactions(
    bank_client: &BankClient,
    mint_keypair: &Keypair,
) -> Vec<Transaction> {
    let program_id = Pubkey::from(NOOP_PROGRAM_ID);

    (0..4096)
        .map(|_| {
            // Seed the signer account©41
            let rando0 = Keypair::new();
            bank_client
                .transfer_and_confirm(10_000, mint_keypair, &rando0.pubkey())
                .unwrap_or_else(|_| panic!("{}:{}", line!(), file!()));

            let instruction = create_invoke_instruction(rando0.pubkey(), program_id, &1u8);
            let blockhash = bank_client.get_latest_blockhash().unwrap();
            let message = Message::new(&[instruction], Some(&mint_keypair.pubkey()));
            Transaction::new(&[&rando0], message, blockhash)
        })
        .collect()
}

fn sync_bencher(bank: &Bank, _bank_client: &BankClient, transactions: &[Transaction]) {
    let results = bank.process_transactions(transactions.iter());
    assert!(results.iter().all(Result::is_ok));
}

fn async_bencher(bank: &Bank, bank_client: &BankClient, transactions: &[Transaction]) {
    for transaction in transactions.iter().cloned() {
        bank_client.async_send_transaction(transaction).unwrap();
    }
    for _ in 0..1_000_000_000_u64 {
        if bank
            .get_signature_status(transactions.last().unwrap().signatures.get(0).unwrap())
            .is_some()
        {
            break;
        }
        sleep(Duration::from_nanos(1));
    }
    if bank
        .get_signature_status(transactions.last().unwrap().signatures.get(0).unwrap())
        .unwrap()
        .is_err()
    {
        error!(
            "transaction failed: {:?}",
            bank.get_signature_status(transactions.last().unwrap().signatures.get(0).unwrap())
                .unwrap()
        );
        panic!();
    }
}

#[allow(clippy::type_complexity)]
fn do_bench_transactions(
    bencher: &mut Bencher,
    bench_work: &dyn Fn(&Bank, &BankClient, &[Transaction]),
    create_transactions: &dyn Fn(&BankClient, &Keypair) -> Vec<Transaction>,
) {
    solana_logger::setup();
    let ns_per_s = 1_000_000_000;
    let (mut genesis_config, mint_keypair) = create_genesis_config(100_000_000_000_000);
    genesis_config.ticks_per_slot = 100;

    let bank = Bank::new_for_benches(&genesis_config);
    // freeze bank so that slot hashes is populated
    bank.freeze();

    declare_process_instruction!(MockBuiltin, 1, |_invoke_context| {
        // Do nothing
        Ok(())
    });

    let mut bank = Bank::new_from_parent(Arc::new(bank), &Pubkey::default(), 1);
    bank.add_mockup_builtin(Pubkey::from(BUILTIN_PROGRAM_ID), MockBuiltin::vm);
    bank.add_builtin_account("solana_noop_program", &Pubkey::from(NOOP_PROGRAM_ID), false);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(bank.clone());
    let transactions = create_transactions(&bank_client, &mint_keypair);

    // Do once to fund accounts, load modules, etc...
    let results = bank.process_transactions(transactions.iter());
    assert!(results.iter().all(Result::is_ok));

    bencher.iter(|| {
        // Since bencher runs this multiple times, we need to clear the signatures.
        bank.clear_signatures();
        bench_work(&bank, &bank_client, &transactions);
    });

    let summary = bencher.bench(|_bencher| Ok(())).unwrap().unwrap();
    info!("  {:?} transactions", transactions.len());
    info!("  {:?} ns/iter median", summary.median as u64);
    assert!(0f64 != summary.median);
    let tps = transactions.len() as u64 * (ns_per_s / summary.median as u64);
    info!("  {:?} TPS", tps);
}

#[bench]
#[ignore]
fn bench_bank_sync_process_builtin_transactions(bencher: &mut Bencher) {
    do_bench_transactions(bencher, &sync_bencher, &create_builtin_transactions);
}

#[bench]
#[ignore]
fn bench_bank_sync_process_native_loader_transactions(bencher: &mut Bencher) {
    do_bench_transactions(bencher, &sync_bencher, &create_native_loader_transactions);
}

#[bench]
#[ignore]
fn bench_bank_async_process_builtin_transactions(bencher: &mut Bencher) {
    do_bench_transactions(bencher, &async_bencher, &create_builtin_transactions);
}

#[bench]
#[ignore]
fn bench_bank_async_process_native_loader_transactions(bencher: &mut Bencher) {
    do_bench_transactions(bencher, &async_bencher, &create_native_loader_transactions);
}

#[bench]
#[ignore]
fn bench_bank_update_recent_blockhashes(bencher: &mut Bencher) {
    let (genesis_config, _mint_keypair) = create_genesis_config(100);
    let mut bank = Arc::new(Bank::new_for_benches(&genesis_config));
    goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
    let genesis_hash = bank.last_blockhash();
    // Prime blockhash_queue
    for i in 0..(MAX_RECENT_BLOCKHASHES + 1) {
        bank = Arc::new(Bank::new_from_parent(
            bank,
            &Pubkey::default(),
            (i + 1) as u64,
        ));
        goto_end_of_slot(Arc::get_mut(&mut bank).unwrap());
    }
    // Verify blockhash_queue is full (genesis hash has been kicked out)
    assert!(!bank.is_hash_valid_for_age(&genesis_hash, MAX_RECENT_BLOCKHASHES));
    bencher.iter(|| {
        bank.update_recent_blockhashes();
    });
}
