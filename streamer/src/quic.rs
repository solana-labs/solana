use {
    crossbeam_channel::Sender,
    futures_util::stream::StreamExt,
    pem::Pem,
    pkcs8::{der::Document, AlgorithmIdentifier, ObjectIdentifier},
    quinn::{Endpoint, EndpointConfig, IdleTimeout, IncomingUniStreams, ServerConfig, VarInt},
    rcgen::{CertificateParams, DistinguishedName, DnType, SanType},
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        packet::{Packet, PACKET_DATA_SIZE},
        quic::{QUIC_MAX_CONCURRENT_STREAMS, QUIC_MAX_TIMEOUT_MS},
        signature::Keypair,
        timing,
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        error::Error,
        net::{IpAddr, SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
            Arc, Mutex, RwLock,
        },
        thread,
        time::{Duration, Instant},
    },
    tokio::{
        runtime::{Builder, Runtime},
        time::timeout,
    },
};

pub const MAX_STAKED_CONNECTIONS: usize = 2000;
pub const MAX_UNSTAKED_CONNECTIONS: usize = 500;

/// Returns default server configuration along with its PEM certificate chain.
#[allow(clippy::field_reassign_with_default)] // https://github.com/rust-lang/rust-clippy/issues/6527
fn configure_server(
    identity_keypair: &Keypair,
    gossip_host: IpAddr,
) -> Result<(ServerConfig, String), QuicServerError> {
    let (cert_chain, priv_key) =
        new_cert(identity_keypair, gossip_host).map_err(|_e| QuicServerError::ConfigureFailed)?;
    let cert_chain_pem_parts: Vec<Pem> = cert_chain
        .iter()
        .map(|cert| Pem {
            tag: "CERTIFICATE".to_string(),
            contents: cert.0.clone(),
        })
        .collect();
    let cert_chain_pem = pem::encode_many(&cert_chain_pem_parts);

    let mut server_config = ServerConfig::with_single_cert(cert_chain, priv_key)
        .map_err(|_e| QuicServerError::ConfigureFailed)?;
    let config = Arc::get_mut(&mut server_config.transport).unwrap();

    // QUIC_MAX_CONCURRENT_STREAMS doubled, which was found to improve reliability
    const MAX_CONCURRENT_UNI_STREAMS: u32 = (QUIC_MAX_CONCURRENT_STREAMS * 2) as u32;
    config.max_concurrent_uni_streams(MAX_CONCURRENT_UNI_STREAMS.into());
    config.stream_receive_window((PACKET_DATA_SIZE as u32).into());
    config.receive_window((PACKET_DATA_SIZE as u32 * MAX_CONCURRENT_UNI_STREAMS).into());
    let timeout = IdleTimeout::from(VarInt::from_u32(QUIC_MAX_TIMEOUT_MS));
    config.max_idle_timeout(Some(timeout));

    // disable bidi & datagrams
    const MAX_CONCURRENT_BIDI_STREAMS: u32 = 0;
    config.max_concurrent_bidi_streams(MAX_CONCURRENT_BIDI_STREAMS.into());
    config.datagram_receive_buffer_size(None);

    Ok((server_config, cert_chain_pem))
}

fn new_cert(
    identity_keypair: &Keypair,
    san: IpAddr,
) -> Result<(Vec<rustls::Certificate>, rustls::PrivateKey), Box<dyn Error>> {
    // Generate a self-signed cert from validator identity key
    let cert_params = new_cert_params(identity_keypair, san);
    let cert = rcgen::Certificate::from_params(cert_params)?;
    let cert_der = cert.serialize_der().unwrap();
    let priv_key = cert.serialize_private_key_der();
    let priv_key = rustls::PrivateKey(priv_key);
    let cert_chain = vec![rustls::Certificate(cert_der)];
    Ok((cert_chain, priv_key))
}

fn convert_to_rcgen_keypair(identity_keypair: &Keypair) -> rcgen::KeyPair {
    // from https://datatracker.ietf.org/doc/html/rfc8410#section-3
    const ED25519_IDENTIFIER: [u32; 4] = [1, 3, 101, 112];
    let mut private_key = Vec::<u8>::with_capacity(34);
    private_key.extend_from_slice(&[0x04, 0x20]); // ASN.1 OCTET STRING
    private_key.extend_from_slice(identity_keypair.secret().as_bytes());
    let key_pkcs8 = pkcs8::PrivateKeyInfo {
        algorithm: AlgorithmIdentifier {
            oid: ObjectIdentifier::from_arcs(&ED25519_IDENTIFIER).unwrap(),
            parameters: None,
        },
        private_key: &private_key,
        public_key: None,
    };
    let key_pkcs8_der = key_pkcs8
        .to_der()
        .expect("Failed to convert keypair to DER")
        .to_der();

    // Parse private key into rcgen::KeyPair struct.
    rcgen::KeyPair::from_der(&key_pkcs8_der).expect("Failed to parse keypair from DER")
}

