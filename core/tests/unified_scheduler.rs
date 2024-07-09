use {
    crossbeam_channel::unbounded,
    itertools::Itertools,
    log::*,
    solana_core::{
        consensus::{
            heaviest_subtree_fork_choice::HeaviestSubtreeForkChoice,
            progress_map::{ForkProgress, ProgressMap},
        },
        drop_bank_service::DropBankService,
        repair::cluster_slot_state_verifier::{
            DuplicateConfirmedSlots, DuplicateSlotsTracker, EpochSlotsFrozenSlots,
        },
        replay_stage::ReplayStage,
        unfrozen_gossip_verified_vote_hashes::UnfrozenGossipVerifiedVoteHashes,
    },
    solana_ledger::genesis_utils::create_genesis_config,
    solana_program_runtime::timings::ExecuteTimings,
    solana_runtime::{
        accounts_background_service::AbsRequestSender, bank::Bank, bank_forks::BankForks,
        genesis_utils::GenesisConfigInfo, prioritization_fee_cache::PrioritizationFeeCache,
    },
    solana_sdk::{
        hash::Hash,
        pubkey::Pubkey,
        system_transaction,
        transaction::{Result, SanitizedTransaction},
    },
    solana_unified_scheduler_pool::{
        DefaultTaskHandler, HandlerContext, PooledScheduler, SchedulerPool, TaskHandler,
    },
    std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    },
};

#[test]
fn test_scheduler_waited_by_drop_bank_service() {
    solana_logger::setup();

    static LOCK_TO_STALL: Mutex<()> = Mutex::new(());

    #[derive(Debug)]
    struct StallingHandler;
    impl TaskHandler for StallingHandler {
        fn handle(
            result: &mut Result<()>,
            timings: &mut ExecuteTimings,
            bank: &Arc<Bank>,
            transaction: &SanitizedTransaction,
            index: usize,
            handler_context: &HandlerContext,
        ) {
            info!("Stalling at StallingHandler::handle()...");
            *LOCK_TO_STALL.lock().unwrap();
            // Wait a bit for the replay stage to prune banks
            std::thread::sleep(std::time::Duration::from_secs(3));
            info!("Now entering into DefaultTaskHandler::handle()...");

            DefaultTaskHandler::handle(result, timings, bank, transaction, index, handler_context);
        }
    }

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(10_000);

    // Setup bankforks with unified scheduler enabled
    let genesis_bank = Bank::new_for_tests(&genesis_config);
    let bank_forks = BankForks::new_rw_arc(genesis_bank);
    let ignored_prioritization_fee_cache = Arc::new(PrioritizationFeeCache::new(0u64));
    let pool_raw = SchedulerPool::<PooledScheduler<StallingHandler>, _>::new(
        None,
        None,
        None,
        None,
        ignored_prioritization_fee_cache,
    );
    let pool = pool_raw.clone();
    bank_forks.write().unwrap().install_scheduler_pool(pool);
    let genesis = 0;
    let genesis_bank = &bank_forks.read().unwrap().get(genesis).unwrap();
    genesis_bank.set_fork_graph_in_program_cache(Arc::downgrade(&bank_forks));

    // Create bank, which is pruned later
    let pruned = 2;
    let pruned_bank = Bank::new_from_parent(genesis_bank.clone(), &Pubkey::default(), pruned);
    let pruned_bank = bank_forks.write().unwrap().insert(pruned_bank);

    // Create new root bank
    let root = 3;
    let root_bank = Bank::new_from_parent(genesis_bank.clone(), &Pubkey::default(), root);
    root_bank.freeze();
    let root_hash = root_bank.hash();
    bank_forks.write().unwrap().insert(root_bank);

    let tx = SanitizedTransaction::from_transaction_for_tests(system_transaction::transfer(
        &mint_keypair,
        &solana_sdk::pubkey::new_rand(),
        2,
        genesis_config.hash(),
    ));

    // Delay transaction execution to ensure transaction execution happens after termintion has
    // been started
    let lock_to_stall = LOCK_TO_STALL.lock().unwrap();
    pruned_bank
        .schedule_transaction_executions([(&tx, &0)].into_iter())
        .unwrap();
    drop(pruned_bank);
    assert_eq!(pool_raw.pooled_scheduler_count(), 0);
    drop(lock_to_stall);

    // Create 2 channels to check actual pruned banks
    let (drop_bank_sender1, drop_bank_receiver1) = unbounded();
    let (drop_bank_sender2, drop_bank_receiver2) = unbounded();
    let drop_bank_service = DropBankService::new(drop_bank_receiver2);

    info!("calling handle_new_root()...");
    // Mostly copied from: test_handle_new_root()
    {
        let mut heaviest_subtree_fork_choice = HeaviestSubtreeForkChoice::new((root, root_hash));

        let mut progress = ProgressMap::default();
        for i in genesis..=root {
            progress.insert(i, ForkProgress::new(Hash::default(), None, None, 0, 0));
        }

        let mut duplicate_slots_tracker: DuplicateSlotsTracker =
            vec![root - 1, root, root + 1].into_iter().collect();
        let mut duplicate_confirmed_slots: DuplicateConfirmedSlots = vec![root - 1, root, root + 1]
            .into_iter()
            .map(|s| (s, Hash::default()))
            .collect();
        let mut unfrozen_gossip_verified_vote_hashes: UnfrozenGossipVerifiedVoteHashes =
            UnfrozenGossipVerifiedVoteHashes {
                votes_per_slot: vec![root - 1, root, root + 1]
                    .into_iter()
                    .map(|s| (s, HashMap::new()))
                    .collect(),
            };
        let mut epoch_slots_frozen_slots: EpochSlotsFrozenSlots = vec![root - 1, root, root + 1]
            .into_iter()
            .map(|slot| (slot, Hash::default()))
            .collect();
        ReplayStage::handle_new_root(
            root,
            &bank_forks,
            &mut progress,
            &AbsRequestSender::default(),
            None,
            &mut heaviest_subtree_fork_choice,
            &mut duplicate_slots_tracker,
            &mut duplicate_confirmed_slots,
            &mut unfrozen_gossip_verified_vote_hashes,
            &mut true,
            &mut Vec::new(),
            &mut epoch_slots_frozen_slots,
            &drop_bank_sender1,
        )
        .unwrap();
    }

    // Receive pruned banks from the above handle_new_root
    let pruned_banks = drop_bank_receiver1.recv().unwrap();
    assert_eq!(
        pruned_banks
            .iter()
            .map(|b| b.slot())
            .sorted()
            .collect::<Vec<_>>(),
        vec![genesis, pruned]
    );
    info!("sending pruned banks to DropBankService...");
    drop_bank_sender2.send(pruned_banks).unwrap();

    info!("joining the drop bank service...");
    drop((
        (drop_bank_sender1, drop_bank_receiver1),
        (drop_bank_sender2,),
    ));
    drop_bank_service.join().unwrap();
    info!("finally joined the drop bank service!");

    // the scheduler used by the pruned_bank have been returned now.
    assert_eq!(pool_raw.pooled_scheduler_count(), 1);
}
