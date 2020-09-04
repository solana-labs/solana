use solana_ledger::blockstore::Blockstore;
use solana_runtime::commitment::BlockCommitmentCache;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, RwLock},
    thread::{self, Builder, JoinHandle},
};
use tokio::runtime;

pub struct BigTableUploadService {
    thread: JoinHandle<()>,
}

impl BigTableUploadService {
    pub fn new(
        runtime_handle: runtime::Handle,
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
                    runtime_handle,
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
        runtime: runtime::Handle,
        bigtable_ledger_storage: solana_storage_bigtable::LedgerStorage,
        blockstore: Arc<Blockstore>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        exit: Arc<AtomicBool>,
    ) {
        let mut starting_slot = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let max_confirmed_root = block_commitment_cache
                .read()
                .unwrap()
                .highest_confirmed_root();

            if max_confirmed_root == starting_slot {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }

            let result = runtime.block_on(solana_ledger::bigtable_upload::upload_confirmed_blocks(
                blockstore.clone(),
                bigtable_ledger_storage.clone(),
                starting_slot,
                Some(max_confirmed_root),
                true,
            ));

            match result {
                Ok(()) => starting_slot = max_confirmed_root,
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
