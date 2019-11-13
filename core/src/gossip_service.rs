//! The `gossip_service` module implements the network control plane.

use crate::cluster_info::{ClusterInfo, VALIDATOR_PORT_RANGE};
use crate::contact_info::ContactInfo;
use crate::streamer;
use rand::{thread_rng, Rng};
use solana_client::thin_client::{create_client, ThinClient};
use solana_ledger::bank_forks::BankForks;
use solana_ledger::blocktree::Blocktree;
use solana_perf::recycler::Recycler;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::{SocketAddr, TcpListener, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, JoinHandle};
use std::time::{Duration, Instant};

pub struct GossipService {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl GossipService {
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        blocktree: Option<Arc<Blocktree>>,
        bank_forks: Option<Arc<RwLock<BankForks>>>,
        gossip_socket: UdpSocket,
        exit: &Arc<AtomicBool>,
    ) -> Self {
        let (request_sender, request_receiver) = channel();
        let gossip_socket = Arc::new(gossip_socket);
        trace!(
            "GossipService: id: {}, listening on: {:?}",
            &cluster_info.read().unwrap().my_data().id,
            gossip_socket.local_addr().unwrap()
        );
        let t_receiver = streamer::receiver(
            gossip_socket.clone(),
            &exit,
            request_sender,
            Recycler::default(),
            "gossip_receiver",
        );
        let (response_sender, response_receiver) = channel();
        let t_responder = streamer::responder("gossip", gossip_socket, response_receiver);
        let t_listen = ClusterInfo::listen(
            cluster_info.clone(),
            blocktree,
            bank_forks.clone(),
            request_receiver,
            response_sender.clone(),
            exit,
        );
        let t_gossip = ClusterInfo::gossip(cluster_info.clone(), bank_forks, response_sender, exit);
        let thread_hdls = vec![t_receiver, t_responder, t_listen, t_gossip];
        Self { thread_hdls }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

/// Discover Nodes and Archivers in a cluster
pub fn discover_cluster(
    entry_point: &SocketAddr,
    num_nodes: usize,
) -> std::io::Result<(Vec<ContactInfo>, Vec<ContactInfo>)> {
    discover(entry_point, Some(num_nodes), Some(30), None, None, None)
}

pub fn discover(
    entry_point: &SocketAddr,
    num_nodes: Option<usize>,
    timeout: Option<u64>,
    find_node_by_pubkey: Option<Pubkey>,
    find_node_by_gossip_addr: Option<SocketAddr>,
    my_gossip_addr: Option<&SocketAddr>,
) -> std::io::Result<(Vec<ContactInfo>, Vec<ContactInfo>)> {
    let exit = Arc::new(AtomicBool::new(false));
    let (gossip_service, ip_echo, spy_ref) = make_gossip_node(entry_point, &exit, my_gossip_addr);

    let id = spy_ref.read().unwrap().keypair.pubkey();
    info!("Gossip entry point: {:?}", entry_point);
    info!("Spy node id: {:?}", id);

    let _ip_echo_server = ip_echo.map(solana_net_utils::ip_echo_server);

    let (met_criteria, secs, tvu_peers, archivers) = spy(
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
            secs,
            spy_ref.read().unwrap().contact_info_trace()
        );
        return Ok((tvu_peers, archivers));
    }

    if !tvu_peers.is_empty() {
        info!(
            "discover failed to match criteria by timeout...\n{}",
            spy_ref.read().unwrap().contact_info_trace()
        );
        return Ok((tvu_peers, archivers));
    }

    info!(
        "discover failed...\n{}",
        spy_ref.read().unwrap().contact_info_trace()
    );
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Discover failed",
    ))
}

/// Creates a ThinClient per valid node
pub fn get_clients(nodes: &[ContactInfo]) -> Vec<ThinClient> {
    nodes
        .iter()
        .filter_map(ContactInfo::valid_client_facing_addr)
        .map(|addrs| create_client(addrs, VALIDATOR_PORT_RANGE))
        .collect()
}

