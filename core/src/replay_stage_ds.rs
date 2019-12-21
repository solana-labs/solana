use crate::consensus::StakeLockout;
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use solana_sdk::timing;
use std::collections::HashMap;
use std::time::Instant;

pub struct ReplaySlotStats {
    // Per-slot elapsed time
    pub slot: Slot,
    pub fetch_entries_elapsed: u64,
    pub fetch_entries_fail_elapsed: u64,
    pub entry_verification_elapsed: u64,
    pub replay_elapsed: u64,
    pub replay_start: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct ForkStats {
    pub weight: u128,
    pub fork_weight: u128,
    pub total_staked: u64,
    pub slot: Slot,
    pub block_height: u64,
    pub has_voted: bool,
    pub is_recent: bool,
    pub is_empty: bool,
    pub vote_threshold: bool,
    pub is_locked_out: bool,
    pub stake_lockouts: HashMap<u64, StakeLockout>,
    pub computed: bool,
    pub confirmation_reported: bool,
}

impl ReplaySlotStats {
    pub fn new(slot: Slot) -> Self {
        Self {
            slot,
            fetch_entries_elapsed: 0,
            fetch_entries_fail_elapsed: 0,
            entry_verification_elapsed: 0,
            replay_elapsed: 0,
            replay_start: Instant::now(),
        }
    }

    pub fn report_stats(&self, total_entries: usize, total_shreds: usize) {
        datapoint_info!(
            "replay-slot-stats",
            ("slot", self.slot as i64, i64),
            ("fetch_entries_time", self.fetch_entries_elapsed as i64, i64),
            (
                "fetch_entries_fail_time",
                self.fetch_entries_fail_elapsed as i64,
                i64
            ),
            (
                "entry_verification_time",
                self.entry_verification_elapsed as i64,
                i64
            ),
            ("replay_time", self.replay_elapsed as i64, i64),
            (
                "replay_total_elapsed",
                self.replay_start.elapsed().as_micros() as i64,
                i64
            ),
            ("total_entries", total_entries as i64, i64),
            ("total_shreds", total_shreds as i64, i64),
        );
    }
}

pub struct ForkProgress {
    pub last_entry: Hash,
    pub num_shreds: usize,
    pub num_entries: usize,
    pub tick_hash_count: u64,
    pub started_ms: u64,
    pub is_dead: bool,
    pub stats: ReplaySlotStats,
    pub fork_stats: ForkStats,
}

impl ForkProgress {
    pub fn new(slot: Slot, last_entry: Hash) -> Self {
        Self {
            last_entry,
            num_shreds: 0,
            num_entries: 0,
            tick_hash_count: 0,
            started_ms: timing::timestamp(),
            is_dead: false,
            stats: ReplaySlotStats::new(slot),
            fork_stats: ForkStats::default(),
        }
    }
}
