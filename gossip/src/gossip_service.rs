//! The `gossip_service` module implements the network control plane.

use {
    crate::{cluster_info::ClusterInfo, legacy_contact_info::LegacyContactInfo as ContactInfo},
    crossbeam_channel::{unbounded, Sender},
    rand::{thread_rng, Rng},
    solana_client::{connection_cache::ConnectionCache, thin_client::ThinClient, tpu_connection::TpuConnection},
    solana_perf::recycler::Recycler,
    solana_runtime::bank_forks::BankForks,
    crossbeam_channel::RecvTimeoutError,
    solana_sdk::{
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        quic::MAX_QUIC_CONNECTIONS_PER_PEER,
        timing::timestamp,
    },
    solana_streamer::{
        streamer::StakedNodes,
        socket::SocketAddrSpace,
        streamer::{self, StreamerReceiveStats, StreamerSendStats, PacketBatchReceiver, StreamerError, Result},
        quic::{spawn_server, MAX_STAKED_CONNECTIONS,
            MAX_UNSTAKED_CONNECTIONS, StreamStats},
        nonblocking::quic::{DEFAULT_WAIT_FOR_CHUNK_TIMEOUT_MS},
    },
    
    std::{
        collections::HashSet,
        net::{SocketAddr, TcpListener, UdpSocket},
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
        thread::{self, sleep, JoinHandle, Builder},
        time::{Duration, Instant},
    },
};

pub struct GossipService {
    thread_hdls: Vec<JoinHandle<()>>,
}

fn recv_send_quic(
    connection_cache: &ConnectionCache,
    r: &PacketBatchReceiver,
    socket_addr_space: &SocketAddrSpace,
    stats: &mut Option<StreamerSendStats>,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let packet_batch = r.recv_timeout(timer)?;
    if let Some(stats) = stats {
        packet_batch.iter().for_each(|p| stats.record(p));
    }
    let packets = packet_batch.iter().filter_map(|pkt| {
        let addr = pkt.meta().socket_addr();
        let data = pkt.data(..)?;
        socket_addr_space.check(&addr).then_some((data, addr))
    });
    for p in packets {
        let conn = connection_cache.get_connection(&p.1);
        //todo: handle this error (e.g. logging)
        let _ = conn.send_wire_transaction(p.0);
    }
    Ok(())
}

fn quic_responder(
    name: &'static str,
    connection_cache: ConnectionCache,
    r: PacketBatchReceiver,
    socket_addr_space: SocketAddrSpace,
    stats_reporter_sender: Option<Sender<Box<dyn FnOnce() + Send>>>,
) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solQuicRspndr{name}"))
        .spawn(move || {
            let mut errors = 0;
            let mut last_error = None;
            let mut last_print = 0;
            let mut stats = None;

            if stats_reporter_sender.is_some() {
                stats = Some(StreamerSendStats::default());
            }

            loop {
                if let Err(e) = recv_send_quic(&connection_cache, &r, &socket_addr_space, &mut stats) {
                    match e {
                        StreamerError::RecvTimeout(RecvTimeoutError::Disconnected) => break,
                        StreamerError::RecvTimeout(RecvTimeoutError::Timeout) => (),
                        _ => {
                            errors += 1;
                            last_error = Some(e);
                        }
                    }
                }
                let now = timestamp();
                if now - last_print > 1000 && errors != 0 {
                    datapoint_info!(name, ("errors", errors, i64),);
                    info!("{} last-error: {:?} count: {}", name, last_error, errors);
                    last_print = now;
                    errors = 0;
                }
                if let Some(ref stats_reporter_sender) = stats_reporter_sender {
                    if let Some(ref mut stats) = stats {
                        stats.maybe_submit(name, stats_reporter_sender);
                    }
                }
            }
        })
        .unwrap()
}