fn new_cert_params(identity_keypair: &Keypair, san: IpAddr) -> CertificateParams {
    // TODO(terorie): Is it safe to sign the TLS cert with the identity private key?

    // Unfortunately, rcgen does not accept a "raw" Ed25519 key.
    // We have to convert it to DER and pass it to the library.

    // Convert private key into PKCS#8 v1 object.
    // RFC 8410, Section 7: Private Key Format
    // https://datatracker.ietf.org/doc/html/rfc8410#section-

    let keypair = convert_to_rcgen_keypair(identity_keypair);

    let mut cert_params = CertificateParams::default();
    cert_params.subject_alt_names = vec![SanType::IpAddress(san)];
    cert_params.alg = &rcgen::PKCS_ED25519;
    cert_params.key_pair = Some(keypair);
    cert_params.distinguished_name = DistinguishedName::new();
    cert_params
        .distinguished_name
        .push(DnType::CommonName, "Solana node");
    cert_params
}

fn rt() -> Runtime {
    Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap()
}

#[derive(thiserror::Error, Debug)]
pub enum QuicServerError {
    #[error("Server configure failed")]
    ConfigureFailed,

    #[error("Endpoint creation failed")]
    EndpointFailed,
}

// Return true if the server should drop the stream
fn handle_chunk(
    chunk: &Result<Option<quinn::Chunk>, quinn::ReadError>,
    maybe_batch: &mut Option<PacketBatch>,
    remote_addr: &SocketAddr,
    packet_sender: &Sender<PacketBatch>,
) -> bool {
    match chunk {
        Ok(maybe_chunk) => {
            if let Some(chunk) = maybe_chunk {
                trace!("got chunk: {:?}", chunk);
                let chunk_len = chunk.bytes.len() as u64;

                // shouldn't happen, but sanity check the size and offsets
                if chunk.offset > PACKET_DATA_SIZE as u64 || chunk_len > PACKET_DATA_SIZE as u64 {
                    return true;
                }
                if chunk.offset + chunk_len > PACKET_DATA_SIZE as u64 {
                    return true;
                }

                // chunk looks valid
                if maybe_batch.is_none() {
                    let mut batch = PacketBatch::with_capacity(1);
                    let mut packet = Packet::default();
                    packet.meta.set_addr(remote_addr);
                    batch.packets.push(packet);
                    *maybe_batch = Some(batch);
                }

                if let Some(batch) = maybe_batch.as_mut() {
                    let end = chunk.offset as usize + chunk.bytes.len();
                    batch.packets[0].data[chunk.offset as usize..end].copy_from_slice(&chunk.bytes);
                    batch.packets[0].meta.size = std::cmp::max(batch.packets[0].meta.size, end);
                }
            } else {
                trace!("chunk is none");
                // done receiving chunks
                if let Some(batch) = maybe_batch.take() {
                    let len = batch.packets[0].meta.size;
                    if let Err(e) = packet_sender.send(batch) {
                        info!("send error: {}", e);
                    } else {
                        trace!("sent {} byte packet", len);
                    }
                }
                return true;
            }
        }
        Err(e) => {
            debug!("Received stream error: {:?}", e);
            return true;
        }
    }
    false
}

#[derive(Debug)]
struct ConnectionEntry {
    exit: Arc<AtomicBool>,
    last_update: Arc<AtomicU64>,
    port: u16,
}

impl ConnectionEntry {
    fn new(exit: Arc<AtomicBool>, last_update: Arc<AtomicU64>, port: u16) -> Self {
        Self {
            exit,
            last_update,
            port,
        }
    }

    fn last_update(&self) -> u64 {
        self.last_update.load(Ordering::Relaxed)
    }
}

impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
    }
}

// Map of IP to list of connection entries
#[derive(Default, Debug)]
struct ConnectionTable {
    table: HashMap<IpAddr, Vec<ConnectionEntry>>,
    total_size: usize,
}

// Prune the connection which has the oldest update
// Return number pruned
impl ConnectionTable {
    fn prune_oldest(&mut self, max_size: usize) -> usize {
        let mut num_pruned = 0;
        while self.total_size > max_size {
            let mut oldest = std::u64::MAX;
            let mut oldest_ip = None;
            for (ip, connections) in self.table.iter() {
                for entry in connections {
                    let last_update = entry.last_update();
                    if last_update < oldest {
                        oldest = last_update;
                        oldest_ip = Some(*ip);
                    }
                }
            }
            self.table.remove(&oldest_ip.unwrap());
            self.total_size -= 1;
            num_pruned += 1;
        }
        num_pruned
    }

