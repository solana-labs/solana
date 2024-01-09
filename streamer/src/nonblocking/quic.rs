use {
    crate::{
        quic::{configure_server, QuicServerError, StreamStats, MAX_UNSTAKED_CONNECTIONS},
        streamer::StakedNodes,
        tls_certificates::get_pubkey_from_tls_certificate,
    },
    async_channel::{
        unbounded as async_unbounded, Receiver as AsyncReceiver, Sender as AsyncSender,
    },
    bytes::Bytes,
    crossbeam_channel::Sender,
    indexmap::map::{Entry, IndexMap},
    percentage::Percentage,
    quinn::{Connecting, Connection, Endpoint, EndpointConfig, TokioRuntime, VarInt},
    quinn_proto::VarIntBoundsExceeded,
    rand::{thread_rng, Rng},
    solana_perf::packet::{PacketBatch, PACKETS_PER_BATCH},
    solana_sdk::{
        packet::{Meta, PACKET_DATA_SIZE},
        pubkey::Pubkey,
        quic::{
            QUIC_CONNECTION_HANDSHAKE_TIMEOUT, QUIC_MAX_STAKED_CONCURRENT_STREAMS,
            QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO, QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS,
            QUIC_MIN_STAKED_CONCURRENT_STREAMS, QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO,
            QUIC_TOTAL_STAKED_CONCURRENT_STREAMS, QUIC_UNSTAKED_RECEIVE_WINDOW_RATIO,
        },
        signature::Keypair,
        timing,
    },
    std::{
        iter::repeat_with,
        net::{IpAddr, SocketAddr, UdpSocket},
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            Arc, Mutex, MutexGuard, RwLock,
        },
        time::{Duration, Instant},
    },
    tokio::{task::JoinHandle, time::timeout},
};

/// Limit to 500K PPS
const MAX_STREAMS_PER_100MS: u64 = 500_000 / 10;
const MAX_UNSTAKED_STREAMS_PERCENT: u64 = 20;
const STREAM_THROTTLING_INTERVAL: Duration = Duration::from_millis(100);
const WAIT_FOR_STREAM_TIMEOUT: Duration = Duration::from_millis(100);
pub const DEFAULT_WAIT_FOR_CHUNK_TIMEOUT: Duration = Duration::from_secs(10);

pub const ALPN_TPU_PROTOCOL_ID: &[u8] = b"solana-tpu";

const CONNECTION_CLOSE_CODE_DROPPED_ENTRY: u32 = 1;
const CONNECTION_CLOSE_REASON_DROPPED_ENTRY: &[u8] = b"dropped";

const CONNECTION_CLOSE_CODE_DISALLOWED: u32 = 2;
const CONNECTION_CLOSE_REASON_DISALLOWED: &[u8] = b"disallowed";

const CONNECTION_CLOSE_CODE_EXCEED_MAX_STREAM_COUNT: u32 = 3;
const CONNECTION_CLOSE_REASON_EXCEED_MAX_STREAM_COUNT: &[u8] = b"exceed_max_stream_count";

const CONNECTION_CLOSE_CODE_TOO_MANY: u32 = 4;
const CONNECTION_CLOSE_REASON_TOO_MANY: &[u8] = b"too_many";
const STREAM_STOP_CODE_THROTTLING: u32 = 15;

// A sequence of bytes that is part of a packet
// along with where in the packet it is
struct PacketChunk {
    pub bytes: Bytes,
    // The offset of these bytes in the Quic stream
    // and thus the beginning offset in the slice of the
    // Packet data array into which the bytes will be copied
    pub offset: usize,
    // The end offset of these bytes in the Quic stream
    // and thus the end of the slice in the Packet data array
    // into which the bytes will be copied
    pub end_of_chunk: usize,
}

// A struct to accumulate the bytes making up
// a packet, along with their offsets, and the
// packet metadata. We use this accumulator to avoid
// multiple copies of the Bytes (when building up
// the Packet and then when copying the Packet into a PacketBatch)
struct PacketAccumulator {
    pub meta: Meta,
    pub chunks: Vec<PacketChunk>,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_server(
    name: &'static str,
    sock: UdpSocket,
    keypair: &Keypair,
    gossip_host: IpAddr,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_peer: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    wait_for_chunk_timeout: Duration,
    coalesce: Duration,
) -> Result<(Endpoint, Arc<StreamStats>, JoinHandle<()>), QuicServerError> {
    info!("Start {name} quic server on {sock:?}");
    let (config, _cert) = configure_server(keypair, gossip_host)?;

    let endpoint = Endpoint::new(
        EndpointConfig::default(),
        Some(config),
        sock,
        Arc::new(TokioRuntime),
    )
    .map_err(QuicServerError::EndpointFailed)?;
    let stats = Arc::<StreamStats>::default();
    let handle = tokio::spawn(run_server(
        name,
        endpoint.clone(),
        packet_sender,
        exit,
        max_connections_per_peer,
        staked_nodes,
        max_staked_connections,
        max_unstaked_connections,
        stats.clone(),
        wait_for_chunk_timeout,
        coalesce,
    ));
    Ok((endpoint, stats, handle))
}

#[allow(clippy::too_many_arguments)]
async fn run_server(
    name: &'static str,
    incoming: Endpoint,
    packet_sender: Sender<PacketBatch>,
    exit: Arc<AtomicBool>,
    max_connections_per_peer: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
    wait_for_chunk_timeout: Duration,
    coalesce: Duration,
) {
    const WAIT_FOR_CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);
    debug!("spawn quic server");
    let mut last_datapoint = Instant::now();
    let unstaked_connection_table: Arc<Mutex<ConnectionTable>> = Arc::new(Mutex::new(
        ConnectionTable::new(ConnectionPeerType::Unstaked),
    ));
    let staked_connection_table: Arc<Mutex<ConnectionTable>> =
        Arc::new(Mutex::new(ConnectionTable::new(ConnectionPeerType::Staked)));
    let (sender, receiver) = async_unbounded();
    tokio::spawn(packet_batch_sender(
        packet_sender,
        receiver,
        exit.clone(),
        stats.clone(),
        coalesce,
    ));
    while !exit.load(Ordering::Relaxed) {
        let timeout_connection = timeout(WAIT_FOR_CONNECTION_TIMEOUT, incoming.accept()).await;

        if last_datapoint.elapsed().as_secs() >= 5 {
            stats.report(name);
            last_datapoint = Instant::now();
        }

        if let Ok(Some(connection)) = timeout_connection {
            info!("Got a connection {:?}", connection.remote_address());
            tokio::spawn(setup_connection(
                connection,
                unstaked_connection_table.clone(),
                staked_connection_table.clone(),
                sender.clone(),
                max_connections_per_peer,
                staked_nodes.clone(),
                max_staked_connections,
                max_unstaked_connections,
                stats.clone(),
                wait_for_chunk_timeout,
            ));
        } else {
            debug!("accept(): Timed out waiting for connection");
        }
    }
}