/// Creates a ThinClient by selecting a valid node at random
pub fn get_client(nodes: &[ContactInfo]) -> ThinClient {
    let nodes: Vec<_> = nodes
        .iter()
        .filter_map(ContactInfo::valid_client_facing_addr)
        .collect();
    let select = thread_rng().gen_range(0, nodes.len());
    create_client(nodes[select], VALIDATOR_PORT_RANGE)
}

pub fn get_multi_client(nodes: &[ContactInfo]) -> (ThinClient, usize) {
    let addrs: Vec<_> = nodes
        .iter()
        .filter_map(ContactInfo::valid_client_facing_addr)
        .map(|addrs| addrs)
        .collect();
    let rpc_addrs: Vec<_> = addrs.iter().map(|addr| addr.0).collect();
    let tpu_addrs: Vec<_> = addrs.iter().map(|addr| addr.1).collect();
    let (_, transactions_socket) = solana_net_utils::bind_in_range(VALIDATOR_PORT_RANGE).unwrap();
    let num_nodes = tpu_addrs.len();
    (
        ThinClient::new_from_addrs(rpc_addrs, tpu_addrs, transactions_socket),
        num_nodes,
    )
}

fn spy(
    spy_ref: Arc<RwLock<ClusterInfo>>,
    num_nodes: Option<usize>,
    timeout: Option<u64>,
    find_node_by_pubkey: Option<Pubkey>,
    find_node_by_gossip_addr: Option<SocketAddr>,
) -> (bool, u64, Vec<ContactInfo>, Vec<ContactInfo>) {
    let now = Instant::now();
    let mut met_criteria = false;
    let mut tvu_peers: Vec<ContactInfo> = Vec::new();
    let mut archivers: Vec<ContactInfo> = Vec::new();
    let mut i = 0;
    loop {
        if let Some(secs) = timeout {
            if now.elapsed() >= Duration::from_secs(secs) {
                break;
            }
        }
        // collect tvu peers but filter out archivers since their tvu is transient and we do not want
        // it to show up as a "node"
        tvu_peers = spy_ref
            .read()
            .unwrap()
            .tvu_peers()
            .into_iter()
            .filter(|node| !ClusterInfo::is_archiver(&node))
            .collect::<Vec<_>>();
        archivers = spy_ref.read().unwrap().storage_peers();
        if let Some(num) = num_nodes {
            if tvu_peers.len() + archivers.len() >= num {
                if let Some(gossip_addr) = find_node_by_gossip_addr {
                    if tvu_peers
                        .iter()
                        .chain(archivers.iter())
                        .any(|x| x.gossip == gossip_addr)
                    {
                        met_criteria = true;
                        break;
                    }
                }
                if let Some(pubkey) = find_node_by_pubkey {
                    if tvu_peers
                        .iter()
                        .chain(archivers.iter())
                        .any(|x| x.id == pubkey)
                    {
                        met_criteria = true;
                        break;
                    }
                }
                if find_node_by_pubkey.is_none() && find_node_by_gossip_addr.is_none() {
                    met_criteria = true;
                    break;
                }
            }
        }
        if num_nodes.is_none() {
            if let Some(pubkey) = find_node_by_pubkey {
                if tvu_peers
                    .iter()
                    .chain(archivers.iter())
                    .any(|x| x.id == pubkey)
                {
                    met_criteria = true;
                    break;
                }
            }
            if let Some(gossip_addr) = find_node_by_gossip_addr {
                if tvu_peers
                    .iter()
                    .chain(archivers.iter())
                    .any(|x| x.gossip == gossip_addr)
                {
                    met_criteria = true;
                    break;
                }
            }
        }
        if i % 20 == 0 {
            info!(
                "discovering...\n{}",
                spy_ref.read().unwrap().contact_info_trace()
            );
        }
        sleep(Duration::from_millis(
            crate::cluster_info::GOSSIP_SLEEP_MILLIS,
        ));
        i += 1;
    }
    (met_criteria, now.elapsed().as_secs(), tvu_peers, archivers)
}

