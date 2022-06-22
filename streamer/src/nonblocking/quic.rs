use {
    crate::{
        quic::{configure_server, QuicServerError, StreamStats},
        streamer::StakedNodes,
    },
    crossbeam_channel::Sender,
    futures_util::stream::StreamExt,
    percentage::Percentage,
    quinn::{
        Connecting, Endpoint, EndpointConfig, Incoming, IncomingUniStreams, NewConnection, VarInt,
    },
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        packet::{Packet, PACKET_DATA_SIZE},
        quic::QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS,
        signature::Keypair,
        timing,
    },
    std::{
        collections::{hash_map::Entry, HashMap},
        net::{IpAddr, SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex, RwLock,
        },
        time::{Duration, Instant},
    },
    tokio::{
        task::JoinHandle,
        time::{sleep, timeout},
    },
};

const QUIC_TOTAL_STAKED_CONCURRENT_STREAMS: f64 = 100_000f64;
const WAIT_FOR_STREAM_TIMEOUT_MS: u64 = 1;

#[allow(clippy::too_many_arguments)]
pub fn spawn_server(
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_ip: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
) -> Result<JoinHandle<()>, QuicServerError> {
    let (config, _cert) = configure_server(keypair, gossip_host)?;

    let (_, incoming) = {
        Endpoint::new(EndpointConfig::default(), Some(config), sock)
            .map_err(|_e| QuicServerError::EndpointFailed)?
    };

    let handle = tokio::spawn(run_server(
        incoming,
        packet_sender,
        exit,
        max_connections_per_ip,
        staked_nodes,
        max_staked_connections,
        max_unstaked_connections,
        stats,
    ));
    Ok(handle)
}

pub async fn run_server(
    mut incoming: Incoming,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_ip: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
) {
    debug!("spawn quic server");
    let mut last_datapoint = Instant::now();
    let connection_table: Arc<Mutex<ConnectionTable>> =
        Arc::new(Mutex::new(ConnectionTable::default()));
    let staked_connection_table: Arc<Mutex<ConnectionTable>> =
        Arc::new(Mutex::new(ConnectionTable::default()));
    while !exit.load(Ordering::Relaxed) {
        const WAIT_FOR_CONNECTION_TIMEOUT_MS: u64 = 1000;
        const WAIT_BETWEEN_NEW_CONNECTIONS_US: u64 = 1000;
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
            tokio::spawn(setup_connection(
                connection,
                connection_table.clone(),
                staked_connection_table.clone(),
                packet_sender.clone(),
                max_connections_per_ip,
                staked_nodes.clone(),
                max_staked_connections,
                max_unstaked_connections,
                stats.clone(),
            ));
            sleep(Duration::from_micros(WAIT_BETWEEN_NEW_CONNECTIONS_US)).await;
        }
    }
}

