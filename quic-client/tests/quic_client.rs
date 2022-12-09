#[cfg(test)]
mod tests {
    use {
        crossbeam_channel::{unbounded, Receiver},
        log::*,
        solana_perf::packet::PacketBatch,
        solana_quic_client::nonblocking::quic_client::{
            QuicClientCertificate, QuicLazyInitializedEndpoint,
        },
        solana_sdk::{packet::PACKET_DATA_SIZE, signature::Keypair},
        solana_streamer::{
            quic::StreamStats, streamer::StakedNodes,
            tls_certificates::new_self_signed_tls_certificate_chain,
        },
        solana_tpu_client::connection_cache_stats::ConnectionCacheStats,
        std::{
            net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
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
                assert_eq!(p.meta().size, num_bytes);
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
        use {
            solana_quic_client::quic_client::QuicTpuConnection,
            solana_tpu_client::tpu_connection::TpuConnection,
        };
        solana_logger::setup();
        let (sender, receiver) = unbounded();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (s, exit, keypair, ip, stats) = server_args();
        let (_, t) = solana_streamer::quic::spawn_server(
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
            1000,
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
        use {
            solana_quic_client::nonblocking::quic_client::QuicTpuConnection,
            solana_tpu_client::nonblocking::tpu_connection::TpuConnection,
        };
        solana_logger::setup();
        let (sender, receiver) = unbounded();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (s, exit, keypair, ip, stats) = server_args();
        let (_, t) = solana_streamer::nonblocking::quic::spawn_server(
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
            1000,
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

    #[test]
    fn test_quic_bi_direction() {
        /// This tests bi-directional quic communication. There are the following components
        /// The request receiver -- responsible for receiving requests
        /// The request sender -- responsible sending requests to the request reciever using quic
        /// The response receiver -- responsible for receiving the responses to the requests
        /// The response sender -- responsible for sending responses to the response receiver.
        /// In this we demonstrate that the request sender and the response receiver use the
        /// same quic Endpoint, and the same UDP socket.
        use {
            solana_quic_client::quic_client::QuicTpuConnection,
            solana_tpu_client::tpu_connection::TpuConnection,
        };
        solana_logger::setup();

        // Request Receiver
        let (sender, receiver) = unbounded();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let (request_recv_socket, request_recv_exit, keypair, request_recv_ip, request_recv_stats) =
            server_args();
        let (request_recv_endpoint, request_recv_thread) = solana_streamer::quic::spawn_server(
            request_recv_socket.try_clone().unwrap(),
            &keypair,
            request_recv_ip,
            sender,
            request_recv_exit.clone(),
            1,
            staked_nodes.clone(),
            10,
            10,
            request_recv_stats,
            1000,
        )
        .unwrap();

        drop(request_recv_endpoint);
        // Response Receiver:
        let (
            response_recv_socket,
            response_recv_exit,
            keypair2,
            response_recv_ip,
            response_recv_stats,
        ) = server_args();
        let (sender2, receiver2) = unbounded();

        let addr = response_recv_socket.local_addr().unwrap().ip();
        let port = response_recv_socket.local_addr().unwrap().port();
        let server_addr = SocketAddr::new(addr, port);
        let (response_recv_endpoint, response_recv_thread) = solana_streamer::quic::spawn_server(
            response_recv_socket,
            &keypair2,
            response_recv_ip,
            sender2,
            response_recv_exit.clone(),
            1,
            staked_nodes,
            10,
            10,
            response_recv_stats,
            1000,
        )
        .unwrap();

        // Request Sender, it uses the same endpoint as the response receiver:
        let addr = request_recv_socket.local_addr().unwrap().ip();
        let port = request_recv_socket.local_addr().unwrap().port();
        let tpu_addr = SocketAddr::new(addr, port);
        let connection_cache_stats = Arc::new(ConnectionCacheStats::default());

        let (certs, priv_key) = new_self_signed_tls_certificate_chain(
            &Keypair::new(),
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        )
        .expect("Failed to initialize QUIC client certificates");
        let client_certificate = Arc::new(QuicClientCertificate {
            certificates: certs,
            key: priv_key,
        });

        let endpoint =
            QuicLazyInitializedEndpoint::new(client_certificate, Some(response_recv_endpoint));
        let request_sender =
            QuicTpuConnection::new(Arc::new(endpoint), tpu_addr, connection_cache_stats);
        // Send a full size packet with single byte writes as a request.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets: usize = 3000;
        let packets = vec![vec![0u8; PACKET_DATA_SIZE]; num_expected_packets];

        assert!(request_sender
            .send_wire_transaction_batch_async(packets)
            .is_ok());
        check_packets(receiver, num_bytes, num_expected_packets);
        info!("Received requests!");

        // Response sender
        let (certs, priv_key) = new_self_signed_tls_certificate_chain(
            &Keypair::new(),
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
        )
        .expect("Failed to initialize QUIC client certificates");

        let client_certificate2 = Arc::new(QuicClientCertificate {
            certificates: certs,
            key: priv_key,
        });

        let endpoint2 = QuicLazyInitializedEndpoint::new(client_certificate2, None);
        let connection_cache_stats2 = Arc::new(ConnectionCacheStats::default());
        let response_sender =
            QuicTpuConnection::new(Arc::new(endpoint2), server_addr, connection_cache_stats2);

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets: usize = 3000;
        let packets = vec![vec![0u8; PACKET_DATA_SIZE]; num_expected_packets];

        assert!(response_sender
            .send_wire_transaction_batch_async(packets)
            .is_ok());
        check_packets(receiver2, num_bytes, num_expected_packets);
        info!("Received responses!");

        // Drop the clients explicitly to avoid hung on drops
        drop(request_sender);
        drop(response_sender);

        request_recv_exit.store(true, Ordering::Relaxed);
        request_recv_thread.join().unwrap();
        info!("Request receiver exited!");

        response_recv_exit.store(true, Ordering::Relaxed);
        response_recv_thread.join().unwrap();
        info!("Response receiver exited!");
    }
}