impl GossipService {
    pub fn new(
        cluster_info: &Arc<ClusterInfo>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        gossip_socket: UdpSocket,
        gossip_validators: Option<HashSet<Pubkey>>,
        should_check_duplicate_instance: bool,
        stats_reporter_sender: Option<Sender<Box<dyn FnOnce() + Send>>>,
        exit: &Arc<AtomicBool>,
        use_quic: bool,
        keypair: Arc<Keypair>,
        staked_nodes: Arc<RwLock<StakedNodes>>,
    ) -> Self {
        let (request_sender, request_receiver) = unbounded();
        let gossip_socket = Arc::new(gossip_socket);
        trace!(
            "GossipService: id: {}, listening on: {:?}",
            &cluster_info.id(),
            gossip_socket.local_addr().unwrap()
        );
        let socket_addr_space = *cluster_info.socket_addr_space();
        let (maybe_endpoint, t_receiver) = if use_quic{
            let stats = Arc::new(StreamStats::default());
            //todo: fix
            let (endpoint, t_receiver) = spawn_server(
                gossip_socket.try_clone().unwrap(),
                &keypair,
                cluster_info.my_contact_info().tpu.ip(),
                request_sender,
                exit.clone(),
                MAX_QUIC_CONNECTIONS_PER_PEER,
                staked_nodes,
                MAX_STAKED_CONNECTIONS,
                MAX_UNSTAKED_CONNECTIONS,
                stats,
                DEFAULT_WAIT_FOR_CHUNK_TIMEOUT_MS
            ).unwrap();

            (Some(endpoint), t_receiver)
        } else{
        (None, streamer::receiver(
            gossip_socket.clone(),
            exit.clone(),
            request_sender,
            Recycler::default(),
            Arc::new(StreamerReceiveStats::new("gossip_receiver")),
            1,
            false,
            None,
        ))};
        let (consume_sender, listen_receiver) = unbounded();
        let t_socket_consume = cluster_info.clone().start_socket_consume_thread(
            request_receiver,
            consume_sender,
            exit.clone(),
        );
        let (response_sender, response_receiver) = unbounded();
        let t_listen = cluster_info.clone().listen(
            bank_forks.clone(),
            listen_receiver,
            response_sender.clone(),
            should_check_duplicate_instance,
            exit.clone(),
        );
        let t_gossip = cluster_info.clone().gossip(
            bank_forks,
            response_sender,
            gossip_validators,
            exit.clone(),
        );
        let t_responder = if use_quic {
            let connection_cache = ConnectionCache::new_with_maybe_endpoint_and_offset(1, Some(maybe_endpoint.unwrap()), Some(0));
            quic_responder(            
            "Gossip",
            connection_cache,
            response_receiver,
            socket_addr_space,
            stats_reporter_sender,)
        }
        else {
        streamer::responder(
            "Gossip",
            gossip_socket,
            response_receiver,
            socket_addr_space,
            stats_reporter_sender,
        )};
        let thread_hdls = vec![
            t_receiver,
            t_responder,
            t_socket_consume,
            t_listen,
            t_gossip,
        ];
        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

/// Discover Validators in a cluster
pub fn discover_cluster(
    entrypoint: &SocketAddr,
    num_nodes: usize,
    socket_addr_space: SocketAddrSpace,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    use_quic: bool
) -> std::io::Result<Vec<ContactInfo>> {
    const DISCOVER_CLUSTER_TIMEOUT: Duration = Duration::from_secs(120);
    let (_all_peers, validators) = discover(
        None, // keypair
        Some(entrypoint),
        Some(num_nodes),
        DISCOVER_CLUSTER_TIMEOUT,
        None, // find_node_by_pubkey
        None, // find_node_by_gossip_addr
        None, // my_gossip_addr
        0,    // my_shred_version
        socket_addr_space,
        staked_nodes,
        use_quic
    )?;
    Ok(validators)
}

pub fn discover(
    keypair: Option<Keypair>,
    entrypoint: Option<&SocketAddr>,
    num_nodes: Option<usize>, // num_nodes only counts validators, excludes spy nodes
    timeout: Duration,
    find_node_by_pubkey: Option<Pubkey>,
    find_node_by_gossip_addr: Option<&SocketAddr>,
    my_gossip_addr: Option<&SocketAddr>,
    my_shred_version: u16,
    socket_addr_space: SocketAddrSpace,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    use_quic: bool
) -> std::io::Result<(
    Vec<ContactInfo>, // all gossip peers
    Vec<ContactInfo>, // tvu peers (validators)
)> {
    let keypair = keypair.unwrap_or_else(Keypair::new);
    let exit = Arc::new(AtomicBool::new(false));
    let (gossip_service, ip_echo, spy_ref) = make_gossip_node(
        keypair,
        entrypoint,
        &exit,
        my_gossip_addr,
        my_shred_version,
        true, // should_check_duplicate_instance,
        socket_addr_space,
        staked_nodes,
        use_quic
    );

    let id = spy_ref.id();
    info!("Entrypoint: {:?}", entrypoint);
    info!("Node Id: {:?}", id);
    if let Some(my_gossip_addr) = my_gossip_addr {
        info!("Gossip Address: {:?}", my_gossip_addr);
    }
    let _ip_echo_server = ip_echo
        .map(|tcp_listener| solana_net_utils::ip_echo_server(tcp_listener, Some(my_shred_version)));
    let (met_criteria, elapsed, all_peers, tvu_peers) = spy(
        spy_ref.clone(),
        num_nodes,
        timeout,
        find_node_by_pubkey,
        find_node_by_gossip_addr,
    );

    exit.store(true, Ordering::Relaxed);
    gossip_service.join().unwrap();

    if met_criteria {
        info!(
            "discover success in {}s...\n{}",
            elapsed.as_secs(),
            spy_ref.contact_info_trace()
        );
        return Ok((all_peers, tvu_peers));
    }

    if !tvu_peers.is_empty() {
        info!(
            "discover failed to match criteria by timeout...\n{}",
            spy_ref.contact_info_trace()
        );
        return Ok((all_peers, tvu_peers));
    }

    info!("discover failed...\n{}", spy_ref.contact_info_trace());
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Discover failed",
    ))
}