    fn try_add_connection(
        &mut self,
        addr: &SocketAddr,
        last_update: u64,
        max_connections_per_ip: usize,
    ) -> Option<(Arc<AtomicU64>, Arc<AtomicBool>)> {
        let connection_entry = self.table.entry(addr.ip()).or_insert_with(Vec::new);
        let has_connection_capacity = connection_entry
            .len()
            .checked_add(1)
            .map(|c| c <= max_connections_per_ip)
            .unwrap_or(false);
        if has_connection_capacity {
            let exit = Arc::new(AtomicBool::new(false));
            let last_update = Arc::new(AtomicU64::new(last_update));
            connection_entry.push(ConnectionEntry::new(
                exit.clone(),
                last_update.clone(),
                addr.port(),
            ));
            self.total_size += 1;
            Some((last_update, exit))
        } else {
            None
        }
    }

    fn remove_connection(&mut self, addr: &SocketAddr) {
        if let Entry::Occupied(mut e) = self.table.entry(addr.ip()) {
            let e_ref = e.get_mut();
            e_ref.retain(|connection| connection.port != addr.port());
            if e_ref.is_empty() {
                e.remove_entry();
            }
            self.total_size -= 1;
        }
    }
}

#[derive(Default)]
struct StreamStats {
    total_connections: AtomicUsize,
    total_new_connections: AtomicUsize,
    total_streams: AtomicUsize,
    total_new_streams: AtomicUsize,
    num_evictions: AtomicUsize,
}

impl StreamStats {
    fn report(&self) {
        datapoint_info!(
            "quic-connections",
            (
                "active_connections",
                self.total_connections.load(Ordering::Relaxed),
                i64
            ),
            (
                "active_streams",
                self.total_streams.load(Ordering::Relaxed),
                i64
            ),
            (
                "new_connections",
                self.total_new_connections.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "new_streams",
                self.total_new_streams.swap(0, Ordering::Relaxed),
                i64
            ),
            (
                "evictions",
                self.num_evictions.swap(0, Ordering::Relaxed),
                i64
            ),
        );
    }
}

