use {
    crate::cluster_nodes::ClusterNodesCache,
    crossbeam_channel::{Receiver, RecvTimeoutError, Sender},
    rayon::{prelude::*, ThreadPool},
    solana_gossip::cluster_info::ClusterInfo,
    solana_perf::packet::PacketBatch,
    solana_rayon_threadlimit::get_thread_count,
    solana_runtime::bank_forks::BankForks,
    solana_streamer::streamer::{self, StreamerError},
    std::{
        cell::RefCell,
        collections::HashMap,
        net::IpAddr,
        sync::{Arc, RwLock},
        thread::{self, Builder, JoinHandle},
        time::{Duration, Instant},
    },
};

const CLUSTER_NODES_CACHE_NUM_EPOCH_CAP: usize = 8;
const CLUSTER_NODES_CACHE_TTL: Duration = Duration::from_secs(5);
const STAKES_REFRESH_PERIOD_IN_MS: u128 = 1000;

thread_local!(static PAR_THREAD_POOL: RefCell<ThreadPool> = RefCell::new(rayon::ThreadPoolBuilder::new()
                    .num_threads(get_thread_count())
                    .thread_name(|ix| format!("transaction_sender_stake_stage_{}", ix))
                    .build()
                    .unwrap()));

pub struct FindPacketSenderStakeStage {
    thread_hdl: JoinHandle<()>,
}

impl FindPacketSenderStakeStage {
    pub fn new(
        packet_receiver: Receiver<PacketBatch>,
        sender: Sender<Vec<PacketBatch>>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: Arc<ClusterInfo>,
    ) -> Self {
        let cluster_nodes_cache = Arc::new(ClusterNodesCache::<FindPacketSenderStakeStage>::new(
            CLUSTER_NODES_CACHE_NUM_EPOCH_CAP,
            CLUSTER_NODES_CACHE_TTL,
        ));
        let thread_hdl = Builder::new()
            .name("sol-tx-sender_stake".to_string())
            .spawn(move || {
                let mut last_stakes = Instant::now();
                let mut ip_to_stake: HashMap<IpAddr, u64> = HashMap::new();
                loop {
                    if last_stakes.elapsed().as_millis() > STAKES_REFRESH_PERIOD_IN_MS {
                        let (root_bank, working_bank) = {
                            let bank_forks = bank_forks.read().unwrap();
                            (bank_forks.root_bank(), bank_forks.working_bank())
                        };
                        ip_to_stake = cluster_nodes_cache
                            .get(root_bank.slot(), &root_bank, &working_bank, &cluster_info)
                            .get_ip_to_stakes();
                        last_stakes = Instant::now();
                    }
                    match streamer::recv_packet_batches(&packet_receiver) {
                        Ok((mut batches, _num_packets, _recv_duration)) => {
                            Self::apply_sender_stakes(&mut batches, &ip_to_stake);
                            if let Err(e) = sender.send(batches) {
                                info!("Sender error: {:?}", e);
                            }
                        }
                        Err(e) => match e {
                            StreamerError::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                            StreamerError::RecvTimeout(RecvTimeoutError::Timeout) => (),
                            _ => error!("error: {:?}", e),
                        },
                    }
                }
            })
            .unwrap();
        Self { thread_hdl }
    }

    fn apply_sender_stakes(batches: &mut [PacketBatch], ip_to_stake: &HashMap<IpAddr, u64>) {
        PAR_THREAD_POOL.with(|thread_pool| {
            thread_pool.borrow().install(|| {
                batches
                    .into_par_iter()
                    .flat_map(|batch| batch.packets.par_iter_mut())
                    .for_each(|packet| {
                        packet.meta.sender_stake =
                            *ip_to_stake.get(&packet.meta.addr().ip()).unwrap_or(&0);
                    });
            })
        });
    }

    pub fn join(self) -> thread::Result<()> {
        self.thread_hdl.join()
    }
}
