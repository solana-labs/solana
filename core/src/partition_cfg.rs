use solana_ledger::leader_schedule_cache::LeaderScheduleCache;
use solana_ledger::shred::Shred;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::timestamp;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

///Configure a partition in the retransmit stage
#[derive(Debug, Clone)]
pub struct Partition {
    pub num_partitions: usize,
    pub my_partition: usize,
    pub partition_start_epoch_length: u64,
    pub partition_duration_ms: u64,
    start_ts: u64,
    leaders: Arc<RwLock<Vec<Pubkey>>>,
    pub done: bool,
}
impl Default for Partition {
    fn default() -> Self {
        Self {
            num_partitions: 0,
            my_partition: 0,
            start_ts: 0,
            leaders: Arc::new(RwLock::new(vec![])),
            partition_duration_ms: 0,
            partition_start_epoch_length: 0,
            done: false,
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct PartitionCfg {
    partitions: Vec<Partition>,
    printed: HashSet<u64>,
}

impl PartitionCfg {
    pub fn new(partitions: Vec<Partition>) -> Self {
        Self {
            partitions,
            printed: HashSet::new(),
        }
    }
    pub fn all_partitions_done(&self) -> bool {
        for partition in &self.partitions {
            if !partition.done {
                return false;
            }
        }
        true
    }
    pub fn is_connected(
        &mut self,
        bank: &Option<Arc<Bank>>,
        leader_schedule_cache: &Arc<LeaderScheduleCache>,
        shred: &Shred,
    ) -> bool {
        if bank.is_none() {
            return true;
        }
        let bank = bank.as_ref().unwrap().clone();
        let slot_leader_pubkey = leader_schedule_cache.slot_leader_at(shred.slot(), Some(&bank));
        let slot_leader_pubkey = slot_leader_pubkey.unwrap_or_default();
        let time = timestamp();
        for p in &mut self.partitions {
            if p.done {
                continue;
            }
            let epoch_schedule = bank.epoch_schedule();
            let epoch = bank.epoch();
            let slots_in_epoch = epoch_schedule.get_slots_in_epoch(epoch);
            let past_start = slots_in_epoch > p.partition_start_epoch_length;
            let before_end = time < (p.start_ts + p.partition_duration_ms);
            if !self.printed.contains(&slots_in_epoch) {
                warn!("PARTITION_TEST: slots_in_epoch: {} wait_for: {} time: {} start_ts: {} part_duration_ms: {}",
                    slots_in_epoch,
                    p.partition_start_epoch_length,
                    time,
                    p.start_ts,
                    p.partition_duration_ms,
                    );
                self.printed.insert(slots_in_epoch);
            }

            if past_start && !before_end {
                p.done = true;
            }
            let is_time = past_start && before_end;
            if !is_time {
                continue;
            }
            warn!("PARTITION_TEST partition time! {}", p.my_partition);
            if p.num_partitions == 0 {
                continue;
            }
            p.start_ts = timestamp();
            if p.leaders.read().unwrap().is_empty() {
                let mut leader_vec = p.leaders.write().unwrap();
                let mut leaders: Vec<Pubkey> = bank.vote_accounts().keys().cloned().collect();
                leaders.sort();
                *leader_vec = leaders;
                warn!("PARTITION_TEST partition enabled {}", p.my_partition);
            }
            let is_connected: bool = {
                let leaders = p.leaders.read().unwrap();
                let start = p.my_partition * leaders.len() / p.num_partitions;
                let partition_size = leaders.len() / p.num_partitions;
                let end = start + partition_size;
                let end = if leaders.len() - end < partition_size {
                    leaders.len()
                } else {
                    end
                };
                let my_leaders: HashSet<_> = leaders[start..end].iter().collect();
                my_leaders.contains(&slot_leader_pubkey)
            };
            if is_connected {
                trace!("PARTITION_TEST connected {}", p.my_partition);
                continue;
            }
            trace!("PARTITION_TEST not connected {}", p.my_partition);
            return false;
        }
        trace!("PARTITION_TEST connected");
        true
    }
}
