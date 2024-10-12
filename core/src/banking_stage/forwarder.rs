use {
    super::{
        forward_packet_batches_by_accounts::ForwardPacketBatchesByAccounts,
        leader_slot_metrics::LeaderSlotMetricsTracker,
        unprocessed_transaction_storage::UnprocessedTransactionStorage, BankingStageStats,
        ForwardOption,
    },
    crate::{
        banking_stage::{
            immutable_deserialized_packet::ImmutableDeserializedPacket, LikeClusterInfo,
        },
        next_leader::{next_leader, next_leader_tpu_vote},
        tracer_packet_stats::TracerPacketStats,
    },
    solana_client::connection_cache::ConnectionCache,
    solana_connection_cache::client_connection::ClientConnection as TpuConnection,
    solana_feature_set::FeatureSet,
    solana_measure::measure_us,
    solana_perf::{data_budget::DataBudget, packet::Packet},
    solana_poh::poh_recorder::PohRecorder,
    solana_runtime::bank_forks::BankForks,
    solana_sdk::{pubkey::Pubkey, transaction::SanitizedTransaction, transport::TransportError},
    solana_streamer::sendmmsg::batch_send,
    std::{
        iter::repeat,
        net::{SocketAddr, UdpSocket},
        sync::{atomic::Ordering, Arc, RwLock},
    },
};

pub struct Forwarder<T: LikeClusterInfo> {
    poh_recorder: Arc<RwLock<PohRecorder>>,
    bank_forks: Arc<RwLock<BankForks>>,
    socket: UdpSocket,
    cluster_info: T,
    connection_cache: Arc<ConnectionCache>,
    data_budget: Arc<DataBudget>,
    forward_packet_batches_by_accounts: ForwardPacketBatchesByAccounts,
}

impl<T: LikeClusterInfo> Forwarder<T> {
    pub fn new(
        poh_recorder: Arc<RwLock<PohRecorder>>,
        bank_forks: Arc<RwLock<BankForks>>,
        cluster_info: T,
        connection_cache: Arc<ConnectionCache>,
        data_budget: Arc<DataBudget>,
    ) -> Self {
        Self {
            poh_recorder,
            bank_forks,
            socket: UdpSocket::bind("0.0.0.0:0").unwrap(),
            cluster_info,
            connection_cache,
            data_budget,
            forward_packet_batches_by_accounts:
                ForwardPacketBatchesByAccounts::new_with_default_batch_limits(),
        }
    }

    pub fn clear_batches(&mut self) {
        self.forward_packet_batches_by_accounts.reset();
    }

    pub fn try_add_packet(
        &mut self,
        sanitized_transaction: &SanitizedTransaction,
        immutable_packet: Arc<ImmutableDeserializedPacket>,
        feature_set: &FeatureSet,
    ) -> bool {
        self.forward_packet_batches_by_accounts.try_add_packet(
            sanitized_transaction,
            immutable_packet,
            feature_set,
        )
    }

    pub fn forward_batched_packets(&self, forward_option: &ForwardOption) {
        self.forward_packet_batches_by_accounts
            .iter_batches()
            .filter(|&batch| !batch.is_empty())
            .for_each(|forwardable_batch| {
                let _ = self
                    .forward_packets(forward_option, forwardable_batch.get_forwardable_packets());
            });
    }

