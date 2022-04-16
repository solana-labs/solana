use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_runtime::bank_forks::BankForks,
    std::{
        collections::HashMap,
        net::IpAddr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
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
        shared_staked_nodes: Arc<RwLock<HashMap<IpAddr, u64>>>,
    ) -> Self {
        let thread_hdl = Builder::new()
            .name("sol-sn-updater".to_string())
            .spawn(move || {
                let mut last_stakes = Instant::now();
                while !exit.load(Ordering::Relaxed) {
                    let mut new_ip_to_stake = HashMap::new();
                    Self::try_refresh_ip_to_stake(
                        &mut last_stakes,
                        &mut new_ip_to_stake,
                        &bank_forks,
                        &cluster_info,
                    );
                    let mut shared = shared_staked_nodes.write().unwrap();
                    *shared = new_ip_to_stake;
                }
            })
            .unwrap();

        Self { thread_hdl }
    }

    fn try_refresh_ip_to_stake(
        last_stakes: &mut Instant,
        ip_to_stake: &mut HashMap<IpAddr, u64>,
        bank_forks: &RwLock<BankForks>,
        cluster_info: &ClusterInfo,
    ) {
        if last_stakes.elapsed() > IP_TO_STAKE_REFRESH_DURATION {
            let root_bank = bank_forks.read().unwrap().root_bank();
            let staked_nodes = root_bank.staked_nodes();
            *ip_to_stake = cluster_info
                .tvu_peers()
                .into_iter()
                .filter_map(|node| {
                    let stake = staked_nodes.get(&node.id)?;
                    Some((node.tvu.ip(), *stake))
                })
                .collect();
            *last_stakes = Instant::now();
        } else {
            sleep(Duration::from_millis(1));
        }
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
