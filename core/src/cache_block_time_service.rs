use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};
use solana_ledger::blockstore::Blockstore;
use solana_runtime::bank::Bank;
use solana_sdk::timing::slot_duration_from_slots_per_year;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, Builder, JoinHandle},
    time::Duration,
};

pub type CacheBlockTimeReceiver = Receiver<Arc<Bank>>;
pub type CacheBlockTimeSender = Sender<Arc<Bank>>;

pub struct CacheBlockTimeService {
    thread_hdl: JoinHandle<()>,
}

impl CacheBlockTimeService {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        cache_block_time_receiver: CacheBlockTimeReceiver,
        blockstore: Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let exit = exit.clone();
        let thread_hdl = Builder::new()
            .name("solana-cache-block-time".to_string())
            .spawn(move || loop {
                if exit.load(Ordering::Relaxed) {
                    break;
                }
                if let Err(RecvTimeoutError::Disconnected) =
                    Self::cache_block_time(&cache_block_time_receiver, &blockstore)
                {
                    break;
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn cache_block_time(
        cache_block_time_receiver: &CacheBlockTimeReceiver,
        blockstore: &Arc<Blockstore>,
    ) -> Result<(), RecvTimeoutError> {
        let bank = cache_block_time_receiver.recv_timeout(Duration::from_secs(1))?;
        let slot_duration = slot_duration_from_slots_per_year(bank.slots_per_year());
        let epoch = bank.epoch_schedule().get_epoch(bank.slot());
        let stakes = HashMap::new();
        let stakes = bank.epoch_vote_accounts(epoch).unwrap_or(&stakes);

        if let Err(e) = blockstore.cache_block_time(bank.slot(), slot_duration, stakes) {
            error!("cache_block_time failed: slot {:?} {:?}", bank.slot(), e);
        }
        Ok(())
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
