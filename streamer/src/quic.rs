use {
    crossbeam_channel::Sender,
    futures_util::stream::StreamExt,
    pem::Pem,
    pkcs8::{der::Document, AlgorithmIdentifier, ObjectIdentifier},
    quinn::{Endpoint, EndpointConfig, ServerConfig},
    rcgen::{CertificateParams, DistinguishedName, DnType, SanType},
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        packet::{Packet, PACKET_DATA_SIZE},
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

    const MAX_CONCURRENT_UNI_STREAMS: u32 = 1;
    config.max_concurrent_uni_streams(MAX_CONCURRENT_UNI_STREAMS.into());
    config.stream_receive_window((PACKET_DATA_SIZE as u32).into());
    config.receive_window((PACKET_DATA_SIZE as u32 * MAX_CONCURRENT_UNI_STREAMS).into());

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

struct ConnectionEntry {
    count: usize,
    exit: Arc<AtomicBool>,
    last_update: AtomicU64,
}

impl ConnectionEntry {
    fn new(exit: Arc<AtomicBool>, start: u64) -> Self {
        Self {
            count: 0,
            exit,
            last_update: AtomicU64::new(start),
        }
    }

    pub fn last_update(&self) -> u64 {
        self.last_update.load(Ordering::Relaxed)
    }
}

impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
    }
}

// Return number pruned
fn prune_oldest_from_connection_table(
    table: &mut HashMap<IpAddr, ConnectionEntry>,
    max_size: usize,
) -> usize {
    let mut num_pruned = 0;
    while table.len() > max_size {
        let mut oldest = std::u64::MAX;
        let mut oldest_ip = None;
        for (ip, entry) in table.iter() {
            let last_update = entry.last_update();
            if last_update < oldest {
                oldest = last_update;
                oldest_ip = Some(*ip);
            }
        }
        table.remove(&oldest_ip.unwrap());
        num_pruned += 1;
    }
    num_pruned
}

