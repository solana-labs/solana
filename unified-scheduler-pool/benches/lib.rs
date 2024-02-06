#![allow(unused_imports, dead_code)]
#![feature(test)]

extern crate test;

#[cfg(not(target_env = "msvc"))]
use jemallocator::Jemalloc;

#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

use {
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        bank::Bank,
        bank_forks::BankForks,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        installed_scheduler_pool::{
            DefaultScheduleExecutionArg, InstalledScheduler, SchedulingContext,
        },
        prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::{
        scheduling::SchedulingMode,
        transaction::{Result, SanitizedTransaction},
    },
    solana_unified_scheduler_logic::{Page, SchedulingStateMachine},
    solana_unified_scheduler_pool::{
        HandlerContext, PooledScheduler, SchedulerPool, SpawnableScheduler, TaskHandler,
    },
    std::sync::Arc,
};

#[derive(Debug, Clone)]
struct DummyTaskHandler;

impl TaskHandler<DefaultScheduleExecutionArg> for DummyTaskHandler {
    fn handle(
        &self,
        _result: &mut Result<()>,
        _timings: &mut ExecuteTimings,
        _bank: &Arc<Bank>,
        _transaction: &SanitizedTransaction,
        _index: usize,
        _handler_context: &HandlerContext,
    ) {
    }

    fn create<T: SpawnableScheduler<Self, DefaultScheduleExecutionArg>>(
        _pool: &SchedulerPool<T, Self, DefaultScheduleExecutionArg>,
    ) -> Self {
        Self
    }
}

fn setup_dummy_fork_graph(bank: Bank) -> Arc<Bank> {
    let slot = bank.slot();
    let bank_fork = BankForks::new_rw_arc(bank);
    let bank = bank_fork.read().unwrap().get(slot).unwrap();
    bank.loaded_programs_cache
        .write()
        .unwrap()
        .set_fork_graph(bank_fork);
    bank
}

use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    message::Message,
    pubkey::Pubkey,
    signature::Signer,
    signer::keypair::Keypair,
    transaction::Transaction,
};

fn do_bench_tx_throughput(label: &str, bencher: &mut Criterion) {
    solana_logger::setup();

    /*
    let GenesisConfigInfo {
        genesis_config,
        ..
    } = create_genesis_config(10_000);
    */
    let payer = Keypair::new();

    let mut accounts = vec![];
    for i in 0..100 {
        if i % 2 == 0 {
            accounts.push(AccountMeta::new(Keypair::new().pubkey(), true));
        } else {
            accounts.push(AccountMeta::new_readonly(Keypair::new().pubkey(), true));
        }
    }

    let memo_ix = Instruction {
        program_id: Pubkey::default(),
        accounts,
        data: vec![0x00],
    };
    let mut ixs = vec![];
    for _ in 0..1 {
        ixs.push(memo_ix.clone());
    }
    let msg = Message::new(&ixs, Some(&payer.pubkey()));
    let txn = Transaction::new_unsigned(msg);
    //assert_eq!(wire_txn.len(), 3);
    let tx0 = SanitizedTransaction::from_transaction_for_tests(txn);
    /*
    let bank = Bank::new_for_tests(&genesis_config);
    let bank = setup_dummy_fork_graph(bank);
    let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
    let pool = SchedulerPool::<PooledScheduler<DummyTaskHandler, DefaultScheduleExecutionArg>, _, _>::new(
        None,
        None,
        None,
        ignored_prioritization_fee_cache,
    );
    let context = SchedulingContext::new(SchedulingMode::BlockVerification, bank.clone());
    */

    let (s, r) = crossbeam_channel::bounded(1000);

    use std::sync::atomic::AtomicUsize;
    let i = Arc::new(AtomicUsize::default());
    use std::sync::Mutex;
    let pages: Arc<Mutex<std::collections::HashMap<solana_sdk::pubkey::Pubkey, Page>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    /*
    for _ in 0..5 {
        std::thread::Builder::new()
            .name("solScGen".to_owned())
            .spawn({
                let pages = pages.clone();
                let i = i.clone();
                let tx1 = tx0.clone();
                let s = s.clone();
                move || loop {
                    let tasks = std::iter::repeat_with(|| SchedulingStateMachine::create_task(tx1.clone(), i.fetch_add(1, std::sync::atomic::Ordering::Relaxed), &mut |address| {
        pages.lock().unwrap().entry(address).or_default().clone()
    })).take(100).collect::<Vec<_>>();
                    if s.send(tasks).is_err() {
                        break;
                    }
                }
            })
            .unwrap();
    }
    std::thread::sleep(std::time::Duration::from_secs(5));
    */

    //assert_eq!(bank.transaction_count(), 0);
    //let mut scheduler = pool.do_take_scheduler(context);

    let mut scheduler =
        unsafe { SchedulingStateMachine::exclusively_initialize_current_thread_for_scheduling() };

    let tasks = std::iter::repeat_with(|| {
        SchedulingStateMachine::create_task(
            tx0.clone(),
            i.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            &mut |address| pages.lock().unwrap().entry(address).or_default().clone(),
        )
    })
    .take(100)
    .collect::<Vec<_>>();
    s.send(tasks).unwrap();

    bencher.bench_function(label, |b| {
        b.iter(|| {
            for _ in 0..600 {
                let mut first_task = None;
                let tt = r.recv().unwrap();
                let mut new_tasks = Vec::with_capacity(tt.len());
                for t in tt {
                    /*
                    scheduler.schedule_task(t);
                    */
                    if let Some(task) = scheduler.schedule_task(t) {
                        first_task = Some(task);
                    }
                }
                scheduler.deschedule_task(first_task.as_ref().unwrap());
                new_tasks.push(first_task.unwrap());
                while let Some(unblocked_task) = scheduler.schedule_unblocked_task() {
                    scheduler.deschedule_task(&unblocked_task);
                    new_tasks.push(unblocked_task);
                }
                assert!(scheduler.has_no_active_task());
                s.send(new_tasks).unwrap();
            }
            /*
            scheduler.pause_for_recent_blockhash();
            scheduler.clear_session_result_with_timings();
            scheduler.restart_session();
            */
        })
    });
}

fn bench_entrypoint(bencher: &mut Criterion) {
    do_bench_tx_throughput("bench_tx_throughput", bencher)
}

use criterion::{criterion_group, criterion_main, Criterion};
criterion_group!(benches, bench_entrypoint);
criterion_main!(benches);
