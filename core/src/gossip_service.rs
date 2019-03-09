//! The `gossip_service` module implements the network control plane.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::cluster_info::{ClusterInfo, NodeInfo};
use crate::contact_info::ContactInfo;
use crate::service::Service;
use crate::streamer;
use solana_sdk::signature::{Keypair, KeypairUtil};
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::{self, JoinHandle};
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
        let t_receiver = streamer::blob_receiver(gossip_socket.clone(), &exit, request_sender);
        let (response_sender, response_receiver) = channel();
        let t_responder = streamer::responder("gossip", gossip_socket, response_receiver);
        let t_listen = ClusterInfo::listen(
            cluster_info.clone(),
            blocktree,
            request_receiver,
            response_sender.clone(),
            exit,
        );
        let t_gossip = ClusterInfo::gossip(cluster_info.clone(), bank_forks, response_sender, exit);
        let thread_hdls = vec![t_receiver, t_responder, t_listen, t_gossip];
        Self { thread_hdls }
    }
}

pub fn discover(gossip_addr: &SocketAddr, num_nodes: usize) -> std::io::Result<Vec<NodeInfo>> {
    let exit = Arc::new(AtomicBool::new(false));
    let (gossip_service, spy_ref) = make_spy_node(gossip_addr, &exit);
    let id = spy_ref.read().unwrap().keypair.pubkey();
    trace!(
        "discover: spy_node {} looking for at least {} nodes",
        id,
        num_nodes
    );

    // Wait for the cluster to converge
    let now = Instant::now();
    let mut i = 0;
    while now.elapsed() < Duration::from_secs(30) {
        let rpc_peers = spy_ref.read().unwrap().rpc_peers();
        if rpc_peers.len() >= num_nodes {
            info!(
                "discover success in {}s...\n{}",
                now.elapsed().as_secs(),
                spy_ref.read().unwrap().node_info_trace()
            );

            exit.store(true, Ordering::Relaxed);
            gossip_service.join().unwrap();
            return Ok(rpc_peers);
        }
        if i % 20 == 0 {
            info!(
                "discovering...\n{}",
                spy_ref.read().unwrap().node_info_trace()
            );
        }
        sleep(Duration::from_millis(
            crate::cluster_info::GOSSIP_SLEEP_MILLIS,
        ));
        i += 1;
    }

    exit.store(true, Ordering::Relaxed);
    gossip_service.join().unwrap();
    info!(
        "discover failed...\n{}",
        spy_ref.read().unwrap().node_info_trace()
    );
    Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Failed to converge",
    ))
}

fn make_spy_node(
    gossip_addr: &SocketAddr,
    exit: &Arc<AtomicBool>,
) -> (GossipService, Arc<RwLock<ClusterInfo>>) {
    let keypair = Arc::new(Keypair::new());
    let (node, gossip_socket) = ClusterInfo::spy_node(&keypair.pubkey());
    let mut cluster_info = ClusterInfo::new(node, keypair);
    cluster_info.insert_info(ContactInfo::new_entry_point(gossip_addr));

    let cluster_info = Arc::new(RwLock::new(cluster_info));
    let gossip_service =
        GossipService::new(&cluster_info.clone(), None, None, gossip_socket, &exit);
    (gossip_service, cluster_info)
}

impl Service for GossipService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
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
}
