use solana_runtime::vote_sender_types::ReplayVoteSender;
use std::sync::Arc;
use solana_runtime::prioritization_fee_cache::PrioritizationFeeCache;
use solana_ledger::blockstore_processor::TransactionStatusSender;
use solana_runtime::installed_scheduler_pool::InstalledScheduler;

#[derive(Debug)]
pub struct SchedulerPool {
    schedulers: std::sync::Mutex<Vec<Box<dyn InstalledScheduler>>>,
    log_messages_bytes_limit: Option<usize>,
    transaction_status_sender: Option<TransactionStatusSender>,
    replay_vote_sender: Option<ReplayVoteSender>,
    prioritization_fee_cache: Arc<PrioritizationFeeCache>,
}
