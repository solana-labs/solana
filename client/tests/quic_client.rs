#[cfg(test)]
mod tests {
    use {
        crossbeam_channel::unbounded,
        solana_client::{
            quic_client::QuicTpuConnection,
            tpu_connection::{ClientStats, TpuConnection},
        },
        solana_sdk::{packet::PACKET_DATA_SIZE, quic::QUIC_PORT_OFFSET, signature::Keypair},
        solana_streamer::quic::spawn_server,
        std::{
            collections::HashMap,
            net::{SocketAddr, UdpSocket},
            sync::{
                atomic::{AtomicBool, Ordering},
                Arc, RwLock,
            },
            time::{Duration, Instant},
        },
    };

    #[test]
    fn test_quic_client_multiple_writes() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let staked_nodes = Arc::new(RwLock::new(HashMap::new()));
        let t = spawn_server(
            s.try_clone().unwrap(),
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            10,
            10,
        )
        .unwrap();

        let addr = s.local_addr().unwrap().ip();
        let port = s.local_addr().unwrap().port() - QUIC_PORT_OFFSET;
        let tpu_addr = SocketAddr::new(addr, port);
        let client = QuicTpuConnection::new(tpu_addr);

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets: usize = 4000;
        let packets = vec![vec![0u8; PACKET_DATA_SIZE]; num_expected_packets];

        let stats = Arc::new(ClientStats::default());

        assert!(client
            .send_wire_transaction_batch_async(packets, stats)
            .is_ok());

        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 5 {
            if let Ok(packets) = receiver.recv_timeout(Duration::from_secs(1)) {
                total_packets += packets.packets.len();
                all_packets.push(packets)
            }
            if total_packets >= num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in &batch.packets {
                assert_eq!(p.meta.size, num_bytes);
            }
        }
        assert_eq!(total_packets, num_expected_packets);

        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }
}
