use {
    crate::mock_bank::{deploy_program, MockForkGraph},
    mock_bank::MockBankCallback,
    solana_program_runtime::loaded_programs::ProgramCacheEntryType,
    solana_sdk::pubkey::Pubkey,
    solana_svm::transaction_processor::TransactionBatchProcessor,
    solana_type_overrides::sync::RwLock,
    std::{
        collections::{HashMap, HashSet},
        sync::Arc,
        thread,
    },
};

mod mock_bank;

#[test]
fn fast_concur_test() {
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

    for _ in 0..10 {
        let ths: Vec<_> = (0..4)
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
}
