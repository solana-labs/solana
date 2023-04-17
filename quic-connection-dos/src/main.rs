#![allow(clippy::integer_arithmetic)]
use {
    clap::{crate_description, crate_name, value_t, App, Arg},
    futures::future::join_all,
    quinn::{
        ClientConfig, Connection, Endpoint, EndpointConfig, IdleTimeout, TokioRuntime,
        TransportConfig,
    },
    solana_client::nonblocking::quic_client::QuicClientCertificate,
    solana_net_utils::{bind_in_range, VALIDATOR_PORT_RANGE},
    //rayon::prelude::*,
    //solana_measure::measure::Measure,
    solana_sdk::{
        packet::PACKET_DATA_SIZE,
        quic::{QUIC_KEEP_ALIVE, QUIC_MAX_TIMEOUT},
        signer::keypair::Keypair,
    },
    solana_streamer::{
        nonblocking::quic::ALPN_TPU_PROTOCOL_ID, tls_certificates::new_self_signed_tls_certificate,
    },
    std::{
        env,
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::Arc,
    },
};

struct SkipServerVerification;

impl SkipServerVerification {
    fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}

fn _create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
    quinn::Endpoint::new(config, None, client_socket, TokioRuntime)
        .expect("QuicNewConnection::create_endpoint quinn::Endpoint::new")
}

fn create_endpoint(client_certificate: Arc<QuicClientCertificate>) -> Endpoint {
    let mut endpoint = {
        let client_socket = bind_in_range(IpAddr::V4(Ipv4Addr::UNSPECIFIED), VALIDATOR_PORT_RANGE)
            .expect("QuicLazyInitializedEndpoint::create_endpoint bind_in_range")
            .1;

        _create_endpoint(EndpointConfig::default(), client_socket)
    };

    let mut crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_single_cert(
            vec![client_certificate.certificate.clone()],
            client_certificate.key.clone(),
        )
        .expect("Failed to set QUIC client certificates");
    crypto.enable_early_data = true;
    crypto.alpn_protocols = vec![ALPN_TPU_PROTOCOL_ID.to_vec()];

    let mut config = ClientConfig::new(Arc::new(crypto));
    let mut transport_config = TransportConfig::default();

    let timeout = IdleTimeout::try_from(QUIC_MAX_TIMEOUT).unwrap();
    transport_config.max_idle_timeout(Some(timeout));
    transport_config.keep_alive_interval(Some(QUIC_KEEP_ALIVE));
    config.transport_config(Arc::new(transport_config));

    endpoint.set_default_client_config(config);

    endpoint
}

pub fn get_client_config(keypair: &Keypair) -> ClientConfig {
    let ipaddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let (cert, key) = new_self_signed_tls_certificate(keypair, ipaddr)
        .expect("Failed to generate client certificate");

    let mut crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_single_cert(vec![cert], key)
        .expect("Failed to use client certificate");

    crypto.enable_early_data = true;
    crypto.alpn_protocols = vec![ALPN_TPU_PROTOCOL_ID.to_vec()];

    let mut config = ClientConfig::new(Arc::new(crypto));

    let mut transport_config = TransportConfig::default();
    let timeout = IdleTimeout::try_from(QUIC_MAX_TIMEOUT).unwrap();
    transport_config.max_idle_timeout(Some(timeout));
    transport_config.keep_alive_interval(Some(QUIC_KEEP_ALIVE));
    config.transport_config(Arc::new(transport_config));

    config
}

// Opens a slow stream and tries to send PACKET_DATA_SIZE bytes of junk
// in as many chunks as possible. We don't allow the number of chunks
// to be configurable as client-side writes don't correspond to
// quic-level packets/writes (but by doing multiple writes we are generally able
// to get multiple writes on the quic level despite the spec not guaranteeing this)
pub async fn check_multiple_writes(conn: &Connection) {
    // Send a full size packet with single byte writes.
    let num_bytes = PACKET_DATA_SIZE;
    let mut s1 = conn.open_uni().await.unwrap();
    for _ in 0..num_bytes {
        s1.write_all(&[0u8]).await.unwrap();
    }
    s1.finish().await.unwrap();
}

async fn run_connection_dos(
    server_address: SocketAddr,
    num_connections: u64,
    num_streams_per_conn: u64,
) {
    let (cert, priv_key) =
        new_self_signed_tls_certificate(&Keypair::new(), IpAddr::V4(Ipv4Addr::UNSPECIFIED))
            .expect("Failed to create cert");

    let cert = Arc::new(QuicClientCertificate {
        certificate: cert,
        key: priv_key,
    });

    let endpoint = create_endpoint(cert);

    let mut connections = vec![];
    for _ in 0..num_connections {
        connections.push(
            endpoint
                .connect(server_address, "connect")
                .expect("Failed in connecting")
                .await
                .expect("Failed in waiting"),
        );
    }

    let futures: Vec<_> = connections
        .iter()
        .map(|conn| (0..num_streams_per_conn).map(|_| check_multiple_writes(conn)))
        .flatten()
        .collect();

    join_all(futures).await;
}

