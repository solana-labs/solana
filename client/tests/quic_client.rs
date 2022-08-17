#[cfg(test)]
mod tests {
    use {
        crossbeam_channel::{unbounded, Receiver},
        solana_client::{
            connection_cache::ConnectionCacheStats,
            nonblocking::quic_client::QuicLazyInitializedEndpoint,
        },
        solana_perf::packet::PacketBatch,
        solana_sdk::{packet::PACKET_DATA_SIZE, signature::Keypair},
        solana_streamer::{quic::StreamStats, streamer::StakedNodes},
        std::{
            net::{IpAddr, SocketAddr, UdpSocket},
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, RwLock,
            },
            time::{Duration, Instant},
        },
    };

    fn check_packets(
        receiver: Receiver<PacketBatch>,
        num_bytes: usize,
        num_expected_packets: usize,
    ) {
        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets: usize = 0;
        while now.elapsed().as_secs() < 10 {
            if let Ok(packets) = receiver.recv_timeout(Duration::from_secs(1)) {
                total_packets = total_packets.saturating_add(packets.len());
                all_packets.push(packets)
            }
            if total_packets >= num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in &batch {
                assert_eq!(p.meta.size, num_bytes);
            }
        }
        assert_eq!(total_packets, num_expected_packets);
    }

    fn server_args() -> (
        UdpSocket,
        Arc<AtomicBool>,
        Keypair,
        IpAddr,
        Arc<StreamStats>,
    ) {
        (
            UdpSocket::bind("127.0.0.1:0").unwrap(),
            Arc::new(AtomicBool::new(false)),
            Keypair::new(),
            "127.0.0.1".parse().unwrap(),
            Arc::new(StreamStats::default()),
        )
    }

    #[test]
    fn test_quic_client_multiple_writes() {
        use solana_client::{quic_client::QuicTpuConnection, tpu_connection::TpuConnection};
        solana_logger::setup();
        let (sender, receiver) = unbounded();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (s, exit, keypair, ip, stats) = server_args();
        let t = solana_streamer::quic::spawn_server(
            s.try_clone().unwrap(),
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            10,
            10,
            stats,
        )
        .unwrap();

        let addr = s.local_addr().unwrap().ip();
        let port = s.local_addr().unwrap().port();
        let tpu_addr = SocketAddr::new(addr, port);
        let connection_cache_stats = Arc::new(ConnectionCacheStats::default());
        let client = QuicTpuConnection::new(
            Arc::new(QuicLazyInitializedEndpoint::default()),
            tpu_addr,
            connection_cache_stats,
        );

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets: usize = 3000;
        let packets = vec![vec![0u8; PACKET_DATA_SIZE]; num_expected_packets];

        assert!(client.send_wire_transaction_batch_async(packets).is_ok());

        check_packets(receiver, num_bytes, num_expected_packets);
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[tokio::test]
    async fn test_nonblocking_quic_client_multiple_writes() {
        use solana_client::nonblocking::{
            quic_client::QuicTpuConnection, tpu_connection::TpuConnection,
        };
        solana_logger::setup();
        let (sender, receiver) = unbounded();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (s, exit, keypair, ip, stats) = server_args();
        let t = solana_streamer::nonblocking::quic::spawn_server(
            s.try_clone().unwrap(),
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            10,
            10,
            stats,
        )
        .unwrap();

        let addr = s.local_addr().unwrap().ip();
        let port = s.local_addr().unwrap().port();
        let tpu_addr = SocketAddr::new(addr, port);
        let connection_cache_stats = Arc::new(ConnectionCacheStats::default());
        let client = QuicTpuConnection::new(
            Arc::new(QuicLazyInitializedEndpoint::default()),
            tpu_addr,
            connection_cache_stats,
        );

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets: usize = 3000;
        let packets = vec![vec![0u8; PACKET_DATA_SIZE]; num_expected_packets];

        assert!(client.send_wire_transaction_batch(&packets).await.is_ok());

        check_packets(receiver, num_bytes, num_expected_packets);
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }
}