fn prune_unstaked_connection_table(
    unstaked_connection_table: &mut ConnectionTable,
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

pub fn get_remote_pubkey(connection: &Connection) -> Option<Pubkey> {
    // Use the client cert only if it is self signed and the chain length is 1.
    connection
        .peer_identity()?
        .downcast::<Vec<rustls::Certificate>>()
        .ok()
        .filter(|certs| certs.len() == 1)?
        .first()
        .and_then(get_pubkey_from_tls_certificate)
}

fn get_connection_stake(
    connection: &Connection,
    staked_nodes: &RwLock<StakedNodes>,
) -> Option<(Pubkey, u64, u64, u64, u64)> {
    let pubkey = get_remote_pubkey(connection)?;
    debug!("Peer public key is {pubkey:?}");
    let staked_nodes = staked_nodes.read().unwrap();
    Some((
        pubkey,
        staked_nodes.get_node_stake(&pubkey)?,
        staked_nodes.total_stake(),
        staked_nodes.max_stake(),
        staked_nodes.min_stake(),
    ))
}

pub fn compute_max_allowed_uni_streams(
    peer_type: ConnectionPeerType,
    peer_stake: u64,
    total_stake: u64,
) -> usize {
    // Treat stake = 0 as unstaked
    if peer_stake == 0 {
        QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
    } else {
        match peer_type {
            ConnectionPeerType::Staked => {
                // No checked math for f64 type. So let's explicitly check for 0 here
                if total_stake == 0 || peer_stake > total_stake {
                    warn!(
                        "Invalid stake values: peer_stake: {:?}, total_stake: {:?}",
                        peer_stake, total_stake,
                    );

                    QUIC_MIN_STAKED_CONCURRENT_STREAMS
                } else {
                    let delta = (QUIC_TOTAL_STAKED_CONCURRENT_STREAMS
                        - QUIC_MIN_STAKED_CONCURRENT_STREAMS)
                        as f64;

                    (((peer_stake as f64 / total_stake as f64) * delta) as usize
                        + QUIC_MIN_STAKED_CONCURRENT_STREAMS)
                        .clamp(
                            QUIC_MIN_STAKED_CONCURRENT_STREAMS,
                            QUIC_MAX_STAKED_CONCURRENT_STREAMS,
                        )
                }
            }
            _ => QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS,
        }
    }
}

enum ConnectionHandlerError {
    ConnectionAddError,
    MaxStreamError,
}

#[derive(Clone)]
struct NewConnectionHandlerParams {
    // In principle, the code can be made to work with a crossbeam channel
    // as long as we're careful never to use a blocking recv or send call
    // but I've found that it's simply too easy to accidentally block
    // in async code when using the crossbeam channel, so for the sake of maintainability,
    // we're sticking with an async channel
    packet_sender: AsyncSender<PacketAccumulator>,
    remote_pubkey: Option<Pubkey>,
    stake: u64,
    total_stake: u64,
    max_connections_per_peer: usize,
    stats: Arc<StreamStats>,
    max_stake: u64,
    min_stake: u64,
}

impl NewConnectionHandlerParams {
    fn new_unstaked(
        packet_sender: AsyncSender<PacketAccumulator>,
        max_connections_per_peer: usize,
        stats: Arc<StreamStats>,
    ) -> NewConnectionHandlerParams {
        NewConnectionHandlerParams {
            packet_sender,
            remote_pubkey: None,
            stake: 0,
            total_stake: 0,
            max_connections_per_peer,
            stats,
            max_stake: 0,
            min_stake: 0,
        }
    }
}

fn handle_and_cache_new_connection(
    connection: Connection,
    mut connection_table_l: MutexGuard<ConnectionTable>,
    connection_table: Arc<Mutex<ConnectionTable>>,
    params: &NewConnectionHandlerParams,
    wait_for_chunk_timeout: Duration,
) -> Result<(), ConnectionHandlerError> {
    if let Ok(max_uni_streams) = VarInt::from_u64(compute_max_allowed_uni_streams(
        connection_table_l.peer_type,
        params.stake,
        params.total_stake,
    ) as u64)
    {
        connection.set_max_concurrent_uni_streams(max_uni_streams);
        let receive_window = compute_recieve_window(
            params.max_stake,
            params.min_stake,
            connection_table_l.peer_type,
            params.stake,
        );

        if let Ok(receive_window) = receive_window {
            connection.set_receive_window(receive_window);
        }

        let remote_addr = connection.remote_address();

        debug!(
            "Peer type: {:?}, stake {}, total stake {}, max streams {} receive_window {:?} from peer {}",
            connection_table_l.peer_type,
            params.stake,
            params.total_stake,
            max_uni_streams.into_inner(),
            receive_window,
            remote_addr,
        );

        if let Some((last_update, stream_exit)) = connection_table_l.try_add_connection(
            ConnectionTableKey::new(remote_addr.ip(), params.remote_pubkey),
            remote_addr.port(),
            Some(connection.clone()),
            params.stake,
            timing::timestamp(),
            params.max_connections_per_peer,
        ) {
            let peer_type = connection_table_l.peer_type;
            drop(connection_table_l);
            tokio::spawn(handle_connection(
                connection,
                remote_addr,
                last_update,
                connection_table,
                stream_exit,
                params.clone(),
                peer_type,
                wait_for_chunk_timeout,
            ));
            Ok(())
        } else {
            params
                .stats
                .connection_add_failed
                .fetch_add(1, Ordering::Relaxed);
            Err(ConnectionHandlerError::ConnectionAddError)
        }
    } else {
        connection.close(
            CONNECTION_CLOSE_CODE_EXCEED_MAX_STREAM_COUNT.into(),
            CONNECTION_CLOSE_REASON_EXCEED_MAX_STREAM_COUNT,
        );
        params
            .stats
            .connection_add_failed_invalid_stream_count
            .fetch_add(1, Ordering::Relaxed);
        Err(ConnectionHandlerError::MaxStreamError)
    }
}

fn prune_unstaked_connections_and_add_new_connection(
    connection: Connection,
    connection_table: Arc<Mutex<ConnectionTable>>,
    max_connections: usize,
    params: &NewConnectionHandlerParams,
    wait_for_chunk_timeout: Duration,
) -> Result<(), ConnectionHandlerError> {
    let stats = params.stats.clone();
    if max_connections > 0 {
        let connection_table_clone = connection_table.clone();
        let mut connection_table = connection_table.lock().unwrap();
        prune_unstaked_connection_table(&mut connection_table, max_connections, stats);
        handle_and_cache_new_connection(
            connection,
            connection_table,
            connection_table_clone,
            params,
            wait_for_chunk_timeout,
        )
    } else {
        connection.close(
            CONNECTION_CLOSE_CODE_DISALLOWED.into(),
            CONNECTION_CLOSE_REASON_DISALLOWED,
        );
        Err(ConnectionHandlerError::ConnectionAddError)
    }
}

/// Calculate the ratio for per connection receive window from a staked peer
fn compute_receive_window_ratio_for_staked_node(max_stake: u64, min_stake: u64, stake: u64) -> u64 {
    // Testing shows the maximum througput from a connection is achieved at receive_window =
    // PACKET_DATA_SIZE * 10. Beyond that, there is not much gain. We linearly map the
    // stake to the ratio range from QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO to
    // QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO. Where the linear algebra of finding the ratio 'r'
    // for stake 's' is,
    // r(s) = a * s + b. Given the max_stake, min_stake, max_ratio, min_ratio, we can find
    // a and b.

    if stake > max_stake {
        return QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO;
    }

    let max_ratio = QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO;
    let min_ratio = QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO;
    if max_stake > min_stake {
        let a = (max_ratio - min_ratio) as f64 / (max_stake - min_stake) as f64;
        let b = max_ratio as f64 - ((max_stake as f64) * a);
        let ratio = (a * stake as f64) + b;
        ratio.round() as u64
    } else {
        QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO
    }
}

fn compute_recieve_window(
    max_stake: u64,
    min_stake: u64,
    peer_type: ConnectionPeerType,
    peer_stake: u64,
) -> Result<VarInt, VarIntBoundsExceeded> {
    match peer_type {
        ConnectionPeerType::Unstaked => {
            VarInt::from_u64(PACKET_DATA_SIZE as u64 * QUIC_UNSTAKED_RECEIVE_WINDOW_RATIO)
        }
        ConnectionPeerType::Staked => {
            let ratio =
                compute_receive_window_ratio_for_staked_node(max_stake, min_stake, peer_stake);
            VarInt::from_u64(PACKET_DATA_SIZE as u64 * ratio)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn setup_connection(
    connecting: Connecting,
    unstaked_connection_table: Arc<Mutex<ConnectionTable>>,
    staked_connection_table: Arc<Mutex<ConnectionTable>>,
    packet_sender: AsyncSender<PacketAccumulator>,
    max_connections_per_peer: usize,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    max_staked_connections: usize,
    max_unstaked_connections: usize,
    stats: Arc<StreamStats>,
    wait_for_chunk_timeout: Duration,
) {
    const PRUNE_RANDOM_SAMPLE_SIZE: usize = 2;
    let from = connecting.remote_address();
    if let Ok(connecting_result) = timeout(QUIC_CONNECTION_HANDSHAKE_TIMEOUT, connecting).await {
        match connecting_result {
            Ok(new_connection) => {
                stats.total_new_connections.fetch_add(1, Ordering::Relaxed);

                let params = get_connection_stake(&new_connection, &staked_nodes).map_or(
                    NewConnectionHandlerParams::new_unstaked(
                        packet_sender.clone(),
                        max_connections_per_peer,
                        stats.clone(),
                    ),
                    |(pubkey, stake, total_stake, max_stake, min_stake)| {
                        NewConnectionHandlerParams {
                            packet_sender,
                            remote_pubkey: Some(pubkey),
                            stake,
                            total_stake,
                            max_connections_per_peer,
                            stats: stats.clone(),
                            max_stake,
                            min_stake,
                        }
                    },
                );

                if params.stake > 0 {
                    let mut connection_table_l = staked_connection_table.lock().unwrap();
                    if connection_table_l.total_size >= max_staked_connections {
                        let num_pruned =
                            connection_table_l.prune_random(PRUNE_RANDOM_SAMPLE_SIZE, params.stake);
                        stats.num_evictions.fetch_add(num_pruned, Ordering::Relaxed);
                    }

                    if connection_table_l.total_size < max_staked_connections {
                        if let Ok(()) = handle_and_cache_new_connection(
                            new_connection,
                            connection_table_l,
                            staked_connection_table.clone(),
                            &params,
                            wait_for_chunk_timeout,
                        ) {
                            stats
                                .connection_added_from_staked_peer
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        // If we couldn't prune a connection in the staked connection table, let's
                        // put this connection in the unstaked connection table. If needed, prune a
                        // connection from the unstaked connection table.
                        if let Ok(()) = prune_unstaked_connections_and_add_new_connection(
                            new_connection,
                            unstaked_connection_table.clone(),
                            max_unstaked_connections,
                            &params,
                            wait_for_chunk_timeout,
                        ) {
                            stats
                                .connection_added_from_staked_peer
                                .fetch_add(1, Ordering::Relaxed);
                        } else {
                            stats
                                .connection_add_failed_on_pruning
                                .fetch_add(1, Ordering::Relaxed);
                            stats
                                .connection_add_failed_staked_node
                                .fetch_add(1, Ordering::Relaxed);
                        }
                    }
                } else if let Ok(()) = prune_unstaked_connections_and_add_new_connection(
                    new_connection,
                    unstaked_connection_table.clone(),
                    max_unstaked_connections,
                    &params,
                    wait_for_chunk_timeout,
                ) {
                    stats
                        .connection_added_from_unstaked_peer
                        .fetch_add(1, Ordering::Relaxed);
                } else {
                    stats
                        .connection_add_failed_unstaked_node
                        .fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                handle_connection_error(e, &stats, from);
            }
        }
    } else {
        stats
            .connection_setup_timeout
            .fetch_add(1, Ordering::Relaxed);
    }
}

fn handle_connection_error(e: quinn::ConnectionError, stats: &StreamStats, from: SocketAddr) {
    debug!("error: {:?} from: {:?}", e, from);
    stats.connection_setup_error.fetch_add(1, Ordering::Relaxed);
    match e {
        quinn::ConnectionError::TimedOut => {
            stats
                .connection_setup_error_timed_out
                .fetch_add(1, Ordering::Relaxed);
        }
        quinn::ConnectionError::ConnectionClosed(_) => {
            stats
                .connection_setup_error_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        quinn::ConnectionError::TransportError(_) => {
            stats
                .connection_setup_error_transport
                .fetch_add(1, Ordering::Relaxed);
        }
        quinn::ConnectionError::ApplicationClosed(_) => {
            stats
                .connection_setup_error_app_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        quinn::ConnectionError::Reset => {
            stats
                .connection_setup_error_reset
                .fetch_add(1, Ordering::Relaxed);
        }
        quinn::ConnectionError::LocallyClosed => {
            stats
                .connection_setup_error_locally_closed
                .fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

async fn packet_batch_sender(
    packet_sender: Sender<PacketBatch>,
    packet_receiver: AsyncReceiver<PacketAccumulator>,
    exit: Arc<AtomicBool>,
    stats: Arc<StreamStats>,
    coalesce: Duration,
) {
    trace!("enter packet_batch_sender");
    let mut batch_start_time = Instant::now();
    loop {
        let mut packet_batch = PacketBatch::with_capacity(PACKETS_PER_BATCH);
        let mut total_bytes: usize = 0;

        stats
            .total_packet_batches_allocated
            .fetch_add(1, Ordering::Relaxed);
        stats
            .total_packets_allocated
            .fetch_add(PACKETS_PER_BATCH, Ordering::Relaxed);

        loop {
            if exit.load(Ordering::Relaxed) {
                return;
            }
            let elapsed = batch_start_time.elapsed();
            if packet_batch.len() >= PACKETS_PER_BATCH
                || (!packet_batch.is_empty() && elapsed >= coalesce)
            {
                let len = packet_batch.len();
                if let Err(e) = packet_sender.send(packet_batch) {
                    stats
                        .total_packet_batch_send_err
                        .fetch_add(1, Ordering::Relaxed);
                    trace!("Send error: {}", e);
                } else {
                    stats
                        .total_packet_batches_sent
                        .fetch_add(1, Ordering::Relaxed);

                    stats
                        .total_packets_sent_to_consumer
                        .fetch_add(len, Ordering::Relaxed);

                    stats
                        .total_bytes_sent_to_consumer
                        .fetch_add(total_bytes, Ordering::Relaxed);

                    trace!("Sent {} packet batch", len);
                }
                break;
            }

            let timeout_res = timeout(Duration::from_micros(250), packet_receiver.recv()).await;

            if let Ok(Ok(packet_accumulator)) = timeout_res {
                // Start the timeout from when the packet batch first becomes non-empty
                if packet_batch.is_empty() {
                    batch_start_time = Instant::now();
                }

                unsafe {
                    packet_batch.set_len(packet_batch.len() + 1);
                }

                let i = packet_batch.len() - 1;
                *packet_batch[i].meta_mut() = packet_accumulator.meta;
                let num_chunks = packet_accumulator.chunks.len();
                for chunk in packet_accumulator.chunks {
                    packet_batch[i].buffer_mut()[chunk.offset..chunk.end_of_chunk]
                        .copy_from_slice(&chunk.bytes);
                }

                total_bytes += packet_batch[i].meta().size;

                stats
                    .total_chunks_processed_by_batcher
                    .fetch_add(num_chunks, Ordering::Relaxed);
            }
        }
    }
}

fn max_streams_for_connection_in_100ms(
    connection_type: ConnectionPeerType,
    stake: u64,
    total_stake: u64,
) -> u64 {
    if matches!(connection_type, ConnectionPeerType::Unstaked) || stake == 0 {
        Percentage::from(MAX_UNSTAKED_STREAMS_PERCENT)
            .apply_to(MAX_STREAMS_PER_100MS)
            .saturating_div(MAX_UNSTAKED_CONNECTIONS as u64)
    } else {
        let max_total_staked_streams: u64 = MAX_STREAMS_PER_100MS
            - Percentage::from(MAX_UNSTAKED_STREAMS_PERCENT).apply_to(MAX_STREAMS_PER_100MS);
        ((max_total_staked_streams as f64 / total_stake as f64) * stake as f64) as u64
    }
}

fn reset_throttling_params_if_needed(last_instant: &mut tokio::time::Instant) -> bool {
    if tokio::time::Instant::now().duration_since(*last_instant) > STREAM_THROTTLING_INTERVAL {
        *last_instant = tokio::time::Instant::now();
        true
    } else {
        false
    }
}

async fn handle_connection(
    connection: Connection,
    remote_addr: SocketAddr,
    last_update: Arc<AtomicU64>,
    connection_table: Arc<Mutex<ConnectionTable>>,
    stream_exit: Arc<AtomicBool>,
    params: NewConnectionHandlerParams,
    peer_type: ConnectionPeerType,
    wait_for_chunk_timeout: Duration,
) {
    let stats = params.stats;
    debug!(
        "quic new connection {} streams: {} connections: {}",
        remote_addr,
        stats.total_streams.load(Ordering::Relaxed),
        stats.total_connections.load(Ordering::Relaxed),
    );
    let stable_id = connection.stable_id();
    stats.total_connections.fetch_add(1, Ordering::Relaxed);
    let max_streams_per_100ms =
        max_streams_for_connection_in_100ms(peer_type, params.stake, params.total_stake);
    let mut last_throttling_instant = tokio::time::Instant::now();
    let mut streams_in_current_interval = 0;
    while !stream_exit.load(Ordering::Relaxed) {
        if let Ok(stream) =
            tokio::time::timeout(WAIT_FOR_STREAM_TIMEOUT, connection.accept_uni()).await
        {
            match stream {
                Ok(mut stream) => {
                    if reset_throttling_params_if_needed(&mut last_throttling_instant) {
                        streams_in_current_interval = 0;
                    } else if streams_in_current_interval >= max_streams_per_100ms {
                        stats.throttled_streams.fetch_add(1, Ordering::Relaxed);
                        let _ = stream.stop(VarInt::from_u32(STREAM_STOP_CODE_THROTTLING));
                        continue;
                    }
                    streams_in_current_interval = streams_in_current_interval.saturating_add(1);
                    stats.total_streams.fetch_add(1, Ordering::Relaxed);
                    stats.total_new_streams.fetch_add(1, Ordering::Relaxed);
                    let stream_exit = stream_exit.clone();
                    let stats = stats.clone();
                    let packet_sender = params.packet_sender.clone();
                    let last_update = last_update.clone();
                    tokio::spawn(async move {
                        let mut maybe_batch = None;
                        // The min is to guard against a value too small which can wake up unnecessarily
                        // frequently and wasting CPU cycles. The max guard against waiting for too long
                        // which delay exit and cause some test failures when the timeout value is large.
                        // Within this value, the heuristic is to wake up 10 times to check for exit
                        // for the set timeout if there are no data.
                        let exit_check_interval = (wait_for_chunk_timeout / 10)
                            .clamp(Duration::from_millis(10), Duration::from_secs(1));
                        let mut start = Instant::now();
                        while !stream_exit.load(Ordering::Relaxed) {
                            if let Ok(chunk) = tokio::time::timeout(
                                exit_check_interval,
                                stream.read_chunk(PACKET_DATA_SIZE, false),
                            )
                            .await
                            {
                                if handle_chunk(
                                    chunk,
                                    &mut maybe_batch,
                                    &remote_addr,
                                    &packet_sender,
                                    stats.clone(),
                                    peer_type,
                                )
                                .await
                                {
                                    last_update.store(timing::timestamp(), Ordering::Relaxed);
                                    break;
                                }
                                start = Instant::now();
                            } else if start.elapsed() > wait_for_chunk_timeout {
                                debug!("Timeout in receiving on stream");
                                stats
                                    .total_stream_read_timeouts
                                    .fetch_add(1, Ordering::Relaxed);
                                break;
                            }
                        }
                        stats.total_streams.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(e) => {
                    debug!("stream error: {:?}", e);
                    break;
                }
            }
        }
    }

    let removed_connection_count = connection_table.lock().unwrap().remove_connection(
        ConnectionTableKey::new(remote_addr.ip(), params.remote_pubkey),
        remote_addr.port(),
        stable_id,
    );
    if removed_connection_count > 0 {
        stats
            .connection_removed
            .fetch_add(removed_connection_count, Ordering::Relaxed);
    } else {
        stats
            .connection_remove_failed
            .fetch_add(1, Ordering::Relaxed);
    }
    stats.total_connections.fetch_sub(1, Ordering::Relaxed);
}

// Return true if the server should drop the stream
async fn handle_chunk(
    chunk: Result<Option<quinn::Chunk>, quinn::ReadError>,
    packet_accum: &mut Option<PacketAccumulator>,
    remote_addr: &SocketAddr,
    packet_sender: &AsyncSender<PacketAccumulator>,
    stats: Arc<StreamStats>,
    peer_type: ConnectionPeerType,
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
                let Some(end_of_chunk) = chunk.offset.checked_add(chunk_len) else {
                    return true;
                };
                if end_of_chunk > PACKET_DATA_SIZE as u64 {
                    stats
                        .total_invalid_chunk_size
                        .fetch_add(1, Ordering::Relaxed);
                    return true;
                }

                // chunk looks valid
                if packet_accum.is_none() {
                    let mut meta = Meta::default();
                    meta.set_socket_addr(remote_addr);
                    *packet_accum = Some(PacketAccumulator {
                        meta,
                        chunks: Vec::new(),
                    });
                }

                if let Some(accum) = packet_accum.as_mut() {
                    let offset = chunk.offset;
                    let Some(end_of_chunk) = (chunk.offset as usize).checked_add(chunk.bytes.len())
                    else {
                        return true;
                    };
                    accum.chunks.push(PacketChunk {
                        bytes: chunk.bytes,
                        offset: offset as usize,
                        end_of_chunk,
                    });

                    accum.meta.size = std::cmp::max(accum.meta.size, end_of_chunk);
                }

                match peer_type {
                    ConnectionPeerType::Staked => {
                        stats
                            .total_staked_chunks_received
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    ConnectionPeerType::Unstaked => {
                        stats
                            .total_unstaked_chunks_received
                            .fetch_add(1, Ordering::Relaxed);
                    }
                }
            } else {
                // done receiving chunks
                trace!("chunk is none");
                if let Some(accum) = packet_accum.take() {
                    let bytes_sent = accum.meta.size;
                    let chunks_sent = accum.chunks.len();

                    if let Err(err) = packet_sender.send(accum).await {
                        stats
                            .total_handle_chunk_to_packet_batcher_send_err
                            .fetch_add(1, Ordering::Relaxed);
                        trace!("packet batch send error {:?}", err);
                    } else {
                        stats
                            .total_packets_sent_for_batching
                            .fetch_add(1, Ordering::Relaxed);
                        stats
                            .total_bytes_sent_for_batching
                            .fetch_add(bytes_sent, Ordering::Relaxed);
                        stats
                            .total_chunks_sent_for_batching
                            .fetch_add(chunks_sent, Ordering::Relaxed);

                        trace!("sent {} byte packet for batching", bytes_sent);
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
            conn.close(
                CONNECTION_CLOSE_CODE_DROPPED_ENTRY.into(),
                CONNECTION_CLOSE_REASON_DROPPED_ENTRY,
            );
        }
        self.exit.store(true, Ordering::Relaxed);
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ConnectionPeerType {
    Unstaked,
    Staked,
}

#[derive(Copy, Clone, Eq, Hash, PartialEq)]
enum ConnectionTableKey {
    IP(IpAddr),
    Pubkey(Pubkey),
}

impl ConnectionTableKey {
    fn new(ip: IpAddr, maybe_pubkey: Option<Pubkey>) -> Self {
        maybe_pubkey.map_or(ConnectionTableKey::IP(ip), |pubkey| {
            ConnectionTableKey::Pubkey(pubkey)
        })
    }
}

// Map of IP to list of connection entries
struct ConnectionTable {
    table: IndexMap<ConnectionTableKey, Vec<ConnectionEntry>>,
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
        let key = |(_, connections): &(_, &Vec<_>)| {
            connections.iter().map(ConnectionEntry::last_update).min()
        };
        while self.total_size.saturating_sub(num_pruned) > max_size {
            match self.table.values().enumerate().min_by_key(key) {
                None => break,
                Some((index, connections)) => {
                    num_pruned += connections.len();
                    self.table.swap_remove_index(index);
                }
            }
        }
        self.total_size = self.total_size.saturating_sub(num_pruned);
        num_pruned
    }

    // Randomly selects sample_size many connections, evicts the one with the
    // lowest stake, and returns the number of pruned connections.
    // If the stakes of all the sampled connections are higher than the
    // threshold_stake, rejects the pruning attempt, and returns 0.
    fn prune_random(&mut self, sample_size: usize, threshold_stake: u64) -> usize {
        let num_pruned = std::iter::once(self.table.len())
            .filter(|&size| size > 0)
            .flat_map(|size| {
                let mut rng = thread_rng();
                repeat_with(move || rng.gen_range(0..size))
            })
            .map(|index| {
                let connection = self.table[index].first();
                let stake = connection.map(|connection| connection.stake);
                (index, stake)
            })
            .take(sample_size)
            .min_by_key(|&(_, stake)| stake)
            .filter(|&(_, stake)| stake < Some(threshold_stake))
            .and_then(|(index, _)| self.table.swap_remove_index(index))
            .map(|(_, connections)| connections.len())
            .unwrap_or_default();
        self.total_size = self.total_size.saturating_sub(num_pruned);
        num_pruned
    }

    fn try_add_connection(
        &mut self,
        key: ConnectionTableKey,
        port: u16,
        connection: Option<Connection>,
        stake: u64,
        last_update: u64,
        max_connections_per_peer: usize,
    ) -> Option<(Arc<AtomicU64>, Arc<AtomicBool>)> {
        let connection_entry = self.table.entry(key).or_default();
        let has_connection_capacity = connection_entry
            .len()
            .checked_add(1)
            .map(|c| c <= max_connections_per_peer)
            .unwrap_or(false);
        if has_connection_capacity {
            let exit = Arc::new(AtomicBool::new(false));
            let last_update = Arc::new(AtomicU64::new(last_update));
            connection_entry.push(ConnectionEntry::new(
                exit.clone(),
                stake,
                last_update.clone(),
                port,
                connection,
            ));
            self.total_size += 1;
            Some((last_update, exit))
        } else {
            if let Some(connection) = connection {
                connection.close(
                    CONNECTION_CLOSE_CODE_TOO_MANY.into(),
                    CONNECTION_CLOSE_REASON_TOO_MANY,
                );
            }
            None
        }
    }

    // Returns number of connections that were removed
    fn remove_connection(&mut self, key: ConnectionTableKey, port: u16, stable_id: usize) -> usize {
        if let Entry::Occupied(mut e) = self.table.entry(key) {
            let e_ref = e.get_mut();
            let old_size = e_ref.len();

            e_ref.retain(|connection_entry| {
                // Retain the connection entry if the port is different, or if the connection's
                // stable_id doesn't match the provided stable_id.
                // (Some unit tests do not fill in a valid connection in the table. To support that,
                // if the connection is none, the stable_id check is ignored. i.e. if the port matches,
                // the connection gets removed)
                connection_entry.port != port
                    || connection_entry
                        .connection
                        .as_ref()
                        .and_then(|connection| (connection.stable_id() != stable_id).then_some(0))
                        .is_some()
            });
            let new_size = e_ref.len();
            if e_ref.is_empty() {
                e.remove_entry();
            }
            let connections_removed = old_size.saturating_sub(new_size);
            self.total_size = self.total_size.saturating_sub(connections_removed);
            connections_removed
        } else {
            0
        }
    }
}

#[cfg(test)]
pub mod test {
    use {
        super::*,
        crate::{
            nonblocking::quic::compute_max_allowed_uni_streams,
            quic::{MAX_STAKED_CONNECTIONS, MAX_UNSTAKED_CONNECTIONS},
            tls_certificates::new_self_signed_tls_certificate,
        },
        assert_matches::assert_matches,
        async_channel::unbounded as async_unbounded,
        crossbeam_channel::{unbounded, Receiver},
        quinn::{ClientConfig, IdleTimeout, TransportConfig},
        solana_sdk::{
            net::DEFAULT_TPU_COALESCE,
            quic::{QUIC_KEEP_ALIVE, QUIC_MAX_TIMEOUT},
            signature::Keypair,
            signer::Signer,
        },
        std::{collections::HashMap, net::Ipv4Addr},
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

    pub fn get_client_config(keypair: &Keypair) -> ClientConfig {
        let ipaddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let (cert, key) = new_self_signed_tls_certificate(keypair, ipaddr)
            .expect("Failed to generate client certificate");

        let mut crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(SkipServerVerification::new())
            .with_client_auth_cert(vec![cert], key)
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

    fn setup_quic_server(
        option_staked_nodes: Option<StakedNodes>,
        max_connections_per_peer: usize,
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
        let (_, stats, t) = spawn_server(
            "quic_streamer_test",
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            max_connections_per_peer,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            Duration::from_secs(2),
            DEFAULT_TPU_COALESCE,
        )
        .unwrap();
        (t, exit, receiver, server_address, stats)
    }

    pub async fn make_client_endpoint(
        addr: &SocketAddr,
        client_keypair: Option<&Keypair>,
    ) -> Connection {
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut endpoint = quinn::Endpoint::new(
            EndpointConfig::default(),
            None,
            client_socket,
            Arc::new(TokioRuntime),
        )
        .unwrap();
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

    pub async fn check_timeout(receiver: Receiver<PacketBatch>, server_address: SocketAddr) {
        let conn1 = make_client_endpoint(&server_address, None).await;
        let total = 30;
        for i in 0..total {
            let mut s1 = conn1.open_uni().await.unwrap();
            s1.write_all(&[0u8]).await.unwrap();
            s1.finish().await.unwrap();
            info!("done {}", i);
            sleep(Duration::from_millis(1000)).await;
        }
        let mut received = 0;
        loop {
            if let Ok(_x) = receiver.try_recv() {
                received += 1;
                info!("got {}", received);
            } else {
                sleep(Duration::from_millis(500)).await;
            }
            if received >= total {
                break;
            }
        }
    }

    pub async fn check_block_multiple_connections(server_address: SocketAddr) {
        let conn1 = make_client_endpoint(&server_address, None).await;
        let conn2 = make_client_endpoint(&server_address, None).await;
        let mut s1 = conn1.open_uni().await.unwrap();
        let s2 = conn2.open_uni().await;
        if let Ok(mut s2) = s2 {
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
        } else {
            // It has been noticed if there is already connection open against the server, this open_uni can fail
            // with ApplicationClosed(ApplicationClose) error due to CONNECTION_CLOSE_CODE_TOO_MANY before writing to
            // the stream -- expect it.
            assert_matches!(s2, Err(quinn::ConnectionError::ApplicationClosed(_)));
        }
    }

    pub async fn check_multiple_streams(
        receiver: Receiver<PacketBatch>,
        server_address: SocketAddr,
    ) {
        let conn1 = Arc::new(make_client_endpoint(&server_address, None).await);
        let conn2 = Arc::new(make_client_endpoint(&server_address, None).await);
        let mut num_expected_packets = 0;
        for i in 0..10 {
            info!("sending: {}", i);
            let c1 = conn1.clone();
            let c2 = conn2.clone();
            let mut s1 = c1.open_uni().await.unwrap();
            let mut s2 = c2.open_uni().await.unwrap();
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
            if let Ok(packets) = receiver.try_recv() {
                total_packets += packets.len();
                all_packets.push(packets)
            } else {
                sleep(Duration::from_secs(1)).await;
            }
            if total_packets == num_expected_packets {
                break;
            }
        }
        for batch in all_packets {
            for p in batch.iter() {
                assert_eq!(p.meta().size, 1);
            }
        }
        assert_eq!(total_packets, num_expected_packets);
    }

    pub async fn check_multiple_writes(
        receiver: Receiver<PacketBatch>,
        server_address: SocketAddr,
        client_keypair: Option<&Keypair>,
    ) {
        let conn1 = Arc::new(make_client_endpoint(&server_address, client_keypair).await);

        // Send a full size packet with single byte writes.
        let num_bytes = PACKET_DATA_SIZE;
        let num_expected_packets = 1;
        let mut s1 = conn1.open_uni().await.unwrap();
        for _ in 0..num_bytes {
            s1.write_all(&[0u8]).await.unwrap();
        }
        s1.finish().await.unwrap();

        let mut all_packets = vec![];
        let now = Instant::now();
        let mut total_packets = 0;
        while now.elapsed().as_secs() < 5 {
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
    }

    pub async fn check_unstaked_node_connect_failure(server_address: SocketAddr) {
        let conn1 = Arc::new(make_client_endpoint(&server_address, None).await);

        // Send a full size packet with single byte writes.
        if let Ok(mut s1) = conn1.open_uni().await {
            for _ in 0..PACKET_DATA_SIZE {
                // Ignoring any errors here. s1.finish() will test the error condition
                s1.write_all(&[0u8]).await.unwrap_or_default();
            }
            s1.finish().await.unwrap_err();
        }
    }

    #[tokio::test]
    async fn test_quic_server_exit() {
        let (t, exit, _receiver, _server_address, _stats) = setup_quic_server(None, 1);
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_timeout() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server(None, 1);
        check_timeout(receiver, server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_packet_batcher() {
        solana_logger::setup();
        let (pkt_batch_sender, pkt_batch_receiver) = unbounded();
        let (ptk_sender, pkt_receiver) = async_unbounded();
        let exit = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(StreamStats::default());

        let handle = tokio::spawn(packet_batch_sender(
            pkt_batch_sender,
            pkt_receiver,
            exit.clone(),
            stats,
            DEFAULT_TPU_COALESCE,
        ));

        let num_packets = 1000;

        for _i in 0..num_packets {
            let mut meta = Meta::default();
            let bytes = Bytes::from("Hello world");
            let offset = 0;
            let size = bytes.len();
            meta.size = size;
            let packet_accum = PacketAccumulator {
                meta,
                chunks: vec![PacketChunk {
                    bytes,
                    offset,
                    end_of_chunk: size,
                }],
            };
            ptk_sender.send(packet_accum).await.unwrap();
        }
        let mut i = 0;
        let start = Instant::now();
        while i < num_packets && start.elapsed().as_secs() < 2 {
            if let Ok(batch) = pkt_batch_receiver.try_recv() {
                i += batch.len();
            } else {
                sleep(Duration::from_millis(1)).await;
            }
        }
        assert_eq!(i, num_packets);
        exit.store(true, Ordering::Relaxed);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_stream_timeout() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address, stats) = setup_quic_server(None, 1);

        let conn1 = make_client_endpoint(&server_address, None).await;
        assert_eq!(stats.total_streams.load(Ordering::Relaxed), 0);
        assert_eq!(stats.total_stream_read_timeouts.load(Ordering::Relaxed), 0);

        // Send one byte to start the stream
        let mut s1 = conn1.open_uni().await.unwrap();
        s1.write_all(&[0u8]).await.unwrap_or_default();

        // Wait long enough for the stream to timeout in receiving chunks
        let sleep_time = Duration::from_secs(3).min(WAIT_FOR_STREAM_TIMEOUT * 1000);
        sleep(sleep_time).await;

        // Test that the stream was created, but timed out in read
        assert_eq!(stats.total_streams.load(Ordering::Relaxed), 0);
        assert_ne!(stats.total_stream_read_timeouts.load(Ordering::Relaxed), 0);

        // Test that more writes to the stream will fail (i.e. the stream is no longer writable
        // after the timeouts)
        assert!(s1.write_all(&[0u8]).await.is_err());
        assert!(s1.finish().await.is_err());

        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_block_multiple_connections() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address, _stats) = setup_quic_server(None, 1);
        check_block_multiple_connections(server_address).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_multiple_connections_on_single_client_endpoint() {
        solana_logger::setup();
        let (t, exit, _receiver, server_address, stats) = setup_quic_server(None, 2);

        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let mut endpoint = quinn::Endpoint::new(
            EndpointConfig::default(),
            None,
            client_socket,
            Arc::new(TokioRuntime),
        )
        .unwrap();
        let default_keypair = Keypair::new();
        endpoint.set_default_client_config(get_client_config(&default_keypair));
        let conn1 = endpoint
            .connect(server_address, "localhost")
            .expect("Failed in connecting")
            .await
            .expect("Failed in waiting");

        let conn2 = endpoint
            .connect(server_address, "localhost")
            .expect("Failed in connecting")
            .await
            .expect("Failed in waiting");

        let mut s1 = conn1.open_uni().await.unwrap();
        s1.write_all(&[0u8]).await.unwrap();
        s1.finish().await.unwrap();

        let mut s2 = conn2.open_uni().await.unwrap();
        conn1.close(
            CONNECTION_CLOSE_CODE_DROPPED_ENTRY.into(),
            CONNECTION_CLOSE_REASON_DROPPED_ENTRY,
        );
        // Wait long enough for the stream to timeout in receiving chunks
        let sleep_time = Duration::from_secs(1).min(WAIT_FOR_STREAM_TIMEOUT * 1000);
        sleep(sleep_time).await;

        assert_eq!(stats.connection_removed.load(Ordering::Relaxed), 1);

        s2.write_all(&[0u8]).await.unwrap();
        s2.finish().await.unwrap();

        conn2.close(
            CONNECTION_CLOSE_CODE_DROPPED_ENTRY.into(),
            CONNECTION_CLOSE_REASON_DROPPED_ENTRY,
        );
        // Wait long enough for the stream to timeout in receiving chunks
        let sleep_time = Duration::from_secs(1).min(WAIT_FOR_STREAM_TIMEOUT * 1000);
        sleep(sleep_time).await;

        assert_eq!(stats.connection_removed.load(Ordering::Relaxed), 2);

        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_multiple_writes() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, _stats) = setup_quic_server(None, 1);
        check_multiple_writes(receiver, server_address, None).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
    }

    #[tokio::test]
    async fn test_quic_server_staked_connection_removal() {
        solana_logger::setup();

        let client_keypair = Keypair::new();
        let stakes = HashMap::from([(client_keypair.pubkey(), 100_000)]);
        let staked_nodes = StakedNodes::new(
            Arc::new(stakes),
            HashMap::<Pubkey, u64>::default(), // overrides
        );
        let (t, exit, receiver, server_address, stats) = setup_quic_server(Some(staked_nodes), 1);
        check_multiple_writes(receiver, server_address, Some(&client_keypair)).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            stats
                .connection_added_from_unstaked_peer
                .load(Ordering::Relaxed),
            0
        );
        assert_eq!(stats.connection_removed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.connection_remove_failed.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_quic_server_zero_staked_connection_removal() {
        // In this test, the client has a pubkey, but is not in stake table.
        solana_logger::setup();

        let client_keypair = Keypair::new();
        let stakes = HashMap::from([(client_keypair.pubkey(), 0)]);
        let staked_nodes = StakedNodes::new(
            Arc::new(stakes),
            HashMap::<Pubkey, u64>::default(), // overrides
        );
        let (t, exit, receiver, server_address, stats) = setup_quic_server(Some(staked_nodes), 1);
        check_multiple_writes(receiver, server_address, Some(&client_keypair)).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            stats
                .connection_added_from_staked_peer
                .load(Ordering::Relaxed),
            0
        );
        assert_eq!(stats.connection_removed.load(Ordering::Relaxed), 1);
        assert_eq!(stats.connection_remove_failed.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn test_quic_server_unstaked_connection_removal() {
        solana_logger::setup();
        let (t, exit, receiver, server_address, stats) = setup_quic_server(None, 1);
        check_multiple_writes(receiver, server_address, None).await;
        exit.store(true, Ordering::Relaxed);
        t.await.unwrap();
        sleep(Duration::from_millis(100)).await;
        assert_eq!(
            stats
                .connection_added_from_staked_peer
                .load(Ordering::Relaxed),
            0
        );
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
        let (_, _, t) = spawn_server(
            "quic_streamer_test",
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            1,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            0, // Do not allow any connection from unstaked clients/nodes
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            DEFAULT_TPU_COALESCE,
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
        let (_, stats, t) = spawn_server(
            "quic_streamer_test",
            s,
            &keypair,
            ip,
            sender,
            exit.clone(),
            2,
            staked_nodes,
            MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS,
            DEFAULT_WAIT_FOR_CHUNK_TIMEOUT,
            DEFAULT_TPU_COALESCE,
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
    fn test_prune_table_with_ip() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);
        let mut num_entries = 5;
        let max_connections_per_peer = 10;
        let sockets: Vec<_> = (0..num_entries)
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(
                    ConnectionTableKey::IP(socket.ip()),
                    socket.port(),
                    None,
                    0,
                    i as u64,
                    max_connections_per_peer,
                )
                .unwrap();
        }
        num_entries += 1;
        table
            .try_add_connection(
                ConnectionTableKey::IP(sockets[0].ip()),
                sockets[0].port(),
                None,
                0,
                5,
                max_connections_per_peer,
            )
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
            table.remove_connection(ConnectionTableKey::IP(socket.ip()), socket.port(), 0);
        }
        assert_eq!(table.total_size, 0);
    }

    #[test]
    fn test_prune_table_with_unique_pubkeys() {
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);

        // We should be able to add more entries than max_connections_per_peer, since each entry is
        // from a different peer pubkey.
        let num_entries = 15;
        let max_connections_per_peer = 10;

        let pubkeys: Vec<_> = (0..num_entries).map(|_| Pubkey::new_unique()).collect();
        for (i, pubkey) in pubkeys.iter().enumerate() {
            table
                .try_add_connection(
                    ConnectionTableKey::Pubkey(*pubkey),
                    0,
                    None,
                    0,
                    i as u64,
                    max_connections_per_peer,
                )
                .unwrap();
        }

        let new_size = 3;
        let pruned = table.prune_oldest(new_size);
        assert_eq!(pruned, num_entries as usize - new_size);
        assert_eq!(table.table.len(), new_size);
        assert_eq!(table.total_size, new_size);
        for pubkey in pubkeys.iter().take(num_entries as usize).skip(new_size - 1) {
            table.remove_connection(ConnectionTableKey::Pubkey(*pubkey), 0, 0);
        }
        assert_eq!(table.total_size, 0);
    }

    #[test]
    fn test_prune_table_with_non_unique_pubkeys() {
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);

        let max_connections_per_peer = 10;
        let pubkey = Pubkey::new_unique();
        (0..max_connections_per_peer).for_each(|i| {
            table
                .try_add_connection(
                    ConnectionTableKey::Pubkey(pubkey),
                    0,
                    None,
                    0,
                    i as u64,
                    max_connections_per_peer,
                )
                .unwrap();
        });

        // We should NOT be able to add more entries than max_connections_per_peer, since we are
        // using the same peer pubkey.
        assert!(table
            .try_add_connection(
                ConnectionTableKey::Pubkey(pubkey),
                0,
                None,
                0,
                10,
                max_connections_per_peer,
            )
            .is_none());

        // We should be able to add an entry from another peer pubkey
        let num_entries = max_connections_per_peer + 1;
        let pubkey2 = Pubkey::new_unique();
        assert!(table
            .try_add_connection(
                ConnectionTableKey::Pubkey(pubkey2),
                0,
                None,
                0,
                10,
                max_connections_per_peer,
            )
            .is_some());

        assert_eq!(table.total_size, num_entries);

        let new_max_size = 3;
        let pruned = table.prune_oldest(new_max_size);
        assert!(pruned >= num_entries - new_max_size);
        assert!(table.table.len() <= new_max_size);
        assert!(table.total_size <= new_max_size);

        table.remove_connection(ConnectionTableKey::Pubkey(pubkey2), 0, 0);
        assert_eq!(table.total_size, 0);
    }

    #[test]
    fn test_prune_table_random() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);
        let num_entries = 5;
        let max_connections_per_peer = 10;
        let sockets: Vec<_> = (0..num_entries)
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(
                    ConnectionTableKey::IP(socket.ip()),
                    socket.port(),
                    None,
                    (i + 1) as u64,
                    i as u64,
                    max_connections_per_peer,
                )
                .unwrap();
        }

        // Try pruninng with threshold stake less than all the entries in the table
        // It should fail to prune (i.e. return 0 number of pruned entries)
        let pruned = table.prune_random(/*sample_size:*/ 2, /*threshold_stake:*/ 0);
        assert_eq!(pruned, 0);

        // Try pruninng with threshold stake higher than all the entries in the table
        // It should succeed to prune (i.e. return 1 number of pruned entries)
        let pruned = table.prune_random(
            2,                      // sample_size
            num_entries as u64 + 1, // threshold_stake
        );
        assert_eq!(pruned, 1);
    }

    #[test]
    fn test_remove_connections() {
        use std::net::Ipv4Addr;
        solana_logger::setup();
        let mut table = ConnectionTable::new(ConnectionPeerType::Staked);
        let num_ips = 5;
        let max_connections_per_peer = 10;
        let mut sockets: Vec<_> = (0..num_ips)
            .map(|i| SocketAddr::new(IpAddr::V4(Ipv4Addr::new(i, 0, 0, 0)), 0))
            .collect();
        for (i, socket) in sockets.iter().enumerate() {
            table
                .try_add_connection(
                    ConnectionTableKey::IP(socket.ip()),
                    socket.port(),
                    None,
                    0,
                    (i * 2) as u64,
                    max_connections_per_peer,
                )
                .unwrap();

            table
                .try_add_connection(
                    ConnectionTableKey::IP(socket.ip()),
                    socket.port(),
                    None,
                    0,
                    (i * 2 + 1) as u64,
                    max_connections_per_peer,
                )
                .unwrap();
        }

        let single_connection_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(num_ips, 0, 0, 0)), 0);
        table
            .try_add_connection(
                ConnectionTableKey::IP(single_connection_addr.ip()),
                single_connection_addr.port(),
                None,
                0,
                (num_ips * 2) as u64,
                max_connections_per_peer,
            )
            .unwrap();

        let zero_connection_addr =
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(num_ips + 1, 0, 0, 0)), 0);

        sockets.push(single_connection_addr);
        sockets.push(zero_connection_addr);

        for socket in sockets.iter() {
            table.remove_connection(ConnectionTableKey::IP(socket.ip()), socket.port(), 0);
        }
        assert_eq!(table.total_size, 0);
    }

    #[test]

    fn test_max_allowed_uni_streams() {
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Unstaked, 0, 0),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Unstaked, 10, 0),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Staked, 0, 0),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Staked, 10, 0),
            QUIC_MIN_STAKED_CONCURRENT_STREAMS
        );
        let delta =
            (QUIC_TOTAL_STAKED_CONCURRENT_STREAMS - QUIC_MIN_STAKED_CONCURRENT_STREAMS) as f64;
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Staked, 1000, 10000),
            QUIC_MAX_STAKED_CONCURRENT_STREAMS,
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Staked, 100, 10000),
            ((delta / (100_f64)) as usize + QUIC_MIN_STAKED_CONCURRENT_STREAMS)
                .min(QUIC_MAX_STAKED_CONCURRENT_STREAMS)
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Staked, 0, 10000),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Unstaked, 1000, 10000),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Unstaked, 1, 10000),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
        assert_eq!(
            compute_max_allowed_uni_streams(ConnectionPeerType::Unstaked, 0, 10000),
            QUIC_MAX_UNSTAKED_CONCURRENT_STREAMS
        );
    }

    #[test]
    fn test_cacluate_receive_window_ratio_for_staked_node() {
        let mut max_stake = 10000;
        let mut min_stake = 0;
        let ratio = compute_receive_window_ratio_for_staked_node(max_stake, min_stake, min_stake);
        assert_eq!(ratio, QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO);

        let ratio = compute_receive_window_ratio_for_staked_node(max_stake, min_stake, max_stake);
        let max_ratio = QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO;
        assert_eq!(ratio, max_ratio);

        let ratio =
            compute_receive_window_ratio_for_staked_node(max_stake, min_stake, max_stake / 2);
        let average_ratio =
            (QUIC_MAX_STAKED_RECEIVE_WINDOW_RATIO + QUIC_MIN_STAKED_RECEIVE_WINDOW_RATIO) / 2;
        assert_eq!(ratio, average_ratio);

        max_stake = 10000;
        min_stake = 10000;
        let ratio = compute_receive_window_ratio_for_staked_node(max_stake, min_stake, max_stake);
        assert_eq!(ratio, max_ratio);

        max_stake = 0;
        min_stake = 0;
        let ratio = compute_receive_window_ratio_for_staked_node(max_stake, min_stake, max_stake);
        assert_eq!(ratio, max_ratio);

        max_stake = 1000;
        min_stake = 10;
        let ratio =
            compute_receive_window_ratio_for_staked_node(max_stake, min_stake, max_stake + 10);
        assert_eq!(ratio, max_ratio);
    }

    #[test]
    fn test_max_streams_for_connection_in_100ms() {
        // 50K packets per ms * 20% / 500 max unstaked connections
        assert_eq!(
            max_streams_for_connection_in_100ms(ConnectionPeerType::Unstaked, 0, 10000),
            20
        );

        // 50K packets per ms * 20% / 500 max unstaked connections
        assert_eq!(
            max_streams_for_connection_in_100ms(ConnectionPeerType::Unstaked, 10, 10000),
            20
        );

        // If stake is 0, same limits as unstaked connections will apply.
        // 50K packets per ms * 20% / 500 max unstaked connections
        assert_eq!(
            max_streams_for_connection_in_100ms(ConnectionPeerType::Staked, 0, 10000),
            20
        );

        // max staked streams = 50K packets per ms * 80% = 40K
        // function = 40K * stake / total_stake
        assert_eq!(
            max_streams_for_connection_in_100ms(ConnectionPeerType::Staked, 15, 10000),
            60
        );

        // max staked streams = 50K packets per ms * 80% = 40K
        // function = 40K * stake / total_stake
        assert_eq!(
            max_streams_for_connection_in_100ms(ConnectionPeerType::Staked, 1000, 10000),
            4000
        );
    }
}
