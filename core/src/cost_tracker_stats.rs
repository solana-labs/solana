//! The Stats is not thread safe, each thread should have its own
//! instance of stat with `id`; Stat reports and reset for each slot.
#[derive(Debug, Default)]
pub struct CostTrackerStats {
    pub id: u32,
    pub transaction_cost_histogram: histogram::Histogram,
    pub writable_accounts_cost_histogram: histogram::Histogram,
    pub transaction_count: u64,
    pub block_cost: u64,
    pub bank_slot: u64,
}

impl CostTrackerStats {
    pub fn new(id: u32, bank_slot: u64) -> Self {
        CostTrackerStats {
            id,
            bank_slot,
            ..CostTrackerStats::default()
        }
    }

    pub fn report(&self) {
        datapoint_info!(
            "cost_tracker_stats",
            ("id", self.id as i64, i64),
            (
                "transaction_cost_unit_min",
                self.transaction_cost_histogram.minimum().unwrap_or(0),
                i64
            ),
            (
                "transaction_cost_unit_max",
                self.transaction_cost_histogram.maximum().unwrap_or(0),
                i64
            ),
            (
                "transaction_cost_unit_mean",
                self.transaction_cost_histogram.mean().unwrap_or(0),
                i64
            ),
            (
                "transaction_cost_unit_2nd_std",
                self.transaction_cost_histogram
                    .percentile(95.0)
                    .unwrap_or(0),
                i64
            ),
            (
                "writable_accounts_cost_min",
                self.writable_accounts_cost_histogram.minimum().unwrap_or(0),
                i64
            ),
            (
                "writable_accounts_cost_max",
                self.writable_accounts_cost_histogram.maximum().unwrap_or(0),
                i64
            ),
            (
                "writable_accounts_cost_mean",
                self.writable_accounts_cost_histogram.mean().unwrap_or(0),
                i64
            ),
            (
                "writable_accounts_cost_2nd_std",
                self.writable_accounts_cost_histogram
                    .percentile(95.0)
                    .unwrap_or(0),
                i64
            ),
            ("transaction_count", self.transaction_count as i64, i64),
            ("block_cost", self.block_cost as i64, i64),
            ("bank_slot", self.bank_slot as i64, i64),
        );
    }
}
