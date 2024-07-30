use {
    solana_gossip::{cluster_info::ClusterInfo, contact_info::Protocol},
    solana_poh::poh_recorder::PohRecorder,
    solana_sdk::{
        clock::{Slot, NUM_CONSECUTIVE_LEADER_SLOTS},
        pubkey::Pubkey,
    },
    solana_send_transaction_service::tpu_info::TpuInfo,
    std::{
        collections::HashMap,
        iter::once,
        net::SocketAddr,
        sync::{Arc, RwLock},
    },
};

#[derive(Clone)]
pub struct ClusterTpuInfo {
    cluster_info: Arc<ClusterInfo>,
    poh_recorder: Arc<RwLock<PohRecorder>>,
    recent_peers: HashMap<Pubkey, (SocketAddr, SocketAddr)>, // values are socket address for UDP and QUIC protocols
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
            .chain(once(self.cluster_info.my_contact_info()))
            .filter_map(|node| {
                Some((
                    *node.pubkey(),
                    (
                        node.tpu(Protocol::UDP).ok()?,
                        node.tpu(Protocol::QUIC).ok()?,
                    ),
                ))
            })
            .collect();
    }

    fn get_leader_tpus(&self, max_count: u64, protocol: Protocol) -> Vec<&SocketAddr> {
        let recorder = self.poh_recorder.read().unwrap();
        let leaders: Vec<_> = (0..max_count)
            .filter_map(|i| recorder.leader_after_n_slots(i * NUM_CONSECUTIVE_LEADER_SLOTS))
            .collect();
        drop(recorder);
        let mut unique_leaders = vec![];
        for leader in leaders.iter() {
            if let Some(addr) = self.recent_peers.get(leader).map(|addr| match protocol {
                Protocol::UDP => &addr.0,
                Protocol::QUIC => &addr.1,
            }) {
                if !unique_leaders.contains(&addr) {
                    unique_leaders.push(addr);
                }
            }
        }
        unique_leaders
    }

    fn get_leader_tpus_with_slots(
        &self,
        max_count: u64,
        protocol: Protocol,
    ) -> Vec<(&SocketAddr, Slot)> {
        let recorder = self.poh_recorder.read().unwrap();
        let leaders: Vec<_> = (0..max_count)
            .rev()
            .filter_map(|future_slot| {
                NUM_CONSECUTIVE_LEADER_SLOTS
                    .checked_mul(future_slot)
                    .and_then(|slots_in_the_future| {
                        recorder.leader_and_slot_after_n_slots(slots_in_the_future)
                    })
            })
            .collect();
        drop(recorder);
        let addrs_to_slots = leaders
            .into_iter()
            .filter_map(|(leader_id, leader_slot)| {
                self.recent_peers
                    .get(&leader_id)
                    .map(|(udp_tpu, quic_tpu)| match protocol {
                        Protocol::UDP => (udp_tpu, leader_slot),
                        Protocol::QUIC => (quic_tpu, leader_slot),
                    })
            })
            .collect::<HashMap<_, _>>();
        let mut unique_leaders = Vec::from_iter(addrs_to_slots);
        unique_leaders.sort_by_key(|(_addr, slot)| *slot);
        unique_leaders
    }
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_gossip::contact_info::ContactInfo,
        solana_ledger::{
            blockstore::Blockstore, get_tmp_ledger_path_auto_delete,
            leader_schedule_cache::LeaderScheduleCache,
        },
        solana_runtime::{
            bank::Bank,
            genesis_utils::{
                create_genesis_config_with_vote_accounts, GenesisConfigInfo, ValidatorVoteKeypairs,
            },
        },
        solana_sdk::{
            poh_config::PohConfig,
            quic::QUIC_PORT_OFFSET,
            signature::{Keypair, Signer},
            timing::timestamp,
        },
        solana_streamer::socket::SocketAddrSpace,
        std::{net::Ipv4Addr, sync::atomic::AtomicBool},
    };

    #[test]
    fn test_refresh_recent_peers() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let validator_vote_keypairs0 = ValidatorVoteKeypairs::new_rand();
        let validator_vote_keypairs1 = ValidatorVoteKeypairs::new_rand();
        let validator_vote_keypairs2 = ValidatorVoteKeypairs::new_rand();
        let mut expected_validator_pubkeys = vec![
            validator_vote_keypairs0.node_keypair.pubkey(),
            validator_vote_keypairs1.node_keypair.pubkey(),
            validator_vote_keypairs2.node_keypair.pubkey(),
        ];
        expected_validator_pubkeys.sort();
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
            Arc::new(blockstore),
            &Arc::new(LeaderScheduleCache::new_from_bank(&bank)),
            &PohConfig::default(),
            Arc::new(AtomicBool::default()),
        );

        let validator0_contact_info = ContactInfo::new_localhost(
            &validator_vote_keypairs0.node_keypair.pubkey(),
            timestamp(),
        );
        let validator1_contact_info = ContactInfo::new_localhost(
            &validator_vote_keypairs1.node_keypair.pubkey(),
            timestamp(),
        );
        let validator2_contact_info = ContactInfo::new_localhost(
            &validator_vote_keypairs2.node_keypair.pubkey(),
            timestamp(),
        );
        let cluster_info = Arc::new(ClusterInfo::new(
            validator0_contact_info,
            Arc::new(validator_vote_keypairs0.node_keypair),
            SocketAddrSpace::Unspecified,
        ));
        cluster_info.insert_info(validator1_contact_info);
        cluster_info.insert_info(validator2_contact_info);

        let mut leader_info =
            ClusterTpuInfo::new(cluster_info, Arc::new(RwLock::new(poh_recorder)));
        leader_info.refresh_recent_peers();
        let mut refreshed_recent_peers =
            leader_info.recent_peers.keys().copied().collect::<Vec<_>>();
        refreshed_recent_peers.sort();

        assert_eq!(refreshed_recent_peers, expected_validator_pubkeys);
    }

    #[test]
    fn test_get_leader_tpus() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

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
            Arc::new(blockstore),
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

        let validator0_socket = (
            SocketAddr::from((Ipv4Addr::LOCALHOST, 1111)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 1111 + QUIC_PORT_OFFSET)),
        );
        let validator1_socket = (
            SocketAddr::from((Ipv4Addr::LOCALHOST, 2222)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 2222 + QUIC_PORT_OFFSET)),
        );
        let validator2_socket = (
            SocketAddr::from((Ipv4Addr::LOCALHOST, 3333)),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 3333 + QUIC_PORT_OFFSET)),
        );
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
            leader_info.get_leader_tpus(1, Protocol::UDP),
            vec![&recent_peers.get(&first_leader).unwrap().0]
        );
        assert_eq!(
            leader_info.get_leader_tpus_with_slots(1, Protocol::UDP),
            vec![(&recent_peers.get(&first_leader).unwrap().0, 0)]
        );

        let second_leader = solana_ledger::leader_schedule_utils::slot_leader_at(
            slot + NUM_CONSECUTIVE_LEADER_SLOTS,
            &bank,
        )
        .unwrap();
        let mut expected_leader_sockets = vec![
            &recent_peers.get(&first_leader).unwrap().0,
            &recent_peers.get(&second_leader).unwrap().0,
        ];
        expected_leader_sockets.dedup();
        assert_eq!(
            leader_info.get_leader_tpus(2, Protocol::UDP),
            expected_leader_sockets
        );
        assert_eq!(
            leader_info.get_leader_tpus_with_slots(2, Protocol::UDP),
            expected_leader_sockets
                .into_iter()
                .zip([0, 4])
                .collect::<Vec<_>>()
        );

        let third_leader = solana_ledger::leader_schedule_utils::slot_leader_at(
            slot + (2 * NUM_CONSECUTIVE_LEADER_SLOTS),
            &bank,
        )
        .unwrap();
        let mut expected_leader_sockets = vec![
            &recent_peers.get(&first_leader).unwrap().0,
            &recent_peers.get(&second_leader).unwrap().0,
            &recent_peers.get(&third_leader).unwrap().0,
        ];
        expected_leader_sockets.dedup();
        assert_eq!(
            leader_info.get_leader_tpus(3, Protocol::UDP),
            expected_leader_sockets
        );
        // Only 2 leader tpus are returned always... so [0, 4, 8] isn't right here.
        // This assumption is safe. After all, leader schedule generation must be deterministic.
        assert_eq!(
            leader_info.get_leader_tpus_with_slots(3, Protocol::UDP),
            expected_leader_sockets
                .into_iter()
                .zip([0, 4])
                .collect::<Vec<_>>()
        );

        for x in 4..8 {
            assert!(leader_info.get_leader_tpus(x, Protocol::UDP).len() <= recent_peers.len());
            assert!(
                leader_info
                    .get_leader_tpus_with_slots(x, Protocol::UDP)
                    .len()
                    <= recent_peers.len()
            );
        }
    }
}
