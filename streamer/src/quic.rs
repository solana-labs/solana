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
    },
    std::{
        error::Error,
        net::{IpAddr, SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        thread,
        time::Duration,
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
    config.receive_window((10_000 * PACKET_DATA_SIZE as u32 * MAX_CONCURRENT_UNI_STREAMS).into());
    config.datagram_receive_buffer_size(Some(PACKET_DATA_SIZE));

    // disable bidi & datagrams
    const MAX_CONCURRENT_BIDI_STREAMS: u32 = 0;
    config.max_concurrent_bidi_streams(MAX_CONCURRENT_BIDI_STREAMS.into());

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

pub fn rt() -> Runtime {
    //Builder::new_current_thread().enable_all().build().unwrap()
    Builder::new_multi_thread().enable_all().build().unwrap()
}

pub fn rt_mt() -> Runtime {
    Builder::new_multi_thread().enable_all().build().unwrap()
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

pub fn spawn_server(
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, QuicServerError> {
    let (config, _cert) = configure_server(keypair, gossip_host)?;

    let runtime = rt();
    let (_, mut incoming) = {
        let _guard = runtime.enter();
        Endpoint::new(EndpointConfig::default(), Some(config), sock)
            .map_err(|_e| QuicServerError::EndpointFailed)?
    };

    let handle = thread::spawn(move || {
        let handle = runtime.spawn(async move {
            while !exit.load(Ordering::Relaxed) {
                const WAIT_FOR_CONNECTION_TIMEOUT_MS: u64 = 1000;
                let timeout_connection = timeout(
                    Duration::from_millis(WAIT_FOR_CONNECTION_TIMEOUT_MS),
                    incoming.next(),
                )
                .await;

                if let Ok(Some(connection)) = timeout_connection {
                    if let Ok(new_connection) = connection.await {
                        let exit = exit.clone();
                        let quinn::NewConnection {
                            connection,
                            mut uni_streams,
                            mut datagrams,
                            ..
                        } = new_connection;

                        let remote_addr = connection.remote_address();
                        let packet_sender_datagrams = packet_sender.clone();
                        let packet_sender_streams = packet_sender.clone();

                        trace!("new connection {}", remote_addr);
                        tokio::spawn(async move {
                            loop {
                                trace!("waiting for datagrams..");
                                match datagrams.next().await {
                                    Some(maybe_datagram) => match maybe_datagram {
                                        Ok(datagram) => {
                                            trace!("got datagram");

                                            let mut batch = PacketBatch::with_capacity(1);
                                            let mut packet = Packet::default();
                                            packet.data[..datagram.len()]
                                                .copy_from_slice(&datagram);
                                            packet.meta.set_addr(&remote_addr);
                                            packet.meta.size = datagram.len();
                                            batch.packets.push(packet);

                                            packet_sender_datagrams.send(batch).unwrap();
                                        }
                                        Err(e) => {
                                            trace!("error: {:?}", e);
                                            break;
                                        }
                                    },
                                    None => {
                                        trace!("datagrams none..");
                                        break;
                                    }
                                }
                            }
                        });

                        tokio::spawn(async move {
                            while let Some(Ok(mut stream)) = uni_streams.next().await {
                                let mut maybe_batch = None;
                                while !exit.load(Ordering::Relaxed) {
                                    if handle_chunk(
                                        &stream.read_chunk(PACKET_DATA_SIZE, false).await,
                                        &mut maybe_batch,
                                        &remote_addr,
                                        &packet_sender_streams,
                                    ) {
                                        break;
                                    }
                                }
                            }
                        });
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
    use super::*;
    use crossbeam_channel::unbounded;
    use quinn::{ClientConfig, NewConnection};
    use std::{net::SocketAddr, time::Instant};

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
        let transport = Arc::get_mut(&mut config.transport).unwrap();
        transport.datagram_send_buffer_size(100 * 1024 * 1024);
        transport.datagram_receive_buffer_size(Some(100 * 1024 * 1024));
        config
    }

    #[test]
    fn test_quic_server_exit() {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, _receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let t = spawn_server(s, &keypair, ip, sender, exit.clone()).unwrap();
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

    fn get_env(name: &'static str, default: usize) -> usize {
        std::env::var(name)
            .map(|x| x.parse().unwrap())
            .unwrap_or(default)
    }

    #[test]
    fn test_quic_datagrams() {
        solana_logger::setup();
        use solana_measure::measure::Measure;
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let t = spawn_server(s, &keypair, ip, sender, exit.clone()).unwrap();

        //let packet = vec![20u8; max_datagram_size];
        let packets_per_thread = get_env("PACKETS_PER_THREAD", 1000);
        let batch_size = get_env("BATCH_SIZE", 1);
        let num_threads = get_env("NUM_THREADS", 100);
        let packet_size = get_env("PACKET_SIZE", 100);

        let num_batches = packets_per_thread / batch_size;
        let num_packets = num_threads * packets_per_thread;

        let start = Instant::now();
        let mut time = Measure::start("total_send_receive");
        let mut send_time = Measure::start("start_send_datagrams");
        let send_threads: Vec<_> = (0..num_threads)
            .into_iter()
            .map(|tid| {
                thread::spawn(move || {
                    let runtime = rt_mt();
                    let _rt_guard = runtime.enter();
                    let conn = Arc::new(make_client_endpoint(&runtime, &server_address));
                    if tid == 0 {
                        let max_datagram_size = conn.connection.max_datagram_size().unwrap();
                        info!("max_datagram_size: {:?}", max_datagram_size);
                    }

                    let packet = vec![20u8; packet_size];
                    for _ in 0..num_batches {
                        //info!("sending..");
                        for _ in 0..batch_size {
                            conn.connection
                                .send_datagram(packet.clone().into())
                                .unwrap();
                        }
                        // send things in batches and slowly to prevent congestion control.
                        std::thread::sleep(Duration::from_millis(5));
                    }
                })
            })
            .collect();
        for st in send_threads {
            st.join().unwrap();
        }
        send_time.stop();
        info!("{} polling now for received", send_time);
        let mut receive_time = Measure::start("receive datagrams");
        let mut num_received = 0;
        let mut last_received = 0;
        let mut start_recv = Instant::now();
        loop {
            if let Ok(received) = receiver.recv_timeout(Duration::from_millis(1000)) {
                num_received += received.packets.len();
            }
            if num_received >= num_packets {
                break;
            }
            if start_recv.elapsed().as_secs() >= 2 {
                info!(
                    "waiting for {} received: {} tps: {:.2}",
                    num_packets,
                    num_received,
                    num_received as f32 / start.elapsed().as_secs_f32()
                );
                start_recv = Instant::now();
                if num_received > 0 && num_received <= last_received {
                    break;
                }
                last_received = num_received;
            }
        }
        time.stop();
        receive_time.stop();
        info!(
            "{} {} tps: {:.2}",
            time,
            receive_time,
            num_received as f32 / time.as_s()
        );
        /*let packet0 = &received.packets[0];
        assert_eq!(packet0.data[..packet0.meta.size], packet[..]);*/
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
        let t = spawn_server(s, &keypair, ip, sender, exit.clone()).unwrap();

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
        let t = spawn_server(s, &keypair, ip, sender, exit.clone()).unwrap();

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
}
