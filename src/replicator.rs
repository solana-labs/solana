use blob_fetch_stage::BlobFetchStage;
use cluster_info::{ClusterInfo, Node, NodeInfo};
use db_ledger::DbLedger;
use leader_scheduler::LeaderScheduler;
use ncp::Ncp;
use service::Service;
use solana_sdk::hash::{Hash, Hasher};
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::{Error, ErrorKind};
use std::mem::size_of;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;
use store_ledger_stage::StoreLedgerStage;
use streamer::BlobReceiver;
use thin_client::poll_gossip_for_leader;
use window;
use window_service::window_service;

pub struct Replicator {
    ncp: Ncp,
    fetch_stage: BlobFetchStage,
    store_ledger_stage: StoreLedgerStage,
    t_window: JoinHandle<()>,
    pub retransmit_receiver: BlobReceiver,
}

pub fn sample_file(in_path: &Path, sample_offsets: &[u64]) -> io::Result<Hash> {
    let in_file = File::open(in_path)?;
    let metadata = in_file.metadata()?;
    let mut buffer_file = BufReader::new(in_file);

    let mut hasher = Hasher::default();
    let sample_size = size_of::<Hash>();
    let sample_size64 = sample_size as u64;
    let mut buf = vec![0; sample_size];

    let file_len = metadata.len();
    if file_len < sample_size64 {
        return Err(Error::new(ErrorKind::Other, "file too short!"));
    }
    for offset in sample_offsets {
        if *offset > (file_len - sample_size64) / sample_size64 {
            return Err(Error::new(ErrorKind::Other, "offset too large"));
        }
        buffer_file.seek(SeekFrom::Start(*offset * sample_size64))?;
        trace!("sampling @ {} ", *offset);
        match buffer_file.read(&mut buf) {
            Ok(size) => {
                assert_eq!(size, buf.len());
                hasher.hash(&buf);
            }
            Err(e) => {
                warn!("Error sampling file");
                return Err(e);
            }
        }
    }

    Ok(hasher.result())
}

