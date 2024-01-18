use {
    crate::{
        bigtable_upload::{self, ConfirmedBlockUploadConfig},
        blockstore::Blockstore,
    },
    solana_runtime::commitment::BlockCommitmentCache,
    std::{
        cmp::min,
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, RwLock,
        },
        thread::{self, Builder, JoinHandle},
    },
    tokio::runtime::Runtime,
};

pub struct BigTableUploadService {
    thread: JoinHandle<()>,
}

impl BigTableUploadService {
    pub fn new(
        runtime: Arc<Runtime>,
        bigtable_ledger_storage: solana_storage_bigtable::LedgerStorage,
        blockstore: Arc<Blockstore>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        max_complete_transaction_status_slot: Arc<AtomicU64>,
        max_complete_rewards_slot: Arc<AtomicU64>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        Self::new_with_config(
            runtime,
            bigtable_ledger_storage,
            blockstore,
            block_commitment_cache,
            max_complete_transaction_status_slot,
            max_complete_rewards_slot,
            ConfirmedBlockUploadConfig::default(),
            exit,
        )
    }

    pub fn new_with_config(
        runtime: Arc<Runtime>,
        bigtable_ledger_storage: solana_storage_bigtable::LedgerStorage,
        blockstore: Arc<Blockstore>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        max_complete_transaction_status_slot: Arc<AtomicU64>,
        max_complete_rewards_slot: Arc<AtomicU64>,
        config: ConfirmedBlockUploadConfig,
        exit: Arc<AtomicBool>,
    ) -> Self {
        info!("Starting BigTable upload service");
        let thread = Builder::new()
            .name("solBigTUpload".to_string())
            .spawn(move || {
                Self::run(
                    runtime,
                    bigtable_ledger_storage,
                    blockstore,
                    block_commitment_cache,
                    max_complete_transaction_status_slot,
                    max_complete_rewards_slot,
                    config,
                    exit,
                )
            })
            .unwrap();

        Self { thread }
    }

    fn run(
        runtime: Arc<Runtime>,
        bigtable_ledger_storage: solana_storage_bigtable::LedgerStorage,
        blockstore: Arc<Blockstore>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        max_complete_transaction_status_slot: Arc<AtomicU64>,
        max_complete_rewards_slot: Arc<AtomicU64>,
        config: ConfirmedBlockUploadConfig,
        exit: Arc<AtomicBool>,
    ) {
        let mut start_slot = blockstore.get_first_available_block().unwrap_or_default();
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            // The highest slot eligible for upload is the highest root that has complete
            // transaction-status metadata and rewards
            let highest_complete_root = [
                max_complete_transaction_status_slot.load(Ordering::SeqCst),
                max_complete_rewards_slot.load(Ordering::SeqCst),
                block_commitment_cache.read().unwrap().root(),
            ]
            .into_iter()
            .min()
            .expect("root and max_complete slots exist");
            let end_slot = min(
                highest_complete_root,
                start_slot.saturating_add(config.max_num_slots_to_check as u64 * 2),
            );

            if end_slot <= start_slot {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }

            let result = runtime.block_on(bigtable_upload::upload_confirmed_blocks(
                blockstore.clone(),
                bigtable_ledger_storage.clone(),
                start_slot,
                end_slot,
                config.clone(),
                exit.clone(),
            ));

            match result {
                Ok(last_slot_uploaded) => start_slot = last_slot_uploaded.saturating_add(1),
                Err(err) => {
                    warn!("bigtable: upload_confirmed_blocks: {}", err);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    if start_slot == 0 {
                        start_slot = blockstore.get_first_available_block().unwrap_or_default();
                    }
                }
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread.join()
    }
}
