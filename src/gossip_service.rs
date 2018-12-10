//! The `gossip_service` module implements the network control plane.

use crate::cluster_info::ClusterInfo;
use crate::db_ledger::DbLedger;
use crate::service::Service;
use crate::streamer;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{self, JoinHandle};

pub struct GossipService {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl GossipService {
    pub fn new(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        db_ledger: Option<Arc<RwLock<DbLedger>>>,
        gossip_socket: UdpSocket,
        exit: Arc<AtomicBool>,
    ) -> Self {
        let (request_sender, request_receiver) = channel();
        let gossip_socket = Arc::new(gossip_socket);
        trace!(
            "GossipService: id: {}, listening on: {:?}",
            &cluster_info.read().unwrap().my_data().id,
            gossip_socket.local_addr().unwrap()
        );
        let t_receiver =
            streamer::blob_receiver(gossip_socket.clone(), exit.clone(), request_sender);
        let (response_sender, response_receiver) = channel();
        let t_responder = streamer::responder("gossip", gossip_socket, response_receiver);
        let t_listen = ClusterInfo::listen(
            cluster_info.clone(),
            db_ledger,
            request_receiver,
            response_sender.clone(),
            exit.clone(),
        );
        let t_gossip = ClusterInfo::gossip(cluster_info.clone(), response_sender, exit.clone());
        let thread_hdls = vec![t_receiver, t_responder, t_listen, t_gossip];
        Self { exit, thread_hdls }
    }

    pub fn close(self) -> thread::Result<()> {
        self.exit.store(true, Ordering::Relaxed);
        self.join()
    }
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
        let cluster_info = ClusterInfo::new(tn.info.clone());
        let c = Arc::new(RwLock::new(cluster_info));
        let d = GossipService::new(&c, None, tn.sockets.gossip, exit.clone());
        d.close().expect("thread join");
    }
}
