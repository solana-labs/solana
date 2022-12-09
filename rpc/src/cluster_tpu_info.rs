use {
    solana_gossip::cluster_info::ClusterInfo,
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{clock::NUM_CONSECUTIVE_LEADER_SLOTS, pubkey::Pubkey},
    solana_send_transaction_service::tpu_info::TpuInfo,
    std::{
        collections::HashMap,
        net::SocketAddr,
        sync::{Arc, RwLock},
    },
};

#[derive(Clone)]
pub struct ClusterTpuInfo {
    cluster_info: Arc<ClusterInfo>,
    poh_recorder: Arc<RwLock<PohRecorder>>,
    recent_peers: HashMap<Pubkey, SocketAddr>,
}

impl ClusterTpuInfo {
    pub fn new(cluster_info: Arc<ClusterInfo>, poh_recorder: Arc<RwLock<PohRecorder>>) -> Self {
        Self {
            cluster_info,
            poh_recorder,
            recent_peers: HashMap::new(),
        }
    }
}

impl TpuInfo for ClusterTpuInfo {
    fn refresh_recent_peers(&mut self) {
        self.recent_peers = self
            .cluster_info
            .tpu_peers()
            .into_iter()
            .map(|ci| (ci.id, ci.tpu))
            .collect();
    }

    fn get_leader_tpus(&self, max_count: u64) -> Vec<&SocketAddr> {
        let recorder = self.poh_recorder.read().unwrap();
        let leaders: Vec<_> = (0..max_count)
            .filter_map(|i| recorder.leader_after_n_slots(i * NUM_CONSECUTIVE_LEADER_SLOTS))
            .collect();
        drop(recorder);
        let mut unique_leaders = vec![];
        for leader in leaders.iter() {
            if let Some(addr) = self.recent_peers.get(leader) {
                if !unique_leaders.contains(&addr) {
                    unique_leaders.push(addr);
                }
            }
        }
        unique_leaders
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_gossip::contact_info::ContactInfo,
        solana_ledger::{
            blockstore::Blockstore, get_tmp_ledger_path, leader_schedule_cache::LeaderScheduleCache,
        },
        solana_runtime::{
            bank::Bank,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{
            poh_config::PohConfig,
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::sync::atomic::AtomicBool,
    };

    #[test]
    fn test_get_leader_tpus() {
        let ledger_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&ledger_path).unwrap();

            let validator_vote_keypairs0 = ValidatorVoteKeypairs::new_rand();
            let validator_vote_keypairs1 = ValidatorVoteKeypairs::new_rand();
            let validator_vote_keypairs2 = ValidatorVoteKeypairs::new_rand();
            let validator_keypairs = vec![
                &validator_vote_keypairs0,
                &validator_vote_keypairs1,
                &validator_vote_keypairs2,
            ];
            let GenesisConfigInfo { genesis_config, .. } = create_genesis_config_with_vote_accounts(
                1_000_000_000,
                &validator_keypairs,
                vec![10_000; 3],
            );
            let bank = Arc::new(Bank::new_for_tests(&genesis_config));

            let (poh_recorder, _entry_receiver, _record_receiver) = PohRecorder::new(
                0,
                bank.last_blockhash(),
                bank.clone(),
                Some((2, 2)),
                bank.ticks_per_slot(),
                &Pubkey::default(),
                &Arc::new(blockstore),
                &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
                &PohConfig::default(),
                Arc::new(AtomicBool::default()),
            );

            let node_keypair = Arc::new(Keypair::new());
            let cluster_info = Arc::new(ClusterInfo::new(
                ContactInfo::new_localhost(&node_keypair.pubkey(), timestamp()),
                node_keypair,
                SocketAddrSpace::Unspecified,
            ));

            let validator0_socket = SocketAddr::from(([127, 0, 0, 1], 1111));
            let validator1_socket = SocketAddr::from(([127, 0, 0, 1], 2222));
            let validator2_socket = SocketAddr::from(([127, 0, 0, 1], 3333));
            let recent_peers: HashMap<_, _> = vec![
                (
                    validator_vote_keypairs0.node_keypair.pubkey(),
                    validator0_socket,
                ),
                (
                    validator_vote_keypairs1.node_keypair.pubkey(),
                    validator1_socket,
                ),
                (
                    validator_vote_keypairs2.node_keypair.pubkey(),
                    validator2_socket,
                ),
            ]
            .iter()
            .cloned()
            .collect();
            let leader_info = ClusterTpuInfo {
                cluster_info,
                poh_recorder: Arc::new(RwLock::new(poh_recorder)),
                recent_peers: recent_peers.clone(),
            };

            let slot = bank.slot();
            let first_leader =
                solana_ledger::leader_schedule_utils::slot_leader_at(slot, &bank).unwrap();
            assert_eq!(
                leader_info.get_leader_tpus(1),
                vec![recent_peers.get(&first_leader).unwrap()]
            );

            let second_leader = solana_ledger::leader_schedule_utils::slot_leader_at(
                slot + NUM_CONSECUTIVE_LEADER_SLOTS,
                &bank,
            )
            .unwrap();
            let mut expected_leader_sockets = vec![
                recent_peers.get(&first_leader).unwrap(),
                recent_peers.get(&second_leader).unwrap(),
            ];
            expected_leader_sockets.dedup();
            assert_eq!(leader_info.get_leader_tpus(2), expected_leader_sockets);

            let third_leader = solana_ledger::leader_schedule_utils::slot_leader_at(
                slot + (2 * NUM_CONSECUTIVE_LEADER_SLOTS),
                &bank,
            )
            .unwrap();
            let mut expected_leader_sockets = vec![
                recent_peers.get(&first_leader).unwrap(),
                recent_peers.get(&second_leader).unwrap(),
                recent_peers.get(&third_leader).unwrap(),
            ];
            expected_leader_sockets.dedup();
            assert_eq!(leader_info.get_leader_tpus(3), expected_leader_sockets);

            for x in 4..8 {
                assert!(leader_info.get_leader_tpus(x).len() <= recent_peers.len());
            }
        }
        Blockstore::destroy(&ledger_path).unwrap();
    }
}
