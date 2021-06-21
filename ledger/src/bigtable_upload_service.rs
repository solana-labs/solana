use {
    crate::{bigtable_upload, blockstore::Blockstore},
    solana_runtime::commitment::BlockCommitmentCache,
    std::{
        sync::atomic::{AtomicBool, Ordering},
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
    },
    tokio::runtime::Runtime,
};

// Delay uploading the largest confirmed root for this many slots.  This is done in an attempt to
// ensure that the `CacheBlockMetaService` has had enough time to add the block time for the root
// before it's uploaded to BigTable.
//
// A more direct connection between CacheBlockMetaService and BigTableUploadService would be
// preferable...
const LARGEST_CONFIRMED_ROOT_UPLOAD_DELAY: usize = 100;

pub struct BigTableUploadService {
    thread: JoinHandle<()>,
}

impl BigTableUploadService {
    pub fn new(
        runtime: Arc<Runtime>,
        bigtable_ledger_storage: solana_storage_bigtable::LedgerStorage,
        blockstore: Arc<Blockstore>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
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
        exit: Arc<AtomicBool>,
    ) {
        let mut start_slot = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let end_slot = block_commitment_cache
                .read()
                .unwrap()
                .highest_confirmed_root()
                .saturating_sub(LARGEST_CONFIRMED_ROOT_UPLOAD_DELAY as u64);

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
