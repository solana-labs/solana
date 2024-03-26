pub use solana_ledger::blockstore_processor::CacheBlockMetaSender;
use {
    crossbeam_channel::{Receiver, RecvTimeoutError},
    solana_ledger::blockstore::Blockstore,
    solana_measure::measure::Measure,
    solana_runtime::bank::Bank,
    std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread::{self, Builder, JoinHandle},
        time::Duration,
    },
};

pub type CacheBlockMetaReceiver = Receiver<Arc<Bank>>;

pub struct CacheBlockMetaService {
    thread_hdl: JoinHandle<()>,
}

const CACHE_BLOCK_TIME_WARNING_MS: u64 = 150;

impl CacheBlockMetaService {
    pub fn new(
        cache_block_meta_receiver: CacheBlockMetaReceiver,
        blockstore: Arc<Blockstore>,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solCacheBlkTime".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                let recv_result = cache_block_meta_receiver.recv_timeout(Duration::from_secs(1));
                match recv_result {
                    Err(RecvTimeoutError::Disconnected) => {
                        break;
                    }
                    Ok(bank) => {
                        let mut cache_block_meta_timer = Measure::start("cache_block_meta_timer");
                        Self::cache_block_meta(&bank, &blockstore);
                        cache_block_meta_timer.stop();
                        if cache_block_meta_timer.as_ms() > CACHE_BLOCK_TIME_WARNING_MS {
                            warn!(
                                "cache_block_meta operation took: {}ms",
                                cache_block_meta_timer.as_ms()
                            );
                        }
                    }
                    _ => {}
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn cache_block_meta(bank: &Bank, blockstore: &Blockstore) {
        if let Err(e) = blockstore.cache_block_time(bank.slot(), bank.clock().unix_timestamp) {
            error!("cache_block_time failed: slot {:?} {:?}", bank.slot(), e);
        }
        if let Err(e) = blockstore.cache_block_height(bank.slot(), bank.block_height()) {
            error!("cache_block_height failed: slot {:?} {:?}", bank.slot(), e);
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
