use {
    crate::{
        quic::{configure_server, QuicServerError, StreamStats},
        streamer::StakedNodes,
    },
    crossbeam_channel::Sender,
    futures_util::stream::StreamExt,
    indexmap::map::{Entry, IndexMap},
    percentage::Percentage,
    quinn::{
        Connecting, Connection, Endpoint, EndpointConfig, Incoming, IncomingUniStreams,
        NewConnection, VarInt,
    },
    rand::{thread_rng, Rng},
    solana_perf::packet::PacketBatch,
    solana_sdk::{
        packet::{Packet, PACKET_DATA_SIZE},
        quic::QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS,
        signature::Keypair,
        timing,
    },
    std::{
        net::{IpAddr, SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex, MutexGuard, RwLock,
        },
        time::{Duration, Instant},
    },
    tokio::{
        task::JoinHandle,
        time::{sleep, timeout},
    },
};

const QUIC_TOTAL_STAKED_CONCURRENT_STREAMS: f64 = 100_000f64;
const WAIT_FOR_STREAM_TIMEOUT_MS: u64 = 100;

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
    let unstaked_connection_table: Arc<Mutex<ConnectionTable>> = Arc::new(Mutex::new(
        ConnectionTable::new(ConnectionPeerType::Unstaked),
    ));
    let staked_connection_table: Arc<Mutex<ConnectionTable>> =
        Arc::new(Mutex::new(ConnectionTable::new(ConnectionPeerType::Staked)));
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
                unstaked_connection_table.clone(),
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

fn prune_unstaked_connection_table(
    unstaked_connection_table: &mut MutexGuard<ConnectionTable>,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
) {
    if unstaked_connection_table.total_size >= max_unstaked_connections {
        const PRUNE_TABLE_TO_PERCENTAGE: u8 = 90;
        let max_percentage_full = Percentage::from(PRUNE_TABLE_TO_PERCENTAGE);

        let max_connections = max_percentage_full.apply_to(max_unstaked_connections);
        let num_pruned = unstaked_connection_table.prune_oldest(max_connections);
        stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
    }
}

async fn setup_connection(
    connection: Connecting,
    unstaked_connection_table: Arc<Mutex<ConnectionTable>>,
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

        let table_and_stake = {
            let staked_nodes = staked_nodes.read().unwrap();
            if let Some(stake) = staked_nodes.stake_map.get(&remote_addr.ip()) {
                let stake = *stake;
                drop(staked_nodes);

                let mut connection_table_l = staked_connection_table.lock().unwrap();
                if connection_table_l.total_size >= max_staked_connections {
                    let num_pruned = connection_table_l.prune_random(stake);
                    if num_pruned == 0 {
                        if max_unstaked_connections > 0 {
                            // If we couldn't prune a connection in the staked connection table, let's
                            // put this connection in the unstaked connection table. If needed, prune a
                            // connection from the unstaked connection table.
                            connection_table_l = unstaked_connection_table.lock().unwrap();
                            prune_unstaked_connection_table(
                                &mut connection_table_l,
                                max_unstaked_connections,
                                stats.clone(),
                            );
                            Some((connection_table_l, stake))
                        } else {
                            stats
                                .connection_add_failed_on_pruning
                                .fetch_add(1, Ordering::Relaxed);
                            None
                        }
                    } else {
                        stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                        Some((connection_table_l, stake))
                    }
                } else {
                    Some((connection_table_l, stake))
                }
            } else if max_unstaked_connections > 0 {
                drop(staked_nodes);
                let mut connection_table_l = unstaked_connection_table.lock().unwrap();
                prune_unstaked_connection_table(
                    &mut connection_table_l,
                    max_unstaked_connections,
                    stats.clone(),
                );
                Some((connection_table_l, 0))
            } else {
                None
            }
        };

        if let Some((mut connection_table_l, stake)) = table_and_stake {
            let table_type = connection_table_l.peer_type;
            let max_uni_streams = match table_type {
                ConnectionPeerType::Unstaked => {
                    VarInt::from_u64(QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS as u64)
                }
                ConnectionPeerType::Staked => {
                    let staked_nodes = staked_nodes.read().unwrap();
                    VarInt::from_u64(
                        ((stake as f64 / staked_nodes.total_stake as f64)
                            * QUIC_TOTAL_STAKED_CONCURRENT_STREAMS) as u64,
                    )
                }
            };

            if let Ok(max_uni_streams) = max_uni_streams {
                connection.set_max_concurrent_uni_streams(max_uni_streams);

                if let Some((last_update, stream_exit)) = connection_table_l.try_add_connection(
                    &remote_addr,
                    Some(connection),
                    stake,
                    timing::timestamp(),
                    max_connections_per_ip,
                ) {
                    drop(connection_table_l);
                    let stats = stats.clone();
                    let connection_table = match table_type {
                        ConnectionPeerType::Unstaked => unstaked_connection_table.clone(),
                        ConnectionPeerType::Staked => staked_connection_table.clone(),
                    };
                    tokio::spawn(handle_connection(
                        uni_streams,
                        packet_sender,
                        remote_addr,
                        last_update,
                        connection_table,
                        stream_exit,
                        stats,
                        stake,
                    ));
                } else {
                    stats.connection_add_failed.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                stats
                    .connection_add_failed_invalid_stream_count
                    .fetch_add(1, Ordering::Relaxed);
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
                        let stream_exit = stream_exit.clone();
                        let stats = stats.clone();
                        let packet_sender = packet_sender.clone();
                        let last_update = last_update.clone();
                        tokio::spawn(async move {
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
                            stats.total_streams.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) => {
                        debug!("stream error: {:?}", e);
                        break;
                    }
                },
                None => {
                    break;
                }
            }
        }
    }
    if connection_table
        .lock()
        .unwrap()
        .remove_connection(&remote_addr)
    {
        stats.connection_removed.fetch_add(1, Ordering::Relaxed);
    } else {
        stats
            .connection_remove_failed
            .fetch_add(1, Ordering::Relaxed);
    }
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
    stake: u64,
    last_update: Arc<AtomicU64>,
    port: u16,
    connection: Option<Connection>,
}

