//! Manages a set of recent invalid fee payers.
//! This is used to filter out transactions that are unlikely to be included
//! in any block, much earlier in the transaction pipeline.
//!

use {
    dashmap::DashSet,
    solana_sdk::pubkey::Pubkey,
    std::{
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread::{sleep, Builder, JoinHandle},
        time::Duration,
    },
};

#[derive(Default)]
pub struct InvalidFeePayerFilter {
    recent_invalid_fee_payers: DashSet<Pubkey>,
    reject_count: Arc<AtomicUsize>,
}

impl InvalidFeePayerFilter {
    /// Reset the filter - this should be called periodically.
    pub fn reset(&self) {
        self.recent_invalid_fee_payers.clear();
    }

    /// Add a pubkey to the filter.
    pub fn add(&self, pubkey: Pubkey) {
        self.recent_invalid_fee_payers.insert(pubkey);
    }

    /// Check if a pubkey is in the filter.
    pub fn should_reject(&self, pubkey: &Pubkey) -> bool {
        let should_reject = self.recent_invalid_fee_payers.contains(pubkey);
        if should_reject {
            self.reject_count.fetch_add(1, Ordering::Relaxed);
        }
        should_reject
    }
}

/// Simple wrapper thread that periodically resets the filter.
pub struct InvalidFeePayerFilterThread {
    thread_hdl: JoinHandle<()>,
}

impl InvalidFeePayerFilterThread {
    pub fn new(exit: Arc<AtomicBool>) -> (Self, Arc<InvalidFeePayerFilter>) {
        let invalid_fee_payer_filter = Arc::new(InvalidFeePayerFilter::default());
        let reject_count = invalid_fee_payer_filter.reject_count.clone();

        let thread_hdl = Builder::new()
            .name("solInvFeePay".to_string())
            .spawn({
                let invalid_fee_payer_filter = invalid_fee_payer_filter.clone();
                move || {
                    const RESET_INTERVAL: Duration = Duration::from_secs(120);
                    while !exit.load(Ordering::Relaxed) {
                        let num_rejects = reject_count.swap(0, Ordering::Relaxed);
                        let num_invalid_fee_payers =
                            invalid_fee_payer_filter.recent_invalid_fee_payers.len();

                        if num_rejects > 0 {
                            datapoint_info!(
                                "invalid_fee_payer_filter",
                                ("invalid_fee_payers", num_invalid_fee_payers, i64),
                                ("rejects", num_rejects, i64),
                            );
                        }

                        invalid_fee_payer_filter.reset();
                        sleep(RESET_INTERVAL);
                    }
                }
            })
            .unwrap();
        (Self { thread_hdl }, invalid_fee_payer_filter)
    }

    pub fn join(self) -> std::thread::Result<()> {
        self.thread_hdl.join()
    }
}