async fn setup_connection(
    connection: Connecting,
    connection_table: Arc<Mutex<ConnectionTable>>,
    staked_connection_table: Arc<Mutex<ConnectionTable>>,
    packet_sender: Sender<PacketBatch>,
    max_connections_per_ip: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
) {
    if let Ok(new_connection) = connection.await {
        stats.total_connections.fetch_add(1, Ordering::Relaxed);
        stats.total_new_connections.fetch_add(1, Ordering::Relaxed);
        let NewConnection {
            connection,
            uni_streams,
            ..
        } = new_connection;

        let remote_addr = connection.remote_address();

        let (mut connection_table_l, stake) = {
            const PRUNE_TABLE_TO_PERCENTAGE: u8 = 90;
            let max_percentage_full = Percentage::from(PRUNE_TABLE_TO_PERCENTAGE);

            let staked_nodes = staked_nodes.read().unwrap();
            if let Some(stake) = staked_nodes.stake_map.get(&remote_addr.ip()) {
                let stake = *stake;
                let total_stake = staked_nodes.total_stake;
                drop(staked_nodes);
                let mut connection_table_l = staked_connection_table.lock().unwrap();
                if connection_table_l.total_size >= max_staked_connections {
                    let max_connections = max_percentage_full.apply_to(max_staked_connections);
                    let num_pruned = connection_table_l.prune_oldest(max_connections);
                    stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                }
                connection.set_max_concurrent_uni_streams(
                    VarInt::from_u64(
                        ((stake as f64 / total_stake as f64) * QUIC_TOTAL_STAKED_CONCURRENT_STREAMS)
                            as u64,
                    )
                    .unwrap(),
                );
                (connection_table_l, stake)
            } else {
                drop(staked_nodes);
                let mut connection_table_l = connection_table.lock().unwrap();
                if connection_table_l.total_size >= max_unstaked_connections {
                    let max_connections = max_percentage_full.apply_to(max_unstaked_connections);
                    let num_pruned = connection_table_l.prune_oldest(max_connections);
                    stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                }
                connection.set_max_concurrent_uni_streams(
                    VarInt::from_u64(QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS as u64).unwrap(),
                );
                (connection_table_l, 0)
            }
        };

        if stake != 0 || max_unstaked_connections > 0 {
            if let Some((last_update, stream_exit)) = connection_table_l.try_add_connection(
                &remote_addr,
                timing::timestamp(),
                max_connections_per_ip,
            ) {
                drop(connection_table_l);
                let stats = stats.clone();
                let connection_table1 = connection_table.clone();
                tokio::spawn(handle_connection(
                    uni_streams,
                    packet_sender,
                    remote_addr,
                    last_update,
                    connection_table1,
                    stream_exit,
                    stats,
                    stake,
                ));
            } else {
                stats.connection_add_failed.fetch_add(1, Ordering::Relaxed);
            }
        } else {
            connection.close(0u32.into(), &[0u8]);
            stats
                .connection_add_failed_unstaked_node
                .fetch_add(1, Ordering::Relaxed);
        }
    } else {
        stats
            .connection_setup_timeout
            .fetch_add(1, Ordering::Relaxed);
    }
}

