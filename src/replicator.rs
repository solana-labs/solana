use crate::blob_fetch_stage::BlobFetchStage;
#[cfg(feature = "chacha")]
use crate::chacha::{chacha_cbc_encrypt_file, CHACHA_BLOCK_SIZE};
use crate::client::mk_client;
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::db_ledger::DbLedger;
use crate::gossip_service::GossipService;
use crate::leader_scheduler::LeaderScheduler;
use crate::ledger::LEDGER_DATA_FILE;
use crate::result::Result;
use crate::rpc_request::{RpcClient, RpcRequest};
use crate::service::Service;
use crate::store_ledger_stage::StoreLedgerStage;
use crate::streamer::BlobReceiver;
use crate::thin_client::retry_get_balance;
use crate::window;
use crate::window_service::window_service;
use rand::thread_rng;
use rand::Rng;
use solana_drone::drone::{request_airdrop_transaction, DRONE_PORT};
use solana_sdk::hash::{Hash, Hasher};
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::storage_program::StorageTransaction;
use solana_sdk::transaction::Transaction;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::{Error, ErrorKind};
use std::mem::size_of;
use std::net::UdpSocket;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::JoinHandle;
use std::time::Duration;

pub struct Replicator {
    gossip_service: GossipService,
    fetch_stage: BlobFetchStage,
    store_ledger_stage: StoreLedgerStage,
    t_window: JoinHandle<()>,
    pub retransmit_receiver: BlobReceiver,
    exit: Arc<AtomicBool>,
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
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_path: Option<&str>,
        node: Node,
        leader_info: &NodeInfo,
        keypair: &Keypair,
    ) -> Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));

        let entry_height = 0;
        let max_entry_height = 1;

        const REPLICATOR_WINDOW_SIZE: usize = 32 * 1024;
        let window = window::new_window(REPLICATOR_WINDOW_SIZE);
        let shared_window = Arc::new(RwLock::new(window));

        info!("Replicator: id: {}", keypair.pubkey());
        info!("Creating cluster info....");
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(node.info)));

        let leader_pubkey = leader_info.id;
        {
            let mut cluster_info_w = cluster_info.write().unwrap();
            cluster_info_w.insert_info(leader_info.clone());
            cluster_info_w.set_leader(leader_info.id);
        }

        let (entry_window_sender, entry_window_receiver) = channel();
        let store_ledger_stage = StoreLedgerStage::new(entry_window_receiver, ledger_path);

        // Create the RocksDb ledger, eventually will simply repurpose the input
        // ledger path as the RocksDb ledger path once we replace the ledger with
        // RocksDb. Note for now, this ledger will not contain any of the existing entries
        // in the ledger located at ledger_path, and will only append on newly received
        // entries after being passed to window_service
        let db_ledger = Arc::new(RwLock::new(
            DbLedger::open(&ledger_path.unwrap())
                .expect("Expected to be able to open database ledger"),
        ));

        let gossip_service = GossipService::new(
            &cluster_info,
            shared_window.clone(),
            ledger_path,
            node.sockets.gossip,
            exit.clone(),
        );

        info!("polling for leader");
        let leader;
        loop {
            if let Some(l) = cluster_info.read().unwrap().get_gossip_top_leader() {
                leader = l.clone();
                break;
            }

            sleep(Duration::from_millis(900));
            info!("{}", cluster_info.read().unwrap().node_info_trace());
        }

        info!("Got leader: {:?}", leader);

        let rpc_client = {
            let cluster_info = cluster_info.read().unwrap();
            let rpc_peers = cluster_info.rpc_peers();
            info!("rpc peers: {:?}", rpc_peers);
            let node_idx = thread_rng().gen_range(0, rpc_peers.len());
            RpcClient::new_from_socket(rpc_peers[node_idx].rpc)
        };

        let storage_last_id = RpcRequest::GetStorageMiningLastId
            .make_rpc_request(&rpc_client, 2, None)
            .expect("rpc request")
            .to_string();
        let _signature = keypair.sign(storage_last_id.as_ref());
        // TODO: use this signature to pick the key and block

        let repair_socket = Arc::new(node.sockets.repair);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            node.sockets.replicate.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new_multi_socket(blob_sockets, exit.clone());

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
            done.clone(),
        );

        info!("window created, waiting for ledger download done");
        while !done.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(100));
        }

        let mut client = mk_client(&leader);

        if retry_get_balance(&mut client, &keypair.pubkey(), None).is_none() {
            let mut drone_addr = leader_info.tpu;
            drone_addr.set_port(DRONE_PORT);

            let airdrop_amount = 1;

            let last_id = client.get_last_id();
            match request_airdrop_transaction(
                &drone_addr,
                &keypair.pubkey(),
                airdrop_amount,
                last_id,
            ) {
                Ok(transaction) => {
                    let signature = client.transfer_signed(&transaction).unwrap();
                    client.poll_for_signature(&signature).unwrap();
                }
                Err(err) => {
                    panic!(
                        "Error requesting airdrop: {:?} to addr: {:?} amount: {}",
                        err, drone_addr, airdrop_amount
                    );
                }
            };
        }

        info!("Done downloading ledger at {}", ledger_path.unwrap());

        let ledger_path = Path::new(ledger_path.unwrap());
        let ledger_data_file_encrypted = ledger_path.join(format!("{}.enc", LEDGER_DATA_FILE));
        #[cfg(feature = "chacha")]
        {
            let ledger_data_file = ledger_path.join(LEDGER_DATA_FILE);
            let mut ivec = [0u8; CHACHA_BLOCK_SIZE];
            ivec[0..4].copy_from_slice(&[2, 3, 4, 5]);

            chacha_cbc_encrypt_file(&ledger_data_file, &ledger_data_file_encrypted, &mut ivec)?;
        }

        info!("Done encrypting the ledger");

        let sampling_offsets = [0, 1, 2, 3];

        match sample_file(&ledger_data_file_encrypted, &sampling_offsets) {
            Ok(hash) => {
                let last_id = client.get_last_id();
                info!("sampled hash: {}", hash);
                let tx = Transaction::storage_new_mining_proof(&keypair, hash, last_id);
                client.transfer_signed(&tx).expect("transfer didn't work!");
            }
            Err(e) => info!("Error occurred while sampling: {:?}", e),
        }

        Ok(Self {
            gossip_service,
            fetch_stage,
            store_ledger_stage,
            t_window,
            retransmit_receiver,
            exit,
        })
    }

    pub fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.join()
    }

    pub fn join(self) {
        self.gossip_service.join().unwrap();
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
    use crate::logger;
    use crate::replicator::sample_file;
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