    /// This function is exclusively used by multi-iterator banking threads to handle forwarding
    /// logic per thread. Central scheduler Controller uses try_add_packet() ... forward_batched_packets()
    /// to handle forwarding slight differently.
    pub fn handle_forwarding(
        &mut self,
        unprocessed_transaction_storage: &mut UnprocessedTransactionStorage,
        hold: bool,
        slot_metrics_tracker: &mut LeaderSlotMetricsTracker,
        banking_stage_stats: &BankingStageStats,
        tracer_packet_stats: &mut TracerPacketStats,
    ) {
        let forward_option = unprocessed_transaction_storage.forward_option();

        // get current working bank from bank_forks, use it to sanitize transaction and
        // load all accounts from address loader;
        let current_bank = self.bank_forks.read().unwrap().working_bank();

        // if we have crossed an epoch boundary, recache any state
        unprocessed_transaction_storage.cache_epoch_boundary_info(&current_bank);

        // sanitize and filter packets that are no longer valid (could be too old, a duplicate of something
        // already processed), then add to forwarding buffer.
        let filter_forwarding_result = unprocessed_transaction_storage
            .filter_forwardable_packets_and_add_batches(
                current_bank,
                &mut self.forward_packet_batches_by_accounts,
            );
        slot_metrics_tracker.increment_transactions_from_packets_us(
            filter_forwarding_result.total_packet_conversion_us,
        );
        banking_stage_stats.packet_conversion_elapsed.fetch_add(
            filter_forwarding_result.total_packet_conversion_us,
            Ordering::Relaxed,
        );
        banking_stage_stats
            .filter_pending_packets_elapsed
            .fetch_add(
                filter_forwarding_result.total_filter_packets_us,
                Ordering::Relaxed,
            );
        banking_stage_stats.dropped_forward_packets_count.fetch_add(
            filter_forwarding_result.total_dropped_packets,
            Ordering::Relaxed,
        );

        self.forward_packet_batches_by_accounts
            .iter_batches()
            .filter(|&batch| !batch.is_empty())
            .for_each(|forward_batch| {
                slot_metrics_tracker.increment_forwardable_batches_count(1);

                let batched_forwardable_packets_count = forward_batch.len();
                let (_forward_result, successful_forwarded_packets_count, leader_pubkey) = self
                    .forward_buffered_packets(
                        &forward_option,
                        forward_batch.get_forwardable_packets(),
                        banking_stage_stats,
                    );

                if let Some(leader_pubkey) = leader_pubkey {
                    tracer_packet_stats.increment_total_forwardable_tracer_packets(
                        filter_forwarding_result.total_forwardable_tracer_packets,
                        leader_pubkey,
                    );
                }
                let failed_forwarded_packets_count = batched_forwardable_packets_count
                    .saturating_sub(successful_forwarded_packets_count);

                if failed_forwarded_packets_count > 0 {
                    slot_metrics_tracker.increment_failed_forwarded_packets_count(
                        failed_forwarded_packets_count as u64,
                    );
                    slot_metrics_tracker.increment_packet_batch_forward_failure_count(1);
                }

                if successful_forwarded_packets_count > 0 {
                    slot_metrics_tracker.increment_successful_forwarded_packets_count(
                        successful_forwarded_packets_count as u64,
                    );
                }
            });
        self.clear_batches();

        if !hold {
            slot_metrics_tracker.increment_cleared_from_buffer_after_forward_count(
                filter_forwarding_result.total_forwardable_packets as u64,
            );
            tracer_packet_stats.increment_total_cleared_from_buffer_after_forward(
                filter_forwarding_result.total_tracer_packets_in_buffer,
            );
            unprocessed_transaction_storage.clear_forwarded_packets();
        }
    }