async fn handle_connection(
    mut uni_streams: IncomingUniStreams,
    packet_sender: Sender<PacketBatch>,
    remote_addr: SocketAddr,
    last_update: Arc<AtomicU64>,
    connection_table: Arc<Mutex<ConnectionTable>>,
    stream_exit: Arc<AtomicBool>,
    stats: Arc<StreamStats>,
    stake: u64,
) {
    debug!(
        "quic new connection {} streams: {} connections: {}",
        remote_addr,
        stats.total_streams.load(Ordering::Relaxed),
        stats.total_connections.load(Ordering::Relaxed),
    );
    while !stream_exit.load(Ordering::Relaxed) {
        if let Ok(stream) = tokio::time::timeout(
            Duration::from_millis(WAIT_FOR_STREAM_TIMEOUT_MS),
            uni_streams.next(),
        )
        .await
        {
            match stream {
                Some(stream_result) => match stream_result {
                    Ok(mut stream) => {
                        stats.total_streams.fetch_add(1, Ordering::Relaxed);
                        stats.total_new_streams.fetch_add(1, Ordering::Relaxed);
                        let mut maybe_batch = None;
                        while !stream_exit.load(Ordering::Relaxed) {
                            if let Ok(chunk) = tokio::time::timeout(
                                Duration::from_millis(WAIT_FOR_STREAM_TIMEOUT_MS),
                                stream.read_chunk(PACKET_DATA_SIZE, false),
                            )
                            .await
                            {
                                if handle_chunk(
                                    &chunk,
                                    &mut maybe_batch,
                                    &remote_addr,
                                    &packet_sender,
                                    stats.clone(),
                                    stake,
                                ) {
                                    last_update.store(timing::timestamp(), Ordering::Relaxed);
                                    break;
                                }
                            } else {
                                debug!("Timeout in receiving on stream");
                                stats
                                    .total_stream_read_timeouts
                                    .fetch_add(1, Ordering::Relaxed);
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
    }
    connection_table
        .lock()
        .unwrap()
        .remove_connection(&remote_addr);
    stats.total_connections.fetch_sub(1, Ordering::Relaxed);
}

// Return true if the server should drop the stream
fn handle_chunk(
    chunk: &Result<Option<quinn::Chunk>, quinn::ReadError>,
    maybe_batch: &mut Option<PacketBatch>,
    remote_addr: &SocketAddr,
    packet_sender: &Sender<PacketBatch>,
    stats: Arc<StreamStats>,
    stake: u64,
) -> bool {
    match chunk {
        Ok(maybe_chunk) => {
            if let Some(chunk) = maybe_chunk {
                trace!("got chunk: {:?}", chunk);
                let chunk_len = chunk.bytes.len() as u64;

                // shouldn't happen, but sanity check the size and offsets
                if chunk.offset > PACKET_DATA_SIZE as u64 || chunk_len > PACKET_DATA_SIZE as u64 {
                    stats.total_invalid_chunks.fetch_add(1, Ordering::Relaxed);
                    return true;
                }
                if chunk.offset + chunk_len > PACKET_DATA_SIZE as u64 {
                    stats
                        .total_invalid_chunk_size
                        .fetch_add(1, Ordering::Relaxed);
                    return true;
                }

                // chunk looks valid
                if maybe_batch.is_none() {
                    let mut batch = PacketBatch::with_capacity(1);
                    let mut packet = Packet::default();
                    packet.meta.set_socket_addr(remote_addr);
                    packet.meta.sender_stake = stake;
                    batch.push(packet);
                    *maybe_batch = Some(batch);
                    stats
                        .total_packets_allocated
                        .fetch_add(1, Ordering::Relaxed);
                }

                if let Some(batch) = maybe_batch.as_mut() {
                    let end = chunk.offset as usize + chunk.bytes.len();
                    batch[0].buffer_mut()[chunk.offset as usize..end].copy_from_slice(&chunk.bytes);
                    batch[0].meta.size = std::cmp::max(batch[0].meta.size, end);
                    stats.total_chunks_received.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                trace!("chunk is none");
                // done receiving chunks
                if let Some(batch) = maybe_batch.take() {
                    let len = batch[0].meta.size;
                    if let Err(e) = packet_sender.send(batch) {
                        stats
                            .total_packet_batch_send_err
                            .fetch_add(1, Ordering::Relaxed);
                        info!("send error: {}", e);
                    } else {
                        stats
                            .total_packet_batches_sent
                            .fetch_add(1, Ordering::Relaxed);
                        trace!("sent {} byte packet", len);
                    }
                } else {
                    stats
                        .total_packet_batches_none
                        .fetch_add(1, Ordering::Relaxed);
                }
                return true;
            }
        }
        Err(e) => {
            debug!("Received stream error: {:?}", e);
            stats
                .total_stream_read_errors
                .fetch_add(1, Ordering::Relaxed);
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
            if let Some(removed) = self.table.remove(&oldest_ip.unwrap()) {
                self.total_size -= removed.len();
                num_pruned += removed.len();
            }
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
            let old_size = e_ref.len();
            e_ref.retain(|connection| connection.port != addr.port());
            let new_size = e_ref.len();
            if e_ref.is_empty() {
                e.remove_entry();
            }
            self.total_size = self
                .total_size
                .saturating_sub(old_size.saturating_sub(new_size));
        }
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::quic::{MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
        crossbeam_channel::{unbounded, Receiver},
        quinn::{ClientConfig, IdleTimeout, VarInt},
        solana_sdk::{
            quic::{QUIC_KEEP_ALIVE_MS, QUIC_MAX_TIMEOUT_MS},
            signature::Keypair,
        },
        tokio::time::sleep,
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

    pub fn get_client_config() -> ClientConfig {
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

    fn setup_quic_server() -> (
        JoinHandle<()>,
        Arc<AtomicBool>,
        crossbeam_channel::Receiver<PacketBatch>,
        SocketAddr,
        Arc<StreamStats>,
    ) {
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let stats = Arc::new(StreamStats::default());
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
            stats.clone(),
        )
        .unwrap();
        (t, exit, receiver, server_address, stats)
    }

    pub async fn make_client_endpoint(addr: &SocketAddr) -> NewConnection {
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, client_socket)
            .unwrap()
            .0;
        endpoint.set_default_client_config(get_client_config());
        endpoint.connect(*addr, "localhost").unwrap().await.unwrap()
    }

    pub async fn check_timeout(receiver: Receiver<PacketBatch>, server_address: SocketAddr) {
        let conn1 = make_client_endpoint(&server_address).await;
        let total = 30;
        for i in 0..total {
            let mut s1 = conn1.connection.open_uni().await.unwrap();
            s1.write_all(&[0u8]).await.unwrap();
            s1.finish().await.unwrap();
            info!("done {}", i);
            sleep(Duration::from_millis(1000)).await;
        }
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
    }

    pub async fn check_block_multiple_connections(server_address: SocketAddr) {
        let conn1 = make_client_endpoint(&server_address).await;
        let conn2 = make_client_endpoint(&server_address).await;
        let mut s1 = conn1.connection.open_uni().await.unwrap();
        let mut s2 = conn2.connection.open_uni().await.unwrap();
        s1.write_all(&[0u8]).await.unwrap();
        s1.finish().await.unwrap();
        // Send enough data to create more than 1 chunks.
        // The first will try to open the connection (which should fail).
        // The following chunks will enable the detection of connection failure.
        let data = vec![1u8; PACKET_DATA_SIZE * 2];
        s2.write_all(&data)
            .await
            .expect_err("shouldn't be able to open 2 connections");
        s2.finish()
            .await
            .expect_err("shouldn't be able to open 2 connections");
    }

    pub async fn check_multiple_streams(
        receiver: Receiver<PacketBatch>,
        server_address: SocketAddr,
    ) {
        let conn1 = Arc::new(make_client_endpoint(&server_address).await);
        let conn2 = Arc::new(make_client_endpoint(&server_address).await);
        let mut num_expected_packets = 0;
        for i in 0..10 {
            info!("sending: {}", i);
            let c1 = conn1.clone();
            let c2 = conn2.clone();
            let mut s1 = c1.connection.open_uni().await.unwrap();
            let mut s2 = c2.connection.open_uni().await.unwrap();
            s1.write_all(&[0u8]).await.unwrap();
            s1.finish().await.unwrap();
            s2.write_all(&[0u8]).await.unwrap();
            s2.finish().await.unwrap();
            num_expected_packets += 2;
            sleep(Duration::from_millis(200)).await;
        }
        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 10 {
            if let Ok(packets) = receiver.recv_timeout(Duration::from_secs(1)) {
                total_packets += packets.len();
                all_packets.push(packets)
            }
            if total_packets == num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in batch.iter() {
                assert_eq!(p.meta.size, 1);
            }
        }
        assert_eq!(total_packets, num_expected_packets);
    }

    pub async fn check_multiple_writes(
        receiver: Receiver<PacketBatch>,
        server_address: SocketAddr,
    ) {
        let conn1 = Arc::new(make_client_endpoint(&server_address).await);

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets = 1;
        let mut s1 = conn1.connection.open_uni().await.unwrap();
        for _ in 0..num_bytes {
            s1.write_all(&[0u8]).await.unwrap();
        }
        s1.finish().await.unwrap();

        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 5 {
            if let Ok(packets) = receiver.recv_timeout(Duration::from_secs(1)) {
                total_packets += packets.len();
                all_packets.push(packets)
            }
            if total_packets > num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in batch.iter() {
                assert_eq!(p.meta.size, num_bytes);
            }
        }
        assert_eq!(total_packets, num_expected_packets);
    }

    pub async fn check_unstaked_node_connect_failure(server_address: SocketAddr) {
        let conn1 = Arc::new(make_client_endpoint(&server_address).await);

        // Send a full size packet with single byte writes.
        if let Ok(mut s1) = conn1.connection.open_uni().await {
            for _ in 0..PACKET_DATA_SIZE {
                // Ignoring any errors here. s1.finish() will test the error condition
                s1.write_all(&[0u8]).await.unwrap_or_default();
            }
            s1.finish().await.unwrap_err();
        }
    }

    #[tokio::test]
    async fn test_quic_server_exit() {
        let (t, exit, _receiver, _server_address, _stats) = setup_quic_server();
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_timeout() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server();
        check_timeout(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_stream_timeout() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address, stats) = setup_quic_server();

        let conn1 = make_client_endpoint(&server_address).await;
        assert_eq!(stats.total_streams.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_stream_read_timeouts.load(Ordering::Relaxed), 0);

        // Send one byte to start the stream
        let mut s1 = conn1.connection.open_uni().await.unwrap();
        s1.write_all(&[0u8]).await.unwrap_or_default();

        // Wait long enough for the stream to timeout in receiving chunks
        let sleep_time = (WAIT_FOR_STREAM_TIMEOUT_MS * 1000).min(2000);
        sleep(Duration::from_millis(sleep_time)).await;

        // Test that the stream was created, but timed out in read
        assert_eq!(stats.total_streams.load(Ordering::Relaxed), 1);
        assert_ne!(stats.total_stream_read_timeouts.load(Ordering::Relaxed), 0);

        // Test that more writes are still successful to the stream (i.e. the stream was writable
        // even after the timeouts)
        for _ in 0..PACKET_DATA_SIZE {
            s1.write_all(&[0u8]).await.unwrap();
        }
        s1.finish().await.unwrap();

        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_block_multiple_connections() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address, _stats) = setup_quic_server();
        check_block_multiple_connections(server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_multiple_writes() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server();
        check_multiple_writes(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_unstaked_node_connect_failure() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, _) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let stats = Arc::new(StreamStats::default());
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            0, // Do not allow any connection from unstaked clients/nodes
            stats,
        )
        .unwrap();

        check_unstaked_node_connect_failure(server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_multiple_streams() {
        solana_logger::setup();
        let s = UdpSocket::bind("127.0.0.1:0").unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = unbounded();
        let keypair = Keypair::new();
        let ip = "127.0.0.1".parse().unwrap();
        let server_address = s.local_addr().unwrap();
        let staked_nodes = Arc::new(RwLock::new(StakedNodes::default()));
        let stats = Arc::new(StreamStats::default());
        let t = spawn_server(
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            2,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            stats,
        )
        .unwrap();

        check_multiple_streams(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[test]
    fn test_prune_table() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::default();
        let mut num_entries = 5;
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
        num_entries += 1;
        table
            .try_add_connection(&sockets[0], 5, max_connections_per_ip)
            .unwrap();

        let new_size = 3;
        let pruned = table.prune_oldest(new_size);
        assert_eq!(pruned, num_entries as usize - new_size);
        for v in table.table.values() {
            for x in v {
                assert!((x.last_update() + 1) >= (num_entries as u64 - new_size as u64));
            }
        }
        assert_eq!(table.table.len(), new_size);
        assert_eq!(table.total_size, new_size);
        for socket in sockets.iter().take(num_entries as usize).skip(new_size - 1) {
            table.remove_connection(socket);
        }
        assert_eq!(table.total_size, 0);
    }

    #[test]
    fn test_remove_connections() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::default();
        let num_ips = 5;
        let max_connections_per_ip = 10;
        let mut sockets: Vec<_> = (0..num_ips)
            .into_iter()
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(socket, (i * 2) as u64, max_connections_per_ip)
                .unwrap();

            table
                .try_add_connection(socket, (i * 2 + 1) as u64, max_connections_per_ip)
                .unwrap();
        }

        let single_connection_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(num_ips, 0, 0, 0)), 0);
        table
            .try_add_connection(
                &single_connection_addr,
                (num_ips * 2) as u64,
                max_connections_per_ip,
            )
            .unwrap();

        let zero_connection_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(num_ips + 1, 0, 0, 0)), 0);

        sockets.push(single_connection_addr);
        sockets.push(zero_connection_addr);

        for socket in sockets.iter() {
            table.remove_connection(socket);
        }
        assert_eq!(table.total_size, 0);
    }
}
