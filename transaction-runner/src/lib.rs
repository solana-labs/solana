use {
    log::*,
}
use std::sync::Arc;
use solana_runtime::bank::Bank;
use solana_runtime::bank::LikeScheduler;
use solana_runtime::bank::ArcPool;
use solana_runtime::bank::LikePool;

struct TransactionRunner(Arc<Bank>, solana_poh::poh_recorder::PohRecorder, solana_ledger::blockstore_processor::TransactionStatusSender);


#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: Vec<Box<dyn LikeScheduler>>,
}

impl SchedulerPool {
    pub const fn new() -> Self {
        Self {
            schedulers: Vec::new(),
        }
    }

    fn create(&mut self, runner: ArcPool) {
        self.schedulers.push(Box::new(Scheduler::default2(runner)));
    }

    pub fn take_from_pool(&mut self, runner: ArcPool) -> Box<dyn LikeScheduler> {
        if let Some(scheduler) = self.schedulers.pop() {
            trace!(
                "SchedulerPool: id_{:016x} is taken... len: {} => {}",
                scheduler.random_id(),
                self.schedulers.len() + 1,
                self.schedulers.len()
            );
            scheduler
        } else {
            self.create(runner.clone());
            self.take_from_pool(runner)
        }
    }

    pub fn return_to_pool(&mut self, scheduler: Box<dyn LikeScheduler>) {
        trace!(
            "SchedulerPool: id_{:016x} is returned... len: {} => {}",
            scheduler.random_id(),
            self.schedulers.len(),
            self.schedulers.len() + 1
        );
        assert!(scheduler.collected_results().lock().unwrap().is_empty());
        //assert!(scheduler.current_checkpoint.clone_context_value().unwrap().bank.is_none());
        assert!(scheduler
            .graceful_stop_initiated()
            .load(std::sync::atomic::Ordering::SeqCst));

        scheduler
            .graceful_stop_initiated()
            .store(false, std::sync::atomic::Ordering::SeqCst);

        self.schedulers.push(scheduler);
    }
}

impl Drop for SchedulerPool {
    fn drop(&mut self) {
        let current_thread_name = std::thread::current().name().unwrap().to_string();
        warn!("SchedulerPool::drop() by {}...", current_thread_name);
        todo!();
        //info!("Scheduler::drop(): id_{:016x} begin..", self.random_id);
        //self.gracefully_stop().unwrap();
        //info!("Scheduler::drop(): id_{:016x} end...", self.random_id);
    }
}

impl LikePool for SchedulerPool {
    fn take_from_pool(&mut self, runner: ArcPool) -> Box<dyn LikeScheduler> {
        panic!();
    }

    fn return_to_pool(&mut self, scheduler: Box<dyn LikeScheduler>) {
        panic!();
    }
}