fn main() {
    solana_logger::setup();

    let matches = App::new(crate_name!())
        .about(crate_description!())
        .version(solana_version::version!())
        .arg(
            Arg::with_name("target_address")
                .long("target_address")
                .takes_value(true)
                .value_name("TARGET_ADDR")
                .help("Target address"),
        )
        .arg(
            Arg::with_name("num_connections")
                .long("num_connections")
                .takes_value(true)
                .value_name("NUM_CONN")
                .help("Number of connections"),
        )
        .arg(
            Arg::with_name("num_streams_per_conn")
                .long("num_streams_per_conn")
                .takes_value(true)
                .value_name("NUM_STREAMS")
                .help("Number of streams per connection"),
        )
        .get_matches();

    let num_connections = value_t!(matches, "num_connections", u64).unwrap_or(20);
    let num_streams_per_conn = value_t!(matches, "num_streams_per_conn", u64).unwrap_or(20);
    let target_address = value_t!(matches, "target_address", String)
        .unwrap_or("127.0.0.1:8009".to_string())
        .parse()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(run_connection_dos(
        target_address,
        num_connections,
        num_streams_per_conn,
    ));
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crossbeam_channel::{unbounded, Receiver},
        log::warn,
        solana_core::validator::ValidatorConfig,
        solana_gossip::contact_info::LegacyContactInfo,
        solana_local_cluster::{
            cluster::Cluster,
            cluster_tests,
            local_cluster::{ClusterConfig, LocalCluster},
            validator_configs::make_identical_validator_configs,
        },
        solana_perf::packet::PacketBatch,
        //solana_client::thin_client::ThinClient,
        solana_rpc::rpc::JsonRpcConfig,
        solana_sdk::net::DEFAULT_TPU_COALESCE,
        solana_streamer::{
            nonblocking::quic::spawn_server,
            quic::{StreamStats, MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
            socket::SocketAddrSpace,
            streamer::StakedNodes,
        },
        std::{
            sync::{
                atomic::{AtomicBool, Ordering},
                RwLock,
            },
            time::{Duration, Instant},
        },
        tokio::{task::JoinHandle, time::sleep},
    };

    fn setup_quic_server(
        option_staked_nodes: Option<StakedNodes>,
        max_connections_per_peer: usize,
    ) -> (
        JoinHandle<()>,
        Arc<AtomicBool>,
        Receiver<PacketBatch>,
        SocketAddr,
        Arc<StreamStats>,
    ) {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(option_staked_nodes.unwrap_or_default()));
        let stats = Arc::new(StreamStats::default());
        let (_, t) = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            max_connections_per_peer,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats.clone(),
            Duration::from_secs(2),
            DEFAULT_TPU_COALESCE,
        )
        .unwrap();
        (t, exit, receiver, server_address, stats)
    }

    #[tokio::test]
    async fn test_connection_dos() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server(None, 1);

        //let tx_client = ThinClient::new(rpc, tpu, cluster.connection_cache.clone());

        const NUM_CONN: u64 = 1;
        const NUM_STREAMS_PER_CONN: u64 = 2;

        tokio::spawn(run_connection_dos(
            server_address,
            NUM_CONN,
            NUM_STREAMS_PER_CONN,
        ));

        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        let num_expected_packets = (NUM_CONN * NUM_STREAMS_PER_CONN) as usize;
        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        while now.elapsed().as_secs() < 60 {
            // We're running in an async environment, we (almost) never
            // want to block
            if let Ok(packets) = receiver.try_recv() {
                total_packets += packets.len();
                all_packets.push(packets)
            } else {
                sleep(Duration::from_secs(1)).await;
            }
            if total_packets >= num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in batch.iter() {
                assert_eq!(p.meta().size, num_bytes);
            }
        }
        assert_eq!(total_packets, num_expected_packets);

        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[test]
    #[ignore]
    fn test_local_cluster() {
        solana_logger::setup();

        const NUM_NODES: usize = 2;
        let cluster = LocalCluster::new(
            &mut ClusterConfig {
                node_stakes: vec![999_990; NUM_NODES],
                cluster_lamports: 200_000_000,
                validator_configs: make_identical_validator_configs(
                    &ValidatorConfig {
                        rpc_config: JsonRpcConfig {
                            //faucet_addr: Some(faucet_addr),
                            ..JsonRpcConfig::default_for_test()
                        },
                        ..ValidatorConfig::default_for_test()
                    },
                    NUM_NODES,
                ),
                //native_instruction_processors,
                //additional_accounts,
                ..ClusterConfig::default()
            },
            SocketAddrSpace::Unspecified,
        );

        //cluster.transfer(&cluster.funding_keypair, &faucet_pubkey, 100_000_000);

        let nodes = cluster.get_node_pubkeys();
        warn!("{:?}", nodes);
        let non_bootstrap_id = nodes
            .into_iter()
            .find(|id| id != cluster.entry_point_info.pubkey())
            .unwrap();

        let non_bootstrap_info = cluster.get_contact_info(&non_bootstrap_id).unwrap();

        let (_rpc, tpu) = LegacyContactInfo::try_from(non_bootstrap_info)
            .map(cluster_tests::get_client_facing_addr)
            .unwrap();
        //let tx_client = ThinClient::new(rpc, tpu, cluster.connection_cache.clone());

        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(run_connection_dos(tpu, 1, 1));
    }
}
