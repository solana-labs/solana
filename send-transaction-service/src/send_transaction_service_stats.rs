use {
    solana_sdk::timing::AtomicInterval,
    std::sync::atomic::{AtomicU64, Ordering},
};

/// Report the send transaction metrics for every 5 seconds.
const SEND_TRANSACTION_METRICS_REPORT_RATE_MS: u64 = 5000;

/// Metrics of the send-transaction-service.
#[derive(Default)]
pub struct SendTransactionServiceStats {
    /// Count of the received transactions
    pub received_transactions: AtomicU64,

    /// Count of the received duplicate transactions
    pub received_duplicate_transactions: AtomicU64,

    /// Count of transactions sent in batch
    pub sent_transactions: AtomicU64,

    /// Count of transactions not being added to retry queue
    /// due to queue size limit
    pub retry_queue_overflow: AtomicU64,

    /// retry queue size
    pub retry_queue_size: AtomicU64,

    /// The count of calls of sending transactions which can be in batch or single.
    pub send_attempt_count: AtomicU64,

    /// Time spent on transactions in micro seconds
    pub send_us: AtomicU64,

    /// Send failure count
    pub send_failure_count: AtomicU64,

    /// Count of nonced transactions
    pub nonced_transactions: AtomicU64,

    /// Count of rooted transactions
    pub rooted_transactions: AtomicU64,

    /// Count of expired transactions
    pub expired_transactions: AtomicU64,

    /// Count of transactions exceeding max retries
    pub transactions_exceeding_max_retries: AtomicU64,

    /// Count of retries of transactions
    pub retries: AtomicU64,

    /// Count of transactions failed
    pub failed_transactions: AtomicU64,
}

#[derive(Default)]
pub(crate) struct SendTransactionServiceStatsReport {
    pub stats: SendTransactionServiceStats,
    last_report: AtomicInterval,
}

impl SendTransactionServiceStatsReport {
    /// report metrics of the send transaction service
    pub fn report(&self) {
        if self
            .last_report
            .should_update(SEND_TRANSACTION_METRICS_REPORT_RATE_MS)
        {
            datapoint_info!(
                "send_transaction_service",
                (
                    "recv-tx",
                    self.stats.received_transactions.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "recv-duplicate",
                    self.stats
                        .received_duplicate_transactions
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "sent-tx",
                    self.stats.sent_transactions.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "retry-queue-overflow",
                    self.stats.retry_queue_overflow.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "retry-queue-size",
                    self.stats.retry_queue_size.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "send-us",
                    self.stats.send_us.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "send-attempt-count",
                    self.stats.send_attempt_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "send-failure-count",
                    self.stats.send_failure_count.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "nonced-tx",
                    self.stats.nonced_transactions.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "rooted-tx",
                    self.stats.rooted_transactions.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "expired-tx",
                    self.stats.expired_transactions.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "max-retries-exceeded-tx",
                    self.stats
                        .transactions_exceeding_max_retries
                        .swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "retries",
                    self.stats.retries.swap(0, Ordering::Relaxed),
                    i64
                ),
                (
                    "failed-tx",
                    self.stats.failed_transactions.swap(0, Ordering::Relaxed),
                    i64
                )
            );
        }
    }
}
