#![allow(clippy::integer_arithmetic)]
use {
    clap::{crate_description, crate_name, value_t, App, Arg},
    quinn::{ClientConfig, Connection, EndpointConfig, IdleTimeout, TokioRuntime, TransportConfig},
    futures::future::join_all,
    //rayon::prelude::*,
    //solana_measure::measure::Measure,
    solana_sdk::{
        quic::{QUIC_KEEP_ALIVE, QUIC_MAX_TIMEOUT},
        signer::keypair::Keypair,
        packet::PACKET_DATA_SIZE,
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

pub async fn make_client_connection(
    addr: &SocketAddr,
    client_keypair: Option<&Keypair>,
) -> Connection {
    let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut endpoint =
        quinn::Endpoint::new(EndpointConfig::default(), None, client_socket, TokioRuntime).unwrap();
    let default_keypair = Keypair::new();
    endpoint.set_default_client_config(get_client_config(
        client_keypair.unwrap_or(&default_keypair),
    ));
    endpoint
        .connect(*addr, "localhost")
        .expect("Failed in connecting")
        .await
        .expect("Failed in waiting")
}

// Opens a slow stream and tries to send PACKET_DATA_SIZE bytes of junk
// in as many chunks as possible. We don't allow the number of chunks
// to be configurable as client-side writes don't correspond to
// quic-level packets/writes (but by doing multiple writes we are generally able
// to get multiple writes on the quic level despite the spec not guaranteeing this)
pub async fn check_multiple_writes(
    conn: Arc<Connection>
) {
    // Send a full size packet with single byte writes.
    let num_bytes = PACKET_DATA_SIZE;
    let mut s1 = conn.open_uni().await.unwrap();
    for _ in 0..num_bytes {
        s1.write_all(&[0u8]).await.unwrap();
    }
    s1.finish().await.unwrap();
}

async fn run_connection_dos(server_address: SocketAddr, num_connections: u64, num_streams_per_conn: u64) {
    let mut connections = vec![];
    for _ in 0..num_connections {
        connections.push(make_client_connection(&server_address, None).await);
    }

    let futures: Vec<_> = connections
        .into_iter()
        .map(|conn| {
            let conn = Arc::new(conn);
            join_all((0..num_streams_per_conn).map(|_| check_multiple_writes(conn.clone())))
        })
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
        .unwrap_or("127.0.0.1".to_string())
        .parse()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(run_connection_dos(target_address, num_connections, num_streams_per_conn));
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        log::warn,
        solana_core::validator::ValidatorConfig,
        solana_gossip::contact_info::LegacyContactInfo,
        solana_local_cluster::{
            cluster::Cluster,
            cluster_tests,
            local_cluster::{ClusterConfig, LocalCluster},
            validator_configs::make_identical_validator_configs,
        },
        //solana_client::thin_client::ThinClient,
        solana_rpc::rpc::JsonRpcConfig,
        solana_streamer::socket::SocketAddrSpace,
    };

    #[test]
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