/// Creates a ThinClient by selecting a valid node at random
pub fn get_client(
    nodes: &[ContactInfo],
    socket_addr_space: &SocketAddrSpace,
    connection_cache: Arc<ConnectionCache>,
) -> ThinClient {
    let nodes: Vec<_> = nodes
        .iter()
        .filter_map(|node| ContactInfo::valid_client_facing_addr(node, socket_addr_space))
        .collect();
    let select = thread_rng().gen_range(0, nodes.len());
    let (rpc, tpu) = nodes[select];
    ThinClient::new(rpc, tpu, connection_cache)
}

pub fn get_multi_client(
    nodes: &[ContactInfo],
    socket_addr_space: &SocketAddrSpace,
    connection_cache: Arc<ConnectionCache>,
) -> (ThinClient, usize) {
    let addrs: Vec<_> = nodes
        .iter()
        .filter_map(|node| ContactInfo::valid_client_facing_addr(node, socket_addr_space))
        .collect();
    let rpc_addrs: Vec<_> = addrs.iter().map(|addr| addr.0).collect();
    let tpu_addrs: Vec<_> = addrs.iter().map(|addr| addr.1).collect();

    let num_nodes = tpu_addrs.len();
    (
        ThinClient::new_from_addrs(rpc_addrs, tpu_addrs, connection_cache),
        num_nodes,
    )
}

fn spy(
    spy_ref: Arc<ClusterInfo>,
    num_nodes: Option<usize>,
    timeout: Duration,
    find_node_by_pubkey: Option<Pubkey>,
    find_node_by_gossip_addr: Option<&SocketAddr>,
) -> (
    bool,             // if found the specified nodes
    Duration,         // elapsed time until found the nodes or timed-out
    Vec<ContactInfo>, // all gossip peers
    Vec<ContactInfo>, // tvu peers (validators)
) {
    let now = Instant::now();
    let mut met_criteria = false;
    let mut all_peers: Vec<ContactInfo> = Vec::new();
    let mut tvu_peers: Vec<ContactInfo> = Vec::new();
    let mut i = 1;
    while !met_criteria && now.elapsed() < timeout {
        all_peers = spy_ref
            .all_peers()
            .into_iter()
            .map(|x| x.0)
            .collect::<Vec<_>>();
        tvu_peers = spy_ref.all_tvu_peers();

        let found_node_by_pubkey = if let Some(pubkey) = find_node_by_pubkey {
            all_peers.iter().any(|x| x.id == pubkey)
        } else {
            false
        };

        let found_node_by_gossip_addr = if let Some(gossip_addr) = find_node_by_gossip_addr {
            all_peers.iter().any(|x| x.gossip == *gossip_addr)
        } else {
            false
        };

        if let Some(num) = num_nodes {
            // Only consider validators and archives for `num_nodes`
            let mut nodes: Vec<_> = tvu_peers.iter().collect();
            nodes.sort();
            nodes.dedup();

            if nodes.len() >= num {
                if found_node_by_pubkey || found_node_by_gossip_addr {
                    met_criteria = true;
                }

                if find_node_by_pubkey.is_none() && find_node_by_gossip_addr.is_none() {
                    met_criteria = true;
                }
            }
        } else if found_node_by_pubkey || found_node_by_gossip_addr {
            met_criteria = true;
        }
        if i % 20 == 0 {
            info!("discovering...\n{}", spy_ref.contact_info_trace());
        }
        sleep(Duration::from_millis(
            crate::cluster_info::GOSSIP_SLEEP_MILLIS,
        ));
        i += 1;
    }
    (met_criteria, now.elapsed(), all_peers, tvu_peers)
}