pub fn spawn_server(
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_ip: usize,
) -> Result<thread::JoinHandle<()>, QuicServerError> {
    let (config, _cert) = configure_server(keypair, gossip_host)?;

    let runtime = rt();
    let (_, mut incoming) = {
        let _guard = runtime.enter();
        Endpoint::new(EndpointConfig::default(), Some(config), sock)
            .map_err(|_e| QuicServerError::EndpointFailed)?
    };

    let total_streams = Arc::new(AtomicUsize::new(0));
    let total_new_streams = Arc::new(AtomicUsize::new(0));
    let handle = thread::spawn(move || {
        let handle = runtime.spawn(async move {
            debug!("spawn quic server");
            let total_connections = Arc::new(AtomicUsize::new(0));
            let num_evictions = Arc::new(AtomicUsize::new(0));
            let total_new_connections = Arc::new(AtomicUsize::new(0));
            let mut last_datapoint = Instant::now();
            let connection_table: Arc<RwLock<HashMap<IpAddr, ConnectionEntry>>> =
                Arc::new(RwLock::new(HashMap::new()));
            while !exit.load(Ordering::Relaxed) {
                const WAIT_FOR_CONNECTION_TIMEOUT_MS: u64 = 1000;
                let timeout_connection = timeout(
                    Duration::from_millis(WAIT_FOR_CONNECTION_TIMEOUT_MS),
                    incoming.next(),
                )
                .await;

                if last_datapoint.elapsed().as_secs() >= 5 {
                    datapoint_info!(
                        "quic-connections",
                        (
                            "active_connections",
                            total_connections.load(Ordering::Relaxed),
                            i64
                        ),
                        ("active_streams", total_streams.load(Ordering::Relaxed), i64),
                        (
                            "new_connections",
                            total_new_connections.swap(0, Ordering::Relaxed),
                            i64
                        ),
                        (
                            "new_streams",
                            total_new_streams.swap(0, Ordering::Relaxed),
                            i64
                        ),
                        ("evictions", num_evictions.swap(0, Ordering::Relaxed), i64),
                    );
                    last_datapoint = Instant::now();
                }

                if let Ok(Some(connection)) = timeout_connection {
                    if let Ok(new_connection) = connection.await {
                        let total_connections = total_connections.clone();
                        total_connections.fetch_add(1, Ordering::Relaxed);
                        total_new_connections.fetch_add(1, Ordering::Relaxed);
                        let quinn::NewConnection {
                            connection,
                            mut uni_streams,
                            ..
                        } = new_connection;

                        let remote_addr = connection.remote_address();

                        let mut connection_table_w = connection_table.write().unwrap();
                        const MAX_CONNECTION_TABLE_SIZE: usize = 5000;
                        let num_pruned = prune_oldest_from_connection_table(
                            &mut connection_table_w,
                            MAX_CONNECTION_TABLE_SIZE,
                        );
                        num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                        let connection_entry = connection_table_w
                            .entry(remote_addr.ip())
                            .or_insert_with(|| {
                                ConnectionEntry::new(
                                    Arc::new(AtomicBool::new(false)),
                                    timing::timestamp(),
                                )
                            });
                        if (connection_entry.count + 1) <= max_connections_per_ip {
                            connection_entry.count += 1;
                            let stream_exit = connection_entry.exit.clone();
                            drop(connection_table_w);
                            let packet_sender = packet_sender.clone();
                            let total_streams = total_streams.clone();
                            let total_new_streams = total_new_streams.clone();
                            let total_connections1 = total_connections.clone();
                            let connection_table1 = connection_table.clone();
                            tokio::spawn(async move {
                                debug!(
                                    "quic new connection {} streams: {} connections: {}",
                                    remote_addr,
                                    total_streams.load(Ordering::Relaxed),
                                    total_connections1.load(Ordering::Relaxed),
                                );
                                while !stream_exit.load(Ordering::Relaxed) {
                                    match uni_streams.next().await {
                                        Some(stream_result) => match stream_result {
                                            Ok(mut stream) => {
                                                total_streams.fetch_add(1, Ordering::Relaxed);
                                                total_new_streams.fetch_add(1, Ordering::Relaxed);
                                                let mut maybe_batch = None;
                                                while !stream_exit.load(Ordering::Relaxed) {
                                                    if handle_chunk(
                                                        &stream
                                                            .read_chunk(PACKET_DATA_SIZE, false)
                                                            .await,
                                                        &mut maybe_batch,
                                                        &remote_addr,
                                                        &packet_sender,
                                                    ) {
                                                        let connection_table_r =
                                                            connection_table1.read().unwrap();
                                                        if let Some(e) = connection_table_r
                                                            .get(&remote_addr.ip())
                                                        {
                                                            e.last_update.store(
                                                                timing::timestamp(),
                                                                Ordering::Relaxed,
                                                            );
                                                        }
                                                        break;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                debug!("stream error: {:?}", e);
                                                total_streams.fetch_sub(1, Ordering::Relaxed);
                                                break;
                                            }
                                        },
                                        None => {
                                            total_streams.fetch_sub(1, Ordering::Relaxed);
                                            break;
                                        }
                                    }
                                }
                                {
                                    let mut connection_table_w = connection_table1.write().unwrap();
                                    if let Entry::Occupied(mut e) =
                                        connection_table_w.entry(remote_addr.ip())
                                    {
                                        let e_ref = e.get_mut();
                                        e_ref.count -= 1;
                                        if e_ref.count == 0 {
                                            e.remove_entry();
                                        }
                                    }
                                }
                                total_connections.fetch_sub(1, Ordering::Relaxed);
                            });
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
        ClientConfig::new(Arc::new(crypto))
    }

    #[test]
    fn test_quic_server_exit() {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, _receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let t = spawn_server(s, &keypair, ip, sender, exit.clone(), 1).unwrap();
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
    fn test_quic_server_block_multiple_connections() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, _receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let t = spawn_server(s, &keypair, ip, sender, exit.clone(), 1).unwrap();

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
        let t = spawn_server(s, &keypair, ip, sender, exit.clone(), 2).unwrap();

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

    #[test]
    fn test_quic_server_multiple_writes() {
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
        let mut table = HashMap::new();
        let exit = Arc::new(AtomicBool::new(false));
        let num_entries = 5;
        for i in 0..num_entries {
            table.insert(
                IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)),
                ConnectionEntry::new(exit.clone(), (i + 1) as u64),
            );
        }
        let new_size = 3;
        let pruned = prune_oldest_from_connection_table(&mut table, new_size);
        assert_eq!(pruned, num_entries as usize - new_size);
        for v in table.values() {
            assert!(v.last_update() >= (num_entries as u64 - new_size as u64));
        }
        assert_eq!(table.len(), new_size);
    }
}
