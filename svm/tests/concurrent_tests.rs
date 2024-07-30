#![cfg(feature = "shuttle-test")]

use {
    crate::mock_bank::{deploy_program, MockForkGraph},
    mock_bank::MockBankCallback,
    shuttle::{
        sync::{Arc, RwLock},
        thread, Runner,
    },
    solana_program_runtime::loaded_programs::ProgramCacheEntryType,
    solana_sdk::pubkey::Pubkey,
    solana_svm::transaction_processor::TransactionBatchProcessor,
    std::collections::{HashMap, HashSet},
};

mod mock_bank;

fn program_cache_execution(threads: usize) {
    let mut mock_bank = MockBankCallback::default();
    let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new(5, 5, HashSet::new());
    let fork_graph = Arc::new(RwLock::new(MockForkGraph {}));
    batch_processor.program_cache.write().unwrap().fork_graph = Some(Arc::downgrade(&fork_graph));

    let programs = vec![
        deploy_program("hello-solana".to_string(), 0, &mut mock_bank),
        deploy_program("simple-transfer".to_string(), 0, &mut mock_bank),
        deploy_program("clock-sysvar".to_string(), 0, &mut mock_bank),
    ];

    let account_maps: HashMap<Pubkey, u64> = programs
        .iter()
        .enumerate()
        .map(|(idx, key)| (*key, idx as u64))
        .collect();

    let ths: Vec<_> = (0..threads)
        .map(|_| {
            let local_bank = mock_bank.clone();
            let processor = TransactionBatchProcessor::new_from(
                &batch_processor,
                batch_processor.slot,
                batch_processor.epoch,
            );
            let maps = account_maps.clone();
            let programs = programs.clone();
            thread::spawn(move || {
                let result = processor.replenish_program_cache(&local_bank, &maps, false, true);
                for key in &programs {
                    let cache_entry = result.find(key);
                    assert!(matches!(
                        cache_entry.unwrap().program,
                        ProgramCacheEntryType::Loaded(_)
                    ));
                }
            })
        })
        .collect();

    for th in ths {
        th.join().unwrap();
    }
}

// Shuttle has its own internal scheduler and the following tests change the way it operates to
// increase the efficiency in finding problems in the program cache's concurrent code.

// This test leverages the probabilistic concurrency testing algorithm
// (https://www.microsoft.com/en-us/research/wp-content/uploads/2016/02/asplos277-pct.pdf).
// It bounds the numbers of preemptions to explore (five in this test) for the four
// threads we use. We run it for 300 iterations.
#[test]
fn test_program_cache_with_probabilistic_scheduler() {
    shuttle::check_pct(
        move || {
            program_cache_execution(4);
        },
        300,
        5,
    );
}

// In this case, the scheduler is random and may preempt threads at any point and any time.
#[test]
fn test_program_cache_with_random_scheduler() {
    shuttle::check_random(move || program_cache_execution(4), 300);
}

// This test explores all the possible thread scheduling patterns that might affect the program
// cache. There is a limitation to run only 500 iterations to avoid consuming too much CI time.
#[test]
fn test_program_cache_with_exhaustive_scheduler() {
    // The DFS (shuttle::check_dfs) test is only complete when we do not generate random
    // values in a thread.
    // Since this is not the case for the execution of jitted program, we can still run the test
    // but with decreased accuracy.
    let scheduler = shuttle::scheduler::DfsScheduler::new(Some(500), true);
    let runner = Runner::new(scheduler, Default::default());
    runner.run(move || program_cache_execution(4));
}