impl ConnectionEntry {
    fn new(
        exit: Arc<AtomicBool>,
        stake: u64,
        last_update: Arc<AtomicU64>,
        port: u16,
        connection: Option<Connection>,
    ) -> Self {
        Self {
            exit,
            stake,
            last_update,
            port,
            connection,
        }
    }

    fn last_update(&self) -> u64 {
        self.last_update.load(Ordering::Relaxed)
    }
}

impl Drop for ConnectionEntry {
    fn drop(&mut self) {
        if let Some(conn) = self.connection.take() {
            conn.close(0u32.into(), &[0u8]);
        }
        self.exit.store(true, Ordering::Relaxed);
    }
}

#[derive(Copy, Clone)]
enum ConnectionPeerType {
    Unstaked,
    Staked,
}

// Map of IP to list of connection entries
struct ConnectionTable {
    table: IndexMap<IpAddr, Vec<ConnectionEntry>>,
    total_size: usize,
    peer_type: ConnectionPeerType,
}

// Prune the connection which has the oldest update
// Return number pruned
impl ConnectionTable {
    fn new(peer_type: ConnectionPeerType) -> Self {
        Self {
            table: IndexMap::default(),
            total_size: 0,
            peer_type,
        }
    }

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
            if let Some(oldest_ip) = oldest_ip {
                if let Some(removed) = self.table.remove(&oldest_ip) {
                    self.total_size -= removed.len();
                    num_pruned += removed.len();
                }
            } else {
                // No valid entries in the table with an IP address. Continuing the loop will cause
                // infinite looping.
                break;
            }
        }
        num_pruned
    }

    fn connection_stake(&self, index: usize) -> Option<u64> {
        self.table
            .get_index(index)
            .and_then(|(_, connection_vec)| connection_vec.first())
            .map(|connection| connection.stake)
    }

    // Randomly select two connections, and evict the one with lower stake. If the stakes of both
    // the connections are higher than the threshold_stake, reject the pruning attempt, and return 0.
    fn prune_random(&mut self, threshold_stake: u64) -> usize {
        let mut num_pruned = 0;
        let mut rng = thread_rng();
        // The candidate1 and candidate2 could potentially be the same. If so, the stake of the candidate
        // will be compared just against the threshold_stake.
        let candidate1 = rng.gen_range(0, self.table.len());
        let candidate2 = rng.gen_range(0, self.table.len());

        let candidate1_stake = self.connection_stake(candidate1).unwrap_or(0);
        let candidate2_stake = self.connection_stake(candidate2).unwrap_or(0);

        if candidate1_stake < threshold_stake || candidate2_stake < threshold_stake {
            let removed = if candidate1_stake < candidate2_stake {
                self.table.swap_remove_index(candidate1)
            } else {
                self.table.swap_remove_index(candidate2)
            };

            if let Some((_, removed_value)) = removed {
                self.total_size -= removed_value.len();
                num_pruned += removed_value.len();
            }
        }

        num_pruned
    }

    fn try_add_connection(
        &mut self,
        addr: &SocketAddr,
        connection: Option<Connection>,
        stake: u64,
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
                stake,
                last_update.clone(),
                addr.port(),
                connection,
            ));
            self.total_size += 1;
            Some((last_update, exit))
        } else {
            None
        }
    }

    fn remove_connection(&mut self, addr: &SocketAddr) -> bool {
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
            true
        } else {
            false
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
        std::net::Ipv4Addr,
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

    fn setup_quic_server(
        option_staked_nodes: Option<StakedNodes>,
    ) -> (
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
        let staked_nodes = Arc::new(RwLock::new(option_staked_nodes.unwrap_or_default()));
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
        let (t, exit, _receiver, _server_address, _stats) = setup_quic_server(None);
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_timeout() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server(None);
        check_timeout(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_stream_timeout() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address, stats) = setup_quic_server(None);

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
        let (t, exit, _receiver, server_address, _stats) = setup_quic_server(None);
        check_block_multiple_connections(server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_multiple_writes() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server(None);
        check_multiple_writes(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_staked_connection_removal() {
        solana_logger::setup();

        let mut staked_nodes = StakedNodes::default();
        staked_nodes
            .stake_map
            .insert(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 100000);
        staked_nodes.total_stake = 100000;

        let (t, exit, receiver, server_address, stats) = setup_quic_server(Some(staked_nodes));
        check_multiple_writes(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
        assert_eq!(stats.connection_removed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.connection_remove_failed.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_quic_server_unstaked_connection_removal() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, stats) = setup_quic_server(None);
        check_multiple_writes(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
        assert_eq!(stats.connection_removed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.connection_remove_failed.load(Ordering::Relaxed), 0);
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
            stats.clone(),
        )
        .unwrap();

        check_multiple_streams(receiver, server_address).await;
        assert_eq!(stats.total_streams.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_new_streams.load(Ordering::Relaxed), 20);
        assert_eq!(stats.total_connections.load(Ordering::Relaxed), 2);
        assert_eq!(stats.total_new_connections.load(Ordering::Relaxed), 2);
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
        assert_eq!(stats.total_connections.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_new_connections.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_prune_table() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);
        let mut num_entries = 5;
        let max_connections_per_ip = 10;
        let sockets: Vec<_> = (0..num_entries)
            .into_iter()
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(socket, None, 0, i as u64, max_connections_per_ip)
                .unwrap();
        }
        num_entries += 1;
        table
            .try_add_connection(&sockets[0], None, 0, 5, max_connections_per_ip)
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
    fn test_prune_table_random() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);
        let num_entries = 5;
        let max_connections_per_ip = 10;
        let sockets: Vec<_> = (0..num_entries)
            .into_iter()
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(
                    socket,
                    None,
                    (i + 1) as u64,
                    i as u64,
                    max_connections_per_ip,
                )
                .unwrap();
        }

        // Try pruninng with threshold stake less than all the entries in the table
        // It should fail to prune (i.e. return 0 number of pruned entries)
        let pruned = table.prune_random(0);
        assert_eq!(pruned, 0);

        // Try pruninng with threshold stake higher than all the entries in the table
        // It should succeed to prune (i.e. return 1 number of pruned entries)
        let pruned = table.prune_random(num_entries as u64 + 1);
        assert_eq!(pruned, 1);
    }

    #[test]
    fn test_remove_connections() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);
        let num_ips = 5;
        let max_connections_per_ip = 10;
        let mut sockets: Vec<_> = (0..num_ips)
            .into_iter()
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(socket, None, 0, (i * 2) as u64, max_connections_per_ip)
                .unwrap();

            table
                .try_add_connection(socket, None, 0, (i * 2 + 1) as u64, max_connections_per_ip)
                .unwrap();
        }

        let single_connection_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(num_ips, 0, 0, 0)), 0);
        table
            .try_add_connection(
                &single_connection_addr,
                None,
                0,
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
