use {
    crate::{bigtable_upload, blockstore::Blockstore},
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
        exit: Arc<AtomicBool>,
    ) -> Self {
        info!("Starting BigTable upload service");
        let thread = Builder::new()
            .name("bigtable-upload".to_string())
            .spawn(move || {
                Self::run(
                    runtime,
                    bigtable_ledger_storage,
                    blockstore,
                    block_commitment_cache,
                    max_complete_transaction_status_slot,
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
        exit: Arc<AtomicBool>,
    ) {
        let mut start_slot = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let end_slot = min(
                max_complete_transaction_status_slot.load(Ordering::SeqCst),
                block_commitment_cache.read().unwrap().root(),
            );

            if end_slot <= start_slot {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }

            let result = runtime.block_on(bigtable_upload::upload_confirmed_blocks(
                blockstore.clone(),
                bigtable_ledger_storage.clone(),
                start_slot,
                Some(end_slot),
                true,
                false,
                exit.clone(),
            ));

            match result {
                Ok(()) => start_slot = end_slot,
                Err(err) => {
                    warn!("bigtable: upload_confirmed_blocks: {}", err);
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread.join()
    }
}