/// Makes a spy or gossip node based on whether or not a gossip_addr was passed in
/// Pass in a gossip addr to fully participate in gossip instead of relying on just pulls
pub fn make_gossip_node(
    keypair: Keypair,
    entrypoint: Option<&SocketAddr>,
    exit: &Arc<AtomicBool>,
    gossip_addr: Option<&SocketAddr>,
    shred_version: u16,
    should_check_duplicate_instance: bool,
    socket_addr_space: SocketAddrSpace,
    staked_nodes: Arc<RwLock<StakedNodes>>,
    use_quic: bool
) -> (GossipService, Option<TcpListener>, Arc<ClusterInfo>) {
    let keypair = Arc::new(keypair);
    let (node, gossip_socket, ip_echo) = if let Some(gossip_addr) = gossip_addr {
        ClusterInfo::gossip_node(keypair.pubkey(), gossip_addr, shred_version)
    } else {
        ClusterInfo::spy_node(keypair.pubkey(), shred_version)
    };
    let cluster_info = ClusterInfo::new(node, keypair.clone(), socket_addr_space);
    if let Some(entrypoint) = entrypoint {
        cluster_info.set_entrypoint(ContactInfo::new_gossip_entry_point(entrypoint));
    }
    let cluster_info = Arc::new(cluster_info);
    let gossip_service = GossipService::new(
        &cluster_info,
        None,
        gossip_socket,
        None,
        should_check_duplicate_instance,
        None,
        exit,
        use_quic,
        keypair,
        staked_nodes
    );
    (gossip_service, ip_echo, cluster_info)
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::cluster_info::{ClusterInfo, Node},
        std::sync::{atomic::AtomicBool, Arc},
    };

    #[test]
    #[ignore]
    // test that stage will exit when flag is set
    fn test_exit() {
        let exit = Arc::new(AtomicBool::new(false));
        let tn = Node::new_localhost();
        let cluster_info = ClusterInfo::new(
            tn.info.clone(),
            Arc::new(Keypair::new()),
            SocketAddrSpace::Unspecified,
        );
        let c = Arc::new(cluster_info);
        let d = GossipService::new(
            &c,
            None,
            tn.sockets.gossip,
            None,
            true, // should_check_duplicate_instance
            None,
            &exit,
            //todo: fix
            false,
            //keypair
            //staked_nodes
        );
        exit.store(true, Ordering::Relaxed);
        d.join().unwrap();
    }

    #[test]
    fn test_gossip_services_spy() {
        const TIMEOUT: Duration = Duration::from_secs(5);
        let keypair = Keypair::new();
        let peer0 = solana_sdk::pubkey::new_rand();
        let peer1 = solana_sdk::pubkey::new_rand();
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let peer0_info = ContactInfo::new_localhost(&peer0, 0);
        let peer1_info = ContactInfo::new_localhost(&peer1, 0);
        let cluster_info = ClusterInfo::new(
            contact_info,
            Arc::new(keypair),
            SocketAddrSpace::Unspecified,
        );
        cluster_info.insert_info(peer0_info.clone());
        cluster_info.insert_info(peer1_info);

        let spy_ref = Arc::new(cluster_info);

        let (met_criteria, elapsed, _, tvu_peers) = spy(spy_ref.clone(), None, TIMEOUT, None, None);
        assert!(!met_criteria);
        assert!((TIMEOUT..TIMEOUT + Duration::from_secs(1)).contains(&elapsed));
        assert_eq!(tvu_peers, spy_ref.tvu_peers());

        // Find num_nodes
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(1), TIMEOUT, None, None);
        assert!(met_criteria);
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(2), TIMEOUT, None, None);
        assert!(met_criteria);

        // Find specific node by pubkey
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), None, TIMEOUT, Some(peer0), None);
        assert!(met_criteria);
        let (met_criteria, _, _, _) = spy(
            spy_ref.clone(),
            None,
            TIMEOUT,
            Some(solana_sdk::pubkey::new_rand()),
            None,
        );
        assert!(!met_criteria);

        // Find num_nodes *and* specific node by pubkey
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(1), TIMEOUT, Some(peer0), None);
        assert!(met_criteria);
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(3), TIMEOUT, Some(peer0), None);
        assert!(!met_criteria);
        let (met_criteria, _, _, _) = spy(
            spy_ref.clone(),
            Some(1),
            TIMEOUT,
            Some(solana_sdk::pubkey::new_rand()),
            None,
        );
        assert!(!met_criteria);

        // Find specific node by gossip address
        let (met_criteria, _, _, _) = spy(
            spy_ref.clone(),
            None,
            TIMEOUT,
            None,
            Some(&peer0_info.gossip),
        );
        assert!(met_criteria);

        let (met_criteria, _, _, _) = spy(
            spy_ref,
            None,
            TIMEOUT,
            None,
            Some(&"1.1.1.1:1234".parse().unwrap()),
        );
        assert!(!met_criteria);
    }
}
