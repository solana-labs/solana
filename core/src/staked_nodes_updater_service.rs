use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::pubkey::Pubkey,
    solana_streamer::streamer::StakedNodes,
    std::{
        collections::HashMap,
        net::IpAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock, RwLockReadGuard,
        },
        thread::{self, sleep, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const IP_TO_STAKE_REFRESH_DURATION: Duration = Duration::from_secs(5);

pub struct StakedNodesUpdaterService {
    thread_hdl: JoinHandle<()>,
}

impl StakedNodesUpdaterService {
    pub fn new(
        exit: Arc<AtomicBool>,
        cluster_info: Arc<ClusterInfo>,
        bank_forks: Arc<RwLock<BankForks>>,
        shared_staked_nodes: Arc<RwLock<StakedNodes>>,
        shared_staked_nodes_overrides: Arc<RwLock<HashMap<Pubkey, u64>>>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("solStakedNodeUd".to_string())
            .spawn(move || {
                let mut last_stakes = Instant::now();
                while !exit.load(Ordering::Relaxed) {
                    let overrides = shared_staked_nodes_overrides.read().unwrap();
                    let mut new_ip_to_stake = HashMap::new();
                    let mut new_id_to_stake = HashMap::new();
                    let mut total_stake = 0;
                    let mut max_stake: u64 = 0;
                    let mut min_stake: u64 = u64::MAX;
                    if Self::try_refresh_stake_maps(
                        &mut last_stakes,
                        &mut new_ip_to_stake,
                        &mut new_id_to_stake,
                        &mut total_stake,
                        &mut max_stake,
                        &mut min_stake,
                        &bank_forks,
                        &cluster_info,
                        &overrides,
                    ) {
                        let mut shared = shared_staked_nodes.write().unwrap();
                        shared.total_stake = total_stake;
                        shared.ip_stake_map = new_ip_to_stake;
                        shared.pubkey_stake_map = new_id_to_stake;
                    }
                }
            })
            .unwrap();

        Self { thread_hdl }
    }

    fn try_refresh_stake_maps(
        last_stakes: &mut Instant,
        ip_to_stake: &mut HashMap<IpAddr, u64>,
        id_to_stake: &mut HashMap<Pubkey, u64>,
        total_stake: &mut u64,
        max_stake: &mut u64,
        min_stake: &mut u64,
        bank_forks: &RwLock<BankForks>,
        cluster_info: &ClusterInfo,
        overrides: &RwLockReadGuard<HashMap<Pubkey, u64>>,
    ) -> bool {
        if last_stakes.elapsed() > IP_TO_STAKE_REFRESH_DURATION {
            let root_bank = bank_forks.read().unwrap().root_bank();
            let staked_nodes = root_bank.staked_nodes();

            for stake in staked_nodes.values() {
                *total_stake += stake;
                *max_stake = *stake.max(max_stake);
                *min_stake = *stake.min(min_stake);
            }

            *id_to_stake = cluster_info
                .tvu_peers()
                .into_iter()
                .filter_map(|node| {
                    let stake = staked_nodes.get(&node.id)?;
                    Some((node.id, *stake))
                })
                .collect();
            *ip_to_stake = cluster_info
                .tvu_peers()
                .into_iter()
                .filter_map(|node| {
                    let stake = staked_nodes.get(&node.id)?;
                    Some((node.tvu.ip(), *stake))
                })
                .collect();
            Self::override_stake(
                cluster_info,
                total_stake,
                id_to_stake,
                ip_to_stake,
                overrides,
            );

            *last_stakes = Instant::now();
            true
        } else {
            sleep(Duration::from_millis(1));
            false
        }
    }

    fn override_stake(
        cluster_info: &ClusterInfo,
        total_stake: &mut u64,
        id_to_stake_map: &mut HashMap<Pubkey, u64>,
        ip_to_stake_map: &mut HashMap<IpAddr, u64>,
        staked_map_overrides: &HashMap<Pubkey, u64>,
    ) {
        for (id_override, stake_override) in staked_map_overrides.iter() {
            if let Some(ip_override) =
                cluster_info
                    .all_peers()
                    .into_iter()
                    .find_map(|(node, _seen_time)| {
                        if node.id == *id_override {
                            return Some(node.tvu.ip());
                        }
                        None
                    })
            {
                if let Some(previous_stake) = id_to_stake_map.get(id_override) {
                    *total_stake -= previous_stake;
                }
                *total_stake += stake_override;
                id_to_stake_map.insert(*id_override, *stake_override);
                ip_to_stake_map.insert(ip_override, *stake_override);
            } else {
                error!(
                        "staked nodes overrides configuration for id {} with stake {} does not match existing IP. Skipping",
                        id_override, stake_override
                    );
            }
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
