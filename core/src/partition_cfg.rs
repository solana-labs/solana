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
    pub start_ts: u64,
    pub end_ts: u64,
    leaders: Arc<RwLock<Vec<Pubkey>>>,
}
impl Default for Partition {
    fn default() -> Self {
        Self {
            num_partitions: 0,
            my_partition: 0,
            start_ts: 0,
            end_ts: 0,
            leaders: Arc::new(RwLock::new(vec![])),
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct PartitionCfg {
    partitions: Vec<Partition>,
}

impl PartitionCfg {
    pub fn new(partitions: Vec<Partition>) -> Self {
        Self { partitions }
    }
    pub fn is_connected(
        &self,
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
        for p in &self.partitions {
            let is_time = (p.start_ts <= time) && (time < p.end_ts);
            if !is_time {
                continue;
            }
            trace!("PARTITION_TEST partition time! {}", p.my_partition);
            if p.num_partitions == 0 {
                continue;
            }
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
