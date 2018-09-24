use blob_fetch_stage::BlobFetchStage;
use crdt::{Crdt, Node, NodeInfo};
use ncp::Ncp;
use service::Service;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use store_ledger_stage::StoreLedgerStage;
use streamer::BlobReceiver;
use window;
use window_service::{window_service, WindowServiceReturnType};

pub struct Replicator {
    ncp: Ncp,
    fetch_stage: BlobFetchStage,
    store_ledger_stage: StoreLedgerStage,
    t_window: JoinHandle<Option<WindowServiceReturnType>>,
    pub retransmit_receiver: BlobReceiver,
}

impl Replicator {
    pub fn new(
        entry_height: u64,
        max_entry_height: u64,
        exit: &Arc<AtomicBool>,
        ledger_path: Option<&str>,
        node: Node,
        network_addr: Option<SocketAddr>,
        done: Arc<AtomicBool>,
    ) -> Replicator {
        let window = window::new_window_from_entries(&[], entry_height, &node.info);
        let shared_window = Arc::new(RwLock::new(window));

        let crdt = Arc::new(RwLock::new(Crdt::new(node.info).expect("Crdt::new")));

        let leader_info = network_addr.map(|i| NodeInfo::new_entry_point(&i));

        if let Some(leader_info) = leader_info.as_ref() {
            crdt.write().unwrap().insert(leader_info);
        } else {
            panic!("No leader info!");
        }

        let repair_socket = Arc::new(node.sockets.repair);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            node.sockets.replicate.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new_multi_socket(blob_sockets, exit.clone());

        let (entry_window_sender, entry_window_receiver) = channel();
        // todo: pull blobs off the retransmit_receiver and recycle them?
        let (retransmit_sender, retransmit_receiver) = channel();
        let t_window = window_service(
            crdt.clone(),
            shared_window.clone(),
            entry_height,
            max_entry_height,
            blob_fetch_receiver,
            entry_window_sender,
            retransmit_sender,
            repair_socket,
            done,
        );

        let store_ledger_stage = StoreLedgerStage::new(entry_window_receiver, ledger_path);

        let ncp = Ncp::new(
            &crdt,
            shared_window.clone(),
            ledger_path,
            node.sockets.gossip,
            exit.clone(),
        );

        Replicator {
            ncp,
            fetch_stage,
            store_ledger_stage,
            t_window,
            retransmit_receiver,
        }
    }

    pub fn join(self) {
        self.ncp.join().unwrap();
        self.fetch_stage.join().unwrap();
        self.t_window.join().unwrap();
        self.store_ledger_stage.join().unwrap();

        // Drain the queue here to prevent self.retransmit_receiver from being dropped
        // before the window_service thread is joined
        let mut retransmit_queue_count = 0;
        while let Ok(_blob) = self.retransmit_receiver.recv_timeout(Duration::new(1, 0)) {
            retransmit_queue_count += 1;
        }
        debug!("retransmit channel count: {}", retransmit_queue_count);
    }
}

#[cfg(test)]
mod tests {
    use client::mk_client;
    use crdt::Node;
    use fullnode::Fullnode;
    use ledger::{genesis, read_ledger};
    use logger;
    use replicator::Replicator;
    use signature::{Keypair, KeypairUtil};
    use std::fs::remove_dir_all;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_replicator_startup() {
        logger::setup();
        info!("starting replicator test");
        let entry_height = 0;
        let replicator_ledger_path = "replicator_test_replicator_ledger";

        let exit = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));

        let leader_ledger_path = "replicator_test_leader_ledger";
        let (mint, leader_ledger_path) = genesis(leader_ledger_path, 100);

        info!("starting leader node");
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let network_addr = leader_node.sockets.gossip.local_addr().unwrap();
        let leader_info = leader_node.info.clone();
        let leader_rotation_interval = 20;
        let leader = Fullnode::new(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            None,
            false,
            Some(leader_rotation_interval),
        );

        let mut leader_client = mk_client(&leader_info);

        let bob = Keypair::new();

        let last_id = leader_client.get_last_id();
        leader_client
            .transfer(1, &mint.keypair(), bob.pubkey(), &last_id)
            .unwrap();

        let replicator_keypair = Keypair::new();

        info!("starting replicator node");
        let replicator_node = Node::new_localhost_with_pubkey(replicator_keypair.pubkey());
        let replicator = Replicator::new(
            entry_height,
            1,
            &exit,
            Some(replicator_ledger_path),
            replicator_node,
            Some(network_addr),
            done.clone(),
        );

        let mut num_entries = 0;
        for _ in 0..60 {
            match read_ledger(replicator_ledger_path, true) {
                Ok(entries) => {
                    for _ in entries {
                        num_entries += 1;
                    }
                    info!("{} entries", num_entries);
                    if num_entries > 0 {
                        break;
                    }
                }
                Err(e) => {
                    info!("error reading ledger: {:?}", e);
                }
            }
            sleep(Duration::from_millis(300));
            let last_id = leader_client.get_last_id();
            leader_client
                .transfer(1, &mint.keypair(), bob.pubkey(), &last_id)
                .unwrap();
        }
        assert_eq!(done.load(Ordering::Relaxed), true);
        assert!(num_entries > 0);
        exit.store(true, Ordering::Relaxed);
        replicator.join();
        leader.exit();
        let _ignored = remove_dir_all(&leader_ledger_path);
        let _ignored = remove_dir_all(&replicator_ledger_path);
    }
}