fn handle_connection(
    mut uni_streams: IncomingUniStreams,
    packet_sender: Sender<PacketBatch>,
    remote_addr: SocketAddr,
    last_update: Arc<AtomicU64>,
    connection_table: Arc<Mutex<ConnectionTable>>,
    stream_exit: Arc<AtomicBool>,
    stats: Arc<StreamStats>,
) {
    tokio::spawn(async move {
        debug!(
            "quic new connection {} streams: {} connections: {}",
            remote_addr,
            stats.total_streams.load(Ordering::Relaxed),
            stats.total_connections.load(Ordering::Relaxed),
        );
        while !stream_exit.load(Ordering::Relaxed) {
            match uni_streams.next().await {
                Some(stream_result) => match stream_result {
                    Ok(mut stream) => {
                        stats.total_streams.fetch_add(1, Ordering::Relaxed);
                        stats.total_new_streams.fetch_add(1, Ordering::Relaxed);
                        let mut maybe_batch = None;
                        while !stream_exit.load(Ordering::Relaxed) {
                            if handle_chunk(
                                &stream.read_chunk(PACKET_DATA_SIZE, false).await,
                                &mut maybe_batch,
                                &remote_addr,
                                &packet_sender,
                            ) {
                                last_update.store(timing::timestamp(), Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        debug!("stream error: {:?}", e);
                        stats.total_streams.fetch_sub(1, Ordering::Relaxed);
                        break;
                    }
                },
                None => {
                    stats.total_streams.fetch_sub(1, Ordering::Relaxed);
                    break;
                }
            }
        }
        connection_table
            .lock()
            .unwrap()
            .remove_connection(&remote_addr);
        stats.total_connections.fetch_sub(1, Ordering::Relaxed);
    });
}

pub fn spawn_server(
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_ip: usize,
    staked_nodes: Arc<RwLock<HashMap<IpAddr, u64>>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
) -> Result<thread::JoinHandle<()>, QuicServerError> {
    let (config, _cert) = configure_server(keypair, gossip_host)?;

    let runtime = rt();
    let (_, mut incoming) = {
        let _guard = runtime.enter();
        Endpoint::new(EndpointConfig::default(), Some(config), sock)
            .map_err(|_e| QuicServerError::EndpointFailed)?
    };

    let stats = Arc::new(StreamStats::default());
    let handle = thread::spawn(move || {
        let handle = runtime.spawn(async move {
            debug!("spawn quic server");
            let mut last_datapoint = Instant::now();
            let connection_table: Arc<Mutex<ConnectionTable>> =
                Arc::new(Mutex::new(ConnectionTable::default()));
            let staked_connection_table: Arc<Mutex<ConnectionTable>> =
                Arc::new(Mutex::new(ConnectionTable::default()));
            while !exit.load(Ordering::Relaxed) {
                const WAIT_FOR_CONNECTION_TIMEOUT_MS: u64 = 1000;
                let timeout_connection = timeout(
                    Duration::from_millis(WAIT_FOR_CONNECTION_TIMEOUT_MS),
                    incoming.next(),
                )
                .await;

                if last_datapoint.elapsed().as_secs() >= 5 {
                    stats.report();
                    last_datapoint = Instant::now();
                }

                if let Ok(Some(connection)) = timeout_connection {
                    if let Ok(new_connection) = connection.await {
                        stats.total_connections.fetch_add(1, Ordering::Relaxed);
                        stats.total_new_connections.fetch_add(1, Ordering::Relaxed);
                        let quinn::NewConnection {
                            connection,
                            uni_streams,
                            ..
                        } = new_connection;

                        let remote_addr = connection.remote_address();

                        let mut connection_table_l =
                            if staked_nodes.read().unwrap().contains_key(&remote_addr.ip()) {
                                let mut connection_table_l =
                                    staked_connection_table.lock().unwrap();
                                let num_pruned =
                                    connection_table_l.prune_oldest(max_staked_connections);
                                stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                                connection_table_l
                            } else {
                                let mut connection_table_l = connection_table.lock().unwrap();
                                let num_pruned =
                                    connection_table_l.prune_oldest(max_unstaked_connections);
                                stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                                connection_table_l
                            };

                        if let Some((last_update, stream_exit)) = connection_table_l
                            .try_add_connection(
                                &remote_addr,
                                timing::timestamp(),
                                max_connections_per_ip,
                            )
                        {
                            drop(connection_table_l);
                            let packet_sender = packet_sender.clone();
                            let stats = stats.clone();
                            let connection_table1 = connection_table.clone();
                            handle_connection(
                                uni_streams,
                                packet_sender,
                                remote_addr,
                                last_update,
                                connection_table1,
                                stream_exit,
                                stats,
                            );
                        }
                    }
                }
            }
        });
        if let Err(e) = runtime.block_on(handle) {
            warn!("error from runtime.block_on: {:?}", e);
        }
    });
    Ok(handle)
}

#[cfg(test)]
mod test {
    use {
        super::*,
        crossbeam_channel::unbounded,
        quinn::{ClientConfig, NewConnection},
        solana_sdk::quic::QUIC_KEEP_ALIVE_MS,
        std::{net::SocketAddr, time::Instant},
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

    pub fn get_client_config() -> quinn::ClientConfig {
        let crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_no_client_auth();
        let mut config = ClientConfig::new(Arc::new(crypto));

        let transport_config = Arc::get_mut(&mut config.transport).unwrap();
        let timeout = IdleTimeout::from(VarInt::from_u32(QUIC_MAX_TIMEOUT_MS));
        transport_config.max_idle_timeout(Some(timeout));
        transport_config.keep_alive_interval(Some(Duration::from_millis(QUIC_KEEP_ALIVE_MS)));

        config
    }

    #[test]
    fn test_quic_server_exit() {
        let (t, exit, _receiver, _server_address) = setup_quic_server();

        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    fn make_client_endpoint(runtime: &Runtime, addr: &SocketAddr) -> NewConnection {
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, client_socket)
            .unwrap()
            .0;
        endpoint.set_default_client_config(get_client_config());
        runtime
            .block_on(endpoint.connect(*addr, "localhost").unwrap())
            .unwrap()
    }

    #[test]
    fn test_quic_timeout() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let t = spawn_server(s, &keypair, ip, sender, exit.clone(), 1).unwrap();

        let runtime = rt();
        let _rt_guard = runtime.enter();
        let conn1 = make_client_endpoint(&runtime, &server_address);
        let total = 30;
        let handle = runtime.spawn(async move {
            for i in 0..total {
                let mut s1 = conn1.connection.open_uni().await.unwrap();
                s1.write_all(&[0u8]).await.unwrap();
                s1.finish().await.unwrap();
                info!("done {}", i);
                std::thread::sleep(Duration::from_millis(1000));
            }
        });
        let mut received = 0;
        loop {
            if let Ok(_x) = receiver.recv_timeout(Duration::from_millis(500)) {
                received += 1;
                info!("got {}", received);
            }
            if received >= total {
                break;
            }
        }
        runtime.block_on(handle).unwrap();
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_server_block_multiple_connections() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address) = setup_quic_server();

        let runtime = rt();
        let _rt_guard = runtime.enter();
        let conn1 = make_client_endpoint(&runtime, &server_address);
        let conn2 = make_client_endpoint(&runtime, &server_address);
        let handle = runtime.spawn(async move {
            let mut s1 = conn1.connection.open_uni().await.unwrap();
            let mut s2 = conn2.connection.open_uni().await.unwrap();
            s1.write_all(&[0u8]).await.unwrap();
            s1.finish().await.unwrap();
            s2.write_all(&[0u8])
                .await
                .expect_err("shouldn't be able to open 2 connections");
        });
        runtime.block_on(handle).unwrap();
        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    #[test]
    fn test_quic_server_multiple_streams() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(HashMap::new()));
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            2,
            staked_nodes,
            10,
            10,
        )
        .unwrap();

        let runtime = rt();
        let _rt_guard = runtime.enter();
        let conn1 = Arc::new(make_client_endpoint(&runtime, &server_address));
        let conn2 = Arc::new(make_client_endpoint(&runtime, &server_address));
        let mut num_expected_packets = 0;
        for i in 0..10 {
            info!("sending: {}", i);
            let c1 = conn1.clone();
            let c2 = conn2.clone();
            let handle = runtime.spawn(async move {
                let mut s1 = c1.connection.open_uni().await.unwrap();
                let mut s2 = c2.connection.open_uni().await.unwrap();
                s1.write_all(&[0u8]).await.unwrap();
                s1.finish().await.unwrap();
                s2.write_all(&[0u8]).await.unwrap();
                s2.finish().await.unwrap();
            });
            runtime.block_on(handle).unwrap();
            num_expected_packets += 2;
            thread::sleep(Duration::from_millis(200));
        }
        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 10 {
            if let Ok(packets) = receiver.recv_timeout(Duration::from_secs(1)) {
                total_packets += packets.packets.len();
                all_packets.push(packets)
            }
            if total_packets == num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in &batch.packets {
                assert_eq!(p.meta.size, 1);
            }
        }
        assert_eq!(total_packets, num_expected_packets);

        exit.store(true, Ordering::Relaxed);
        t.join().unwrap();
    }

    fn setup_quic_server() -> (
        std::thread::JoinHandle<()>,
        Arc<AtomicBool>,
        crossbeam_channel::Receiver<PacketBatch>,
        SocketAddr,
    ) {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(HashMap::new()));
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
        )
        .unwrap();
        (t, exit, receiver, server_address)
    }

    #[test]
    fn test_quic_server_multiple_writes() {
        solana_logger::setup();
        let (t, exit, receiver, server_address) = setup_quic_server();

        let runtime = rt();
        let _rt_guard = runtime.enter();
        let conn1 = Arc::new(make_client_endpoint(&runtime, &server_address));

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets = 1;
        let handle = runtime.spawn(async move {
            let mut s1 = conn1.connection.open_uni().await.unwrap();
            for _ in 0..num_bytes {
                s1.write_all(&[0u8]).await.unwrap();
            }
            s1.finish().await.unwrap();
        });
        runtime.block_on(handle).unwrap();

        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 5 {
            if let Ok(packets) = receiver.recv_timeout(Duration::from_secs(1)) {
                total_packets += packets.packets.len();
                all_packets.push(packets)
            }
            if total_packets > num_expected_packets {
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

    #[test]
    fn test_prune_table() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::default();
        let num_entries = 5;
        let max_connections_per_ip = 10;
        let sockets: Vec<_> = (0..num_entries)
            .into_iter()
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(socket, i as u64, max_connections_per_ip)
                .unwrap();
        }
        let new_size = 3;
        let pruned = table.prune_oldest(new_size);
        assert_eq!(pruned, num_entries as usize - new_size);
        for v in table.table.values() {
            for x in v {
                assert!(x.last_update() >= (num_entries as u64 - new_size as u64));
            }
        }
        assert_eq!(table.table.len(), new_size);
        assert_eq!(table.total_size, new_size);
        for socket in sockets.iter().take(num_entries as usize).skip(new_size - 1) {
            table.remove_connection(socket);
        }
        info!("{:?}", table);
        assert_eq!(table.total_size, 0);
    }
}