    /// Forwards all valid, unprocessed packets in the iterator, up to a rate limit.
    /// Returns whether forwarding succeeded, the number of attempted forwarded packets
    /// if any, the time spent forwarding in us, and the leader pubkey if any.
    pub fn forward_packets<'a>(
        &self,
        forward_option: &ForwardOption,
        forwardable_packets: impl Iterator<Item = &'a Packet>,
    ) -> (
        std::result::Result<(), TransportError>,
        usize,
        u64,
        Option<Pubkey>,
    ) {
        let Some((leader_pubkey, addr)) = self.get_leader_and_addr(forward_option) else {
            return (Ok(()), 0, 0, None);
        };

        self.update_data_budget();
        let packet_vec: Vec<_> = forwardable_packets
            .filter(|p| !p.meta().forwarded())
            .filter(|p| p.meta().is_from_staked_node())
            .filter(|p| self.data_budget.take(p.meta().size))
            .filter_map(|p| p.data(..).map(|data| data.to_vec()))
            .collect();

        let packet_vec_len = packet_vec.len();
        // TODO: see https://github.com/solana-labs/solana/issues/23819
        // fix this so returns the correct number of succeeded packets
        // when there's an error sending the batch. This was left as-is for now
        // in favor of shipping Quic support, which was considered higher-priority
        let (res, forward_us) = if !packet_vec.is_empty() {
            measure_us!(self.forward(forward_option, packet_vec, &addr))
        } else {
            (Ok(()), 0)
        };

        (res, packet_vec_len, forward_us, Some(leader_pubkey))
    }

    /// Forwards all valid, unprocessed packets in the buffer, up to a rate limit. Returns
    /// the number of successfully forwarded packets in second part of tuple
    fn forward_buffered_packets<'a>(
        &self,
        forward_option: &ForwardOption,
        forwardable_packets: impl Iterator<Item = &'a Packet>,
        banking_stage_stats: &BankingStageStats,
    ) -> (
        std::result::Result<(), TransportError>,
        usize,
        Option<Pubkey>,
    ) {
        let (res, num_packets, _forward_us, leader_pubkey) =
            self.forward_packets(forward_option, forwardable_packets);
        if let Err(ref err) = res {
            warn!("failed to forward packets: {err}");
        }

        if num_packets > 0 {
            if let ForwardOption::ForwardTpuVote = forward_option {
                banking_stage_stats
                    .forwarded_vote_count
                    .fetch_add(num_packets, Ordering::Relaxed);
            } else {
                banking_stage_stats
                    .forwarded_transaction_count
                    .fetch_add(num_packets, Ordering::Relaxed);
            }
        }

        (res, num_packets, leader_pubkey)
    }

    /// Get the pubkey and socket address for the leader to forward to
    fn get_leader_and_addr(&self, forward_option: &ForwardOption) -> Option<(Pubkey, SocketAddr)> {
        match forward_option {
            ForwardOption::NotForward => None,
            ForwardOption::ForwardTransaction => {
                next_leader(&self.cluster_info, &self.poh_recorder, |node| {
                    node.tpu_forwards(self.connection_cache.protocol())
                })
            }
            ForwardOption::ForwardTpuVote => {
                next_leader_tpu_vote(&self.cluster_info, &self.poh_recorder)
            }
        }
    }

    /// Re-fill the data budget if enough time has passed
    fn update_data_budget(&self) {
        const INTERVAL_MS: u64 = 100;
        // 12 MB outbound limit per second
        const MAX_BYTES_PER_SECOND: usize = 12_000_000;
        const MAX_BYTES_PER_INTERVAL: usize = MAX_BYTES_PER_SECOND * INTERVAL_MS as usize / 1000;
        const MAX_BYTES_BUDGET: usize = MAX_BYTES_PER_INTERVAL * 5;
        self.data_budget.update(INTERVAL_MS, |bytes| {
            std::cmp::min(
                bytes.saturating_add(MAX_BYTES_PER_INTERVAL),
                MAX_BYTES_BUDGET,
            )
        });
    }

    fn forward(
        &self,
        forward_option: &ForwardOption,
        packet_vec: Vec<Vec<u8>>,
        addr: &SocketAddr,
    ) -> Result<(), TransportError> {
        match forward_option {
            ForwardOption::ForwardTpuVote => {
                // The vote must be forwarded using only UDP.
                let pkts: Vec<_> = packet_vec.into_iter().zip(repeat(*addr)).collect();
                batch_send(&self.socket, &pkts).map_err(|err| err.into())
            }
            ForwardOption::ForwardTransaction => {
                let conn = self.connection_cache.get_connection(addr);
                conn.send_data_batch_async(packet_vec)
            }
            ForwardOption::NotForward => panic!("should not forward"),
        }
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::banking_stage::{
            tests::{create_slow_genesis_config_with_leader, new_test_cluster_info},
            unprocessed_packet_batches::{DeserializedPacket, UnprocessedPacketBatches},
            unprocessed_transaction_storage::ThreadType,
        },
        solana_client::rpc_client::SerializableTransaction,
        solana_gossip::cluster_info::{ClusterInfo, Node},
        solana_ledger::{blockstore::Blockstore, genesis_utils::GenesisConfigInfo},
        solana_perf::packet::PacketFlags,
        solana_poh::{poh_recorder::create_test_recorder, poh_service::PohService},
        solana_runtime::bank::Bank,
        solana_sdk::{
            hash::Hash, poh_config::PohConfig, signature::Keypair, signer::Signer,
            system_transaction, transaction::VersionedTransaction,
        },
        solana_streamer::{
            nonblocking::testing_utilities::{
                setup_quic_server_with_sockets, SpawnTestServerResult, TestServerConfig,
            },
            quic::rt,
        },
        std::{
            sync::atomic::AtomicBool,
            time::{Duration, Instant},
        },
        tempfile::TempDir,
        tokio::time::sleep,
    };

    struct TestSetup {
        _ledger_dir: TempDir,
        blockhash: Hash,
        rent_min_balance: u64,

        bank_forks: Arc<RwLock<BankForks>>,
        poh_recorder: Arc<RwLock<PohRecorder>>,
        exit: Arc<AtomicBool>,
        poh_service: PohService,
        cluster_info: Arc<ClusterInfo>,
        local_node: Node,
    }

    fn setup() -> TestSetup {
        let validator_keypair = Arc::new(Keypair::new());
        let genesis_config_info =
            create_slow_genesis_config_with_leader(10_000, &validator_keypair.pubkey());
        let GenesisConfigInfo { genesis_config, .. } = &genesis_config_info;

        let (bank, bank_forks) = Bank::new_no_wallclock_throttle_for_tests(genesis_config);

        let ledger_path = TempDir::new().unwrap();
        let blockstore = Arc::new(
            Blockstore::open(ledger_path.as_ref())
                .expect("Expected to be able to open database ledger"),
        );
        let poh_config = PohConfig {
            // limit tick count to avoid clearing working_bank at
            // PohRecord then PohRecorderError(MaxHeightReached) at BankingStage
            target_tick_count: Some(bank.max_tick_height() - 1),
            ..PohConfig::default()
        };

        let (exit, poh_recorder, poh_service, _entry_receiver) =
            create_test_recorder(bank, blockstore, Some(poh_config), None);

        let (local_node, cluster_info) = new_test_cluster_info(Some(validator_keypair));
        let cluster_info = Arc::new(cluster_info);

        TestSetup {
            _ledger_dir: ledger_path,
            blockhash: genesis_config.hash(),
            rent_min_balance: genesis_config.rent.minimum_balance(0),

            bank_forks,
            poh_recorder,
            exit,
            poh_service,
            cluster_info,
            local_node,
        }
    }

    async fn check_all_received(
        socket: UdpSocket,
        expected_num_packets: usize,
        expected_packet_size: usize,
        expected_blockhash: &Hash,
    ) {
        let SpawnTestServerResult {
            join_handle,
            exit,
            receiver,
            server_address: _,
            stats: _,
        } = setup_quic_server_with_sockets(vec![socket], None, TestServerConfig::default());

        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 5 {
            if let Ok(packets) = receiver.try_recv() {
                total_packets += packets.len();
                for packet in packets.iter() {
                    assert_eq!(packet.meta().size, expected_packet_size);
                    let tx: VersionedTransaction = packet.deserialize_slice(..).unwrap();
                    assert_eq!(
                        tx.get_recent_blockhash(),
                        expected_blockhash,
                        "Unexpected blockhash, tx: {tx:?}, expected blockhash: {expected_blockhash}."
                    );
                }
            } else {
                sleep(Duration::from_millis(100)).await;
            }
            if total_packets >= expected_num_packets {
                break;
            }
        }
        assert_eq!(total_packets, expected_num_packets);

        exit.store(true, Ordering::Relaxed);
        join_handle.await.unwrap();
    }

    #[test]
    fn test_forwarder_budget() {
        let TestSetup {
            blockhash,
            rent_min_balance,
            bank_forks,
            poh_recorder,
            exit,
            poh_service,
            cluster_info,
            local_node,
            ..
        } = setup();

        // Create `PacketBatch` with 1 unprocessed packet
        let tx = system_transaction::transfer(
            &Keypair::new(),
            &solana_sdk::pubkey::new_rand(),
            rent_min_balance,
            blockhash,
        );
        let mut packet = Packet::from_data(None, tx).unwrap();
        // unstaked transactions will not be forwarded
        packet.meta_mut().set_from_staked_node(true);
        let expected_packet_size = packet.meta().size;
        let deserialized_packet = DeserializedPacket::new(packet).unwrap();

        let test_cases = vec![
            ("budget-restricted", DataBudget::restricted(), 0),
            ("budget-available", DataBudget::default(), 1),
        ];
        let runtime = rt("solQuicTestRt".to_string());
        for (_name, data_budget, expected_num_forwarded) in test_cases {
            let mut forwarder = Forwarder::new(
                poh_recorder.clone(),
                bank_forks.clone(),
                cluster_info.clone(),
                Arc::new(ConnectionCache::new("connection_cache_test")),
                Arc::new(data_budget),
            );
            let unprocessed_packet_batches: UnprocessedPacketBatches =
                UnprocessedPacketBatches::from_iter(
                    vec![deserialized_packet.clone()].into_iter(),
                    1,
                );
            let stats = BankingStageStats::default();
            forwarder.handle_forwarding(
                &mut UnprocessedTransactionStorage::new_transaction_storage(
                    unprocessed_packet_batches,
                    ThreadType::Transactions,
                ),
                true,
                &mut LeaderSlotMetricsTracker::new(0),
                &stats,
                &mut TracerPacketStats::new(0),
            );

            let recv_socket = &local_node.sockets.tpu_forwards_quic[0];
            runtime.block_on(check_all_received(
                (*recv_socket).try_clone().unwrap(),
                expected_num_forwarded,
                expected_packet_size,
                &blockhash,
            ));
        }

        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }

    #[test]
    fn test_handle_forwarding() {
        let TestSetup {
            blockhash,
            rent_min_balance,
            bank_forks,
            poh_recorder,
            exit,
            poh_service,
            cluster_info,
            local_node,
            ..
        } = setup();

        let keypair = Keypair::new();
        let pubkey = solana_sdk::pubkey::new_rand();

        // forwarded packets will not be forwarded again
        let forwarded_packet = {
            let transaction =
                system_transaction::transfer(&keypair, &pubkey, rent_min_balance, blockhash);
            let mut packet = Packet::from_data(None, transaction).unwrap();
            packet.meta_mut().flags |= PacketFlags::FORWARDED;
            DeserializedPacket::new(packet).unwrap()
        };
        // packets from unstaked nodes will not be forwarded
        let unstaked_packet = {
            let transaction =
                system_transaction::transfer(&keypair, &pubkey, rent_min_balance, blockhash);
            let packet = Packet::from_data(None, transaction).unwrap();
            DeserializedPacket::new(packet).unwrap()
        };
        // packets with incorrect blockhash will be filtered out
        let incorrect_blockhash_packet = {
            let transaction =
                system_transaction::transfer(&keypair, &pubkey, rent_min_balance, Hash::default());
            let packet = Packet::from_data(None, transaction).unwrap();
            DeserializedPacket::new(packet).unwrap()
        };

        // maybe also add packet without stake and packet with incorrect blockhash?
        let (expected_packet_size, normal_packet) = {
            let transaction = system_transaction::transfer(&keypair, &pubkey, 1, blockhash);
            let mut packet = Packet::from_data(None, transaction).unwrap();
            packet.meta_mut().set_from_staked_node(true);
            (packet.meta().size, DeserializedPacket::new(packet).unwrap())
        };

        let mut unprocessed_packet_batches = UnprocessedTransactionStorage::new_transaction_storage(
            UnprocessedPacketBatches::from_iter(
                vec![
                    forwarded_packet,
                    unstaked_packet,
                    incorrect_blockhash_packet,
                    normal_packet,
                ],
                4,
            ),
            ThreadType::Transactions,
        );
        let connection_cache = ConnectionCache::new("connection_cache_test");

        let test_cases = vec![
            ("fwd-normal", true, 2, 1),
            ("fwd-no-op", true, 2, 0),
            ("fwd-no-hold", false, 0, 0),
        ];

        let mut forwarder = Forwarder::new(
            poh_recorder,
            bank_forks,
            cluster_info,
            Arc::new(connection_cache),
            Arc::new(DataBudget::default()),
        );
        let runtime = rt("solQuicTestRt".to_string());
        for (name, hold, expected_num_unprocessed, expected_num_processed) in test_cases {
            let stats = BankingStageStats::default();
            forwarder.handle_forwarding(
                &mut unprocessed_packet_batches,
                hold,
                &mut LeaderSlotMetricsTracker::new(0),
                &stats,
                &mut TracerPacketStats::new(0),
            );

            let recv_socket = &local_node.sockets.tpu_forwards_quic[0];

            runtime.block_on(check_all_received(
                (*recv_socket).try_clone().unwrap(),
                expected_num_processed,
                expected_packet_size,
                &blockhash,
            ));

            let num_unprocessed_packets: usize = unprocessed_packet_batches.len();
            assert_eq!(num_unprocessed_packets, expected_num_unprocessed, "{name}");
        }

        exit.store(true, Ordering::Relaxed);
        poh_service.join().unwrap();
    }
}