/// Makes a spy or gossip node based on whether or not a gossip_addr was passed in
/// Pass in a gossip addr to fully participate in gossip instead of relying on just pulls
fn make_gossip_node(
    entry_point: &SocketAddr,
    exit: &Arc<AtomicBool>,
    gossip_addr: Option<&SocketAddr>,
) -> (GossipService, Option<TcpListener>, Arc<RwLock<ClusterInfo>>) {
    let keypair = Arc::new(Keypair::new());
    let (node, gossip_socket, ip_echo) = if let Some(gossip_addr) = gossip_addr {
        ClusterInfo::gossip_node(&keypair.pubkey(), gossip_addr)
    } else {
        ClusterInfo::spy_node(&keypair.pubkey())
    };
    let mut cluster_info = ClusterInfo::new(node, keypair);
    cluster_info.set_entrypoint(ContactInfo::new_gossip_entry_point(entry_point));
    let cluster_info = Arc::new(RwLock::new(cluster_info));
    let gossip_service =
        GossipService::new(&cluster_info.clone(), None, None, gossip_socket, &exit);
    (gossip_service, ip_echo, cluster_info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster_info::{ClusterInfo, Node};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, RwLock};

    #[test]
    #[ignore]
    // test that stage will exit when flag is set
    fn test_exit() {
        let exit = Arc::new(AtomicBool::new(false));
        let tn = Node::new_localhost();
        let cluster_info = ClusterInfo::new_with_invalid_keypair(tn.info.clone());
        let c = Arc::new(RwLock::new(cluster_info));
        let d = GossipService::new(&c, None, None, tn.sockets.gossip, &exit);
        exit.store(true, Ordering::Relaxed);
        d.join().unwrap();
    }

    #[test]
    fn test_gossip_services_spy() {
        let keypair = Keypair::new();
        let peer0 = Pubkey::new_rand();
        let peer1 = Pubkey::new_rand();
        let contact_info = ContactInfo::new_localhost(&keypair.pubkey(), 0);
        let peer0_info = ContactInfo::new_localhost(&peer0, 0);
        let peer1_info = ContactInfo::new_localhost(&peer1, 0);
        let mut cluster_info = ClusterInfo::new(contact_info.clone(), Arc::new(keypair));
        cluster_info.insert_info(peer0_info.clone());
        cluster_info.insert_info(peer1_info);

        let spy_ref = Arc::new(RwLock::new(cluster_info));

        let (met_criteria, secs, tvu_peers, _) = spy(spy_ref.clone(), None, Some(1), None, None);
        assert_eq!(met_criteria, false);
        assert_eq!(secs, 1);
        assert_eq!(tvu_peers, spy_ref.read().unwrap().tvu_peers());

        // Find num_nodes
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(1), None, None, None);
        assert_eq!(met_criteria, true);
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(2), None, None, None);
        assert_eq!(met_criteria, true);

        // Find specific node by pubkey
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), None, None, Some(peer0), None);
        assert_eq!(met_criteria, true);
        let (met_criteria, _, _, _) = spy(
            spy_ref.clone(),
            None,
            Some(0),
            Some(Pubkey::new_rand()),
            None,
        );
        assert_eq!(met_criteria, false);

        // Find num_nodes *and* specific node by pubkey
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(1), None, Some(peer0), None);
        assert_eq!(met_criteria, true);
        let (met_criteria, _, _, _) = spy(spy_ref.clone(), Some(3), Some(0), Some(peer0), None);
        assert_eq!(met_criteria, false);
        let (met_criteria, _, _, _) = spy(
            spy_ref.clone(),
            Some(1),
            Some(0),
            Some(Pubkey::new_rand()),
            None,
        );
        assert_eq!(met_criteria, false);

        // Find specific node by gossip address
        let (met_criteria, _, _, _) =
            spy(spy_ref.clone(), None, None, None, Some(peer0_info.gossip));
        assert_eq!(met_criteria, true);
        let (met_criteria, _, _, _) = spy(
            spy_ref.clone(),
            None,
            Some(0),
            None,
            Some("1.1.1.1:1234".parse().unwrap()),
        );
        assert_eq!(met_criteria, false);
    }
}