impl Replicator {
    pub fn new(
        db_ledger: Arc<RwLock<DbLedger>>,
        entry_height: u64,
        max_entry_height: u64,
        exit: &Arc<AtomicBool>,
        ledger_path: Option<&str>,
        node: Node,
        network_addr: Option<SocketAddr>,
        done: Arc<AtomicBool>,
    ) -> (Replicator, NodeInfo) {
        const REPLICATOR_WINDOW_SIZE: usize = 32 * 1024;
        let window = window::new_window(REPLICATOR_WINDOW_SIZE);
        let shared_window = Arc::new(RwLock::new(window));

        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(node.info)));

        let leader_info = network_addr.map(|i| NodeInfo::new_entry_point(&i));
        let leader_pubkey;
        if let Some(leader_info) = leader_info {
            leader_pubkey = leader_info.id;
            cluster_info.write().unwrap().insert_info(leader_info);
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
            db_ledger,
            cluster_info.clone(),
            0,
            entry_height,
            max_entry_height,
            blob_fetch_receiver,
            entry_window_sender,
            retransmit_sender,
            repair_socket,
            Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
                leader_pubkey,
            ))),
            done,
        );

        let store_ledger_stage = StoreLedgerStage::new(entry_window_receiver, ledger_path);

        let ncp = Ncp::new(
            &cluster_info,
            shared_window.clone(),
            ledger_path,
            node.sockets.gossip,
            exit.clone(),
        );

        let leader =
            poll_gossip_for_leader(network_addr.unwrap(), Some(10)).expect("couldn't reach leader");

        (
            Replicator {
                ncp,
                fetch_stage,
                store_ledger_stage,
                t_window,
                retransmit_receiver,
            },
            leader,
        )
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
    use cluster_info::Node;
    use db_ledger::DbLedger;
    use fullnode::Fullnode;
    use leader_scheduler::LeaderScheduler;
    use ledger::{create_tmp_genesis, get_tmp_ledger_path, read_ledger};
    use logger;
    use replicator::sample_file;
    use replicator::Replicator;
    use rocksdb::{Options, DB};
    use signature::{Keypair, KeypairUtil};
    use solana_sdk::hash::Hash;
    use std::fs::File;
    use std::fs::{create_dir_all, remove_dir_all, remove_file};
    use std::io::Write;
    use std::mem::size_of;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_replicator_startup() {
        logger::setup();
        info!("starting replicator test");
        let entry_height = 0;
        let replicator_ledger_path = &get_tmp_ledger_path("replicator_test_replicator_ledger");

        let exit = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));

        info!("starting leader node");
        let leader_keypair = Arc::new(Keypair::new());
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let network_addr = leader_node.sockets.gossip.local_addr().unwrap();
        let leader_info = leader_node.info.clone();
        let vote_account_keypair = Arc::new(Keypair::new());

        let leader_ledger_path = "replicator_test_leader_ledger";
        let (mint, leader_ledger_path) =
            create_tmp_genesis(leader_ledger_path, 100, leader_info.id, 1);

        let leader = Fullnode::new(
            leader_node,
            &leader_ledger_path,
            leader_keypair,
            vote_account_keypair,
            None,
            false,
            LeaderScheduler::from_bootstrap_leader(leader_info.id),
            None,
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
        let db_ledger_path = get_tmp_ledger_path("test_replicator_startup");
        let db_ledger = Arc::new(RwLock::new(
            DbLedger::open(&db_ledger_path).expect("Expected to be able to open database ledger"),
        ));
        let (replicator, _leader_info) = Replicator::new(
            db_ledger,
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

        DB::destroy(&Options::default(), &db_ledger_path)
            .expect("Expected successful database destuction");
        let _ignored = remove_dir_all(&db_ledger_path);
        let _ignored = remove_dir_all(&leader_ledger_path);
        let _ignored = remove_dir_all(&replicator_ledger_path);
    }

    fn tmp_file_path(name: &str) -> PathBuf {
        use std::env;
        let out_dir = env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let keypair = Keypair::new();

        let mut path = PathBuf::new();
        path.push(out_dir);
        path.push("tmp");
        create_dir_all(&path).unwrap();

        path.push(format!("{}-{}", name, keypair.pubkey()));
        path
    }

    #[test]
    fn test_sample_file() {
        logger::setup();
        let in_path = tmp_file_path("test_sample_file_input.txt");
        let num_strings = 4096;
        let string = "12foobar";
        {
            let mut in_file = File::create(&in_path).unwrap();
            for _ in 0..num_strings {
                in_file.write(string.as_bytes()).unwrap();
            }
        }
        let num_samples = (string.len() * num_strings / size_of::<Hash>()) as u64;
        let samples: Vec<_> = (0..num_samples).collect();
        let res = sample_file(&in_path, samples.as_slice());
        assert!(res.is_ok());
        let ref_hash: Hash = Hash::new(&[
            173, 251, 182, 165, 10, 54, 33, 150, 133, 226, 106, 150, 99, 192, 179, 1, 230, 144,
            151, 126, 18, 191, 54, 67, 249, 140, 230, 160, 56, 30, 170, 52,
        ]);
        let res = res.unwrap();
        assert_eq!(res, ref_hash);

        // Sample just past the end
        assert!(sample_file(&in_path, &[num_samples]).is_err());
        remove_file(&in_path).unwrap();
    }

    #[test]
    fn test_sample_file_invalid_offset() {
        let in_path = tmp_file_path("test_sample_file_invalid_offset_input.txt");
        {
            let mut in_file = File::create(&in_path).unwrap();
            for _ in 0..4096 {
                in_file.write("123456foobar".as_bytes()).unwrap();
            }
        }
        let samples = [0, 200000];
        let res = sample_file(&in_path, &samples);
        assert!(res.is_err());
        remove_file(in_path).unwrap();
    }

    #[test]
    fn test_sample_file_missing_file() {
        let in_path = tmp_file_path("test_sample_file_that_doesnt_exist.txt");
        let samples = [0, 5];
        let res = sample_file(&in_path, &samples);
        assert!(res.is_err());
    }

}
