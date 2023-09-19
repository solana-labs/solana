#![allow(clippy::arithmetic_side_effects)]
#![feature(test)]

use {
    crossbeam_channel::{unbounded, Receiver},
    rayon::{
        iter::IndexedParallelIterator,
        prelude::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator},
    },
    solana_core::banking_stage::{
        committer::Committer, consumer::Consumer, qos_service::QosService,
    },
    solana_entry::entry::Entry,
    solana_ledger::{
        blockstore::Blockstore,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
    },
    solana_poh::{
        poh_recorder::{create_test_recorder, PohRecorder},
        poh_service::PohService,
    },
    solana_runtime::bank::Bank,
    solana_sdk::{
        account::Account, feature_set::apply_cost_tracker_during_replay, signature::Keypair,
        signer::Signer, stake_history::Epoch, system_program, system_transaction,
        transaction::SanitizedTransaction,
    },
    std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
    tempfile::TempDir,
    test::Bencher,
};

extern crate test;

fn create_accounts(num: usize) -> Vec<Keypair> {
    (0..num).into_par_iter().map(|_| Keypair::new()).collect()
}

fn create_funded_accounts(bank: &Bank, num: usize) -> Vec<Keypair> {
    assert!(
        num.is_power_of_two(),
        "must be power of 2 for parallel funding tree"
    );
    let accounts = create_accounts(num);

    accounts.par_iter().for_each(|account| {
        bank.store_account(
            &account.pubkey(),
            &Account {
                lamports: 5100,
                data: vec![],
                owner: system_program::id(),
                executable: false,
                rent_epoch: Epoch::MAX,
            },
        );
    });

    accounts
}

fn create_transactions(bank: &Bank, num: usize) -> Vec<SanitizedTransaction> {
    let funded_accounts = create_funded_accounts(bank, 2 * num);
    funded_accounts
        .into_par_iter()
        .chunks(2)
        .map(|chunk| {
            let from = &chunk[0];
            let to = &chunk[1];
            system_transaction::transfer(from, &to.pubkey(), 1, bank.last_blockhash())
        })
        .map(SanitizedTransaction::from_transaction_for_tests)
        .collect()
}

fn create_consumer(poh_recorder: &RwLock<PohRecorder>) -> Consumer {
    let (replay_vote_sender, _replay_vote_receiver) = unbounded();
    let committer = Committer::new(None, replay_vote_sender, Arc::default());
    let transaction_recorder = poh_recorder.read().unwrap().new_recorder();
    Consumer::new(committer, transaction_recorder, QosService::new(0), None)
}

struct BenchFrame {
    bank: Arc<Bank>,
    ledger_path: TempDir,
    exit: Arc<AtomicBool>,
    poh_recorder: Arc<RwLock<PohRecorder>>,
    poh_service: PohService,
    signal_receiver: Receiver<(Arc<Bank>, (Entry, u64))>,
}

fn setup(apply_cost_tracker_during_replay: bool) -> BenchFrame {
    let mint_total = u64::MAX;
    let GenesisConfigInfo {
        mut genesis_config, ..
    } = create_genesis_config(mint_total);

    // Set a high ticks_per_slot so we don't run out of ticks
    // during the benchmark
    genesis_config.ticks_per_slot = 10_000;

    let mut bank = Bank::new_for_benches(&genesis_config);

    if !apply_cost_tracker_during_replay {
        bank.deactivate_feature(&apply_cost_tracker_during_replay::id());
    }

    // Allow arbitrary transaction processing time for the purposes of this bench
    bank.ns_per_slot = u128::MAX;

    // set cost tracker limits to MAX so it will not filter out TXs
    bank.write_cost_tracker()
        .unwrap()
        .set_limits(std::u64::MAX, std::u64::MAX, std::u64::MAX);
    let bank = Arc::new(bank);

    let ledger_path = TempDir::new().unwrap();
    let blockstore = Arc::new(
        Blockstore::open(ledger_path.path()).expect("Expected to be able to open database ledger"),
    );
    let (exit, poh_recorder, poh_service, signal_receiver) =
        create_test_recorder(bank.clone(), blockstore, None, None);

    BenchFrame {
        bank,
        ledger_path,
        exit,
        poh_recorder,
        poh_service,
        signal_receiver,
    }
}

fn bench_process_and_record_transactions(
    bencher: &mut Bencher,
    batch_size: usize,
    apply_cost_tracker_during_replay: bool,
) {
    let BenchFrame {
        bank,
        ledger_path: _ledger_path,
        exit,
        poh_recorder,
        poh_service,
        signal_receiver: _signal_receiver,
    } = setup(apply_cost_tracker_during_replay);
    let consumer = create_consumer(&poh_recorder);
    let transactions = create_transactions(&bank, 2_usize.pow(20));
    let mut transaction_iter = transactions.chunks(batch_size);

    bencher.iter(move || {
        let summary =
            consumer.process_and_record_transactions(&bank, transaction_iter.next().unwrap(), 0);
        assert!(summary
            .execute_and_commit_transactions_output
            .commit_transactions_result
            .is_ok());
    });

    exit.store(true, Ordering::Relaxed);
    poh_service.join().unwrap();
}

#[bench]
fn bench_process_and_record_transactions_unbatched(bencher: &mut Bencher) {
    bench_process_and_record_transactions(bencher, 1, true);
}

#[bench]
fn bench_process_and_record_transactions_half_batch(bencher: &mut Bencher) {
    bench_process_and_record_transactions(bencher, 32, true);
}

#[bench]
fn bench_process_and_record_transactions_full_batch(bencher: &mut Bencher) {
    bench_process_and_record_transactions(bencher, 64, true);
}

#[bench]
fn bench_process_and_record_transactions_unbatched_disable_tx_cost_update(bencher: &mut Bencher) {
    bench_process_and_record_transactions(bencher, 1, false);
}

#[bench]
fn bench_process_and_record_transactions_half_batch_disable_tx_cost_update(bencher: &mut Bencher) {
    bench_process_and_record_transactions(bencher, 32, false);
}

#[bench]
fn bench_process_and_record_transactions_full_batch_disable_tx_cost_update(bencher: &mut Bencher) {
    bench_process_and_record_transactions(bencher, 64, false);
}
