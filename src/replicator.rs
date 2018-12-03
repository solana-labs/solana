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

        // Create the RocksDb ledger, eventually will simply repurpose the input
        // ledger path as the RocksDb ledger path once we replace the ledger with
        // RocksDb. Note for now, this ledger will not contain any of the existing entries
        // in the ledger located at ledger_path, and will only append on newly received
        // entries after being passed to window_service
        let db_ledger = Arc::new(RwLock::new(
            DbLedger::open(&ledger_path.unwrap())
                .expect("Expected to be able to open database ledger"),
        ));

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
    use logger;
    use replicator::sample_file;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::fs::File;
    use std::fs::{create_dir_all, remove_file};
    use std::io::Write;
    use std::mem::size_of;
    use std::path::PathBuf;

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
