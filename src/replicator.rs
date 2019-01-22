use crate::blob_fetch_stage::BlobFetchStage;
#[cfg(feature = "chacha")]
use crate::chacha::{chacha_cbc_encrypt_ledger, CHACHA_BLOCK_SIZE};
use crate::client::mk_client;
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::db_ledger::DbLedger;
use crate::gossip_service::GossipService;
use crate::leader_scheduler::LeaderScheduler;
use crate::result::{self, Result};
use crate::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use crate::service::Service;
use crate::storage_stage::{get_segment_from_entry, ENTRIES_PER_SEGMENT};
use crate::streamer::BlobReceiver;
use crate::thin_client::{retry_get_balance, ThinClient};
use crate::window_service::window_service;
use rand::thread_rng;
use rand::Rng;
use solana_drone::drone::{request_airdrop_transaction, DRONE_PORT};
use solana_sdk::hash::{Hash, Hasher};
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
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
use std::time::{Duration, Instant};

pub struct Replicator {
    gossip_service: GossipService,
    fetch_stage: BlobFetchStage,
    t_window: JoinHandle<()>,
    pub retransmit_receiver: BlobReceiver,
    exit: Arc<AtomicBool>,
    entry_height: u64,
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

fn get_entry_heights_from_last_id(
    signature: &ring::signature::Signature,
    storage_entry_height: u64,
) -> (u64, u64) {
    let signature_vec = signature.as_ref();
    let mut segment_index = u64::from(signature_vec[0])
        | (u64::from(signature_vec[1]) << 8)
        | (u64::from(signature_vec[1]) << 16)
        | (u64::from(signature_vec[2]) << 24);
    let max_segment_index = get_segment_from_entry(storage_entry_height);
    segment_index %= max_segment_index as u64;
    let entry_height = segment_index * ENTRIES_PER_SEGMENT;
    let max_entry_height = entry_height + ENTRIES_PER_SEGMENT;

    (entry_height, max_entry_height)
}

impl Replicator {
    /// Returns a Result that contains a replicator on success
    ///
    /// # Arguments
    /// * `ledger_path` - (Not actually optional) path to where the ledger will be stored.
    /// Causes panic if none
    /// * `node` - The replicator node
    /// * `leader_info` - NodeInfo representing the leader
    /// * `keypair` - Keypair for this replicator
    /// * `timeout` - (optional) timeout for polling for leader/downloading the ledger. Defaults to
    /// 30 seconds
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_path: Option<&str>,
        node: Node,
        leader_info: &NodeInfo,
        keypair: &Keypair,
        timeout: Option<Duration>,
    ) -> Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let timeout = timeout.unwrap_or_else(|| Duration::new(30, 0));

        info!("Replicator: id: {}", keypair.pubkey());
        info!("Creating cluster info....");
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new(node.info.clone())));

        let leader_pubkey = leader_info.id;
        {
            let mut cluster_info_w = cluster_info.write().unwrap();
            cluster_info_w.insert_info(leader_info.clone());
            cluster_info_w.set_leader(leader_info.id);
        }

        // Create DbLedger, eventually will simply repurpose the input
        // ledger path as the DbLedger path once we replace the ledger with
        // DbLedger. Note for now, this ledger will not contain any of the existing entries
        // in the ledger located at ledger_path, and will only append on newly received
        // entries after being passed to window_service
        let db_ledger = Arc::new(
            DbLedger::open(&ledger_path.unwrap())
                .expect("Expected to be able to open database ledger"),
        );

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(db_ledger.clone()),
            node.sockets.gossip,
            exit.clone(),
        );

        info!("polling for leader");
        let leader = Self::poll_for_leader(&cluster_info, timeout)?;

        info!("Got leader: {:?}", leader);

        let (storage_last_id, storage_entry_height) =
            Self::poll_for_last_id_and_entry_height(&cluster_info)?;

        let signature = keypair.sign(storage_last_id.as_ref());
        let (entry_height, max_entry_height) =
            get_entry_heights_from_last_id(&signature, storage_entry_height);

        info!("replicating entry_height: {}", entry_height);

        let repair_socket = Arc::new(node.sockets.repair);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            node.sockets.tvu.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (fetch_stage, blob_fetch_receiver) =
            BlobFetchStage::new_multi_socket(blob_sockets, exit.clone());

        // todo: pull blobs off the retransmit_receiver and recycle them?
        let (retransmit_sender, retransmit_receiver) = channel();

        let (entry_sender, entry_receiver) = channel();

        let t_window = window_service(
            db_ledger.clone(),
            cluster_info.clone(),
            0,
            entry_height,
            max_entry_height,
            blob_fetch_receiver,
            Some(entry_sender),
            retransmit_sender,
            repair_socket,
            Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(
                leader_pubkey,
            ))),
            done.clone(),
        );

        info!("window created, waiting for ledger download done");
        let start = Instant::now();
        let mut received_so_far = 0;

        while !done.load(Ordering::Relaxed) {
            sleep(Duration::from_millis(100));

            let elapsed = start.elapsed();
            received_so_far += entry_receiver.try_recv().map(|v| v.len()).unwrap_or(0);

            if received_so_far == 0 && elapsed > timeout {
                return Err(result::Error::IO(io::Error::new(
                    ErrorKind::TimedOut,
                    "Timed out waiting to receive any blocks",
                )));
            }
        }

        info!("Done receiving entries from window_service");

        let mut node_info = node.info.clone();
        node_info.tvu = "0.0.0.0:0".parse().unwrap();
        {
            let mut cluster_info_w = cluster_info.write().unwrap();
            cluster_info_w.insert_info(node_info);
        }

        let mut client = mk_client(&leader);

        Self::get_airdrop_tokens(&mut client, keypair, &leader_info);
        info!("Done downloading ledger at {}", ledger_path.unwrap());

        let ledger_path = Path::new(ledger_path.unwrap());
        let ledger_data_file_encrypted = ledger_path.join("ledger.enc");
        let mut sampling_offsets = Vec::new();

        #[cfg(not(feature = "chacha"))]
        sampling_offsets.push(0);

        #[cfg(feature = "chacha")]
        {
            use crate::storage_stage::NUM_STORAGE_SAMPLES;
            use rand::{Rng, SeedableRng};
            use rand_chacha::ChaChaRng;

            let mut ivec = [0u8; 64];
            ivec.copy_from_slice(signature.as_ref());

            let num_encrypted_bytes = chacha_cbc_encrypt_ledger(
                &db_ledger,
                entry_height,
                &ledger_data_file_encrypted,
                &mut ivec,
            )?;

            let num_chacha_blocks = num_encrypted_bytes / CHACHA_BLOCK_SIZE;
            let mut rng_seed = [0u8; 32];
            rng_seed.copy_from_slice(&signature.as_ref()[0..32]);
            let mut rng = ChaChaRng::from_seed(rng_seed);
            for _ in 0..NUM_STORAGE_SAMPLES {
                sampling_offsets.push(rng.gen_range(0, num_chacha_blocks) as u64);
            }
        }

        info!("Done encrypting the ledger");

        match sample_file(&ledger_data_file_encrypted, &sampling_offsets) {
            Ok(hash) => {
                let last_id = client.get_last_id();
                info!("sampled hash: {}", hash);
                let mut tx = Transaction::storage_new_mining_proof(
                    &keypair,
                    hash,
                    last_id,
                    entry_height,
                    Signature::new(signature.as_ref()),
                );
                client
                    .retry_transfer(&keypair, &mut tx, 10)
                    .expect("transfer didn't work!");
            }
            Err(e) => info!("Error occurred while sampling: {:?}", e),
        }

        Ok(Self {
            gossip_service,
            fetch_stage,
            t_window,
            retransmit_receiver,
            exit,
            entry_height,
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

        // Drain the queue here to prevent self.retransmit_receiver from being dropped
        // before the window_service thread is joined
        let mut retransmit_queue_count = 0;
        while let Ok(_blob) = self.retransmit_receiver.recv_timeout(Duration::new(1, 0)) {
            retransmit_queue_count += 1;
        }
        debug!("retransmit channel count: {}", retransmit_queue_count);
    }

    pub fn entry_height(&self) -> u64 {
        self.entry_height
    }

    fn poll_for_leader(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        timeout: Duration,
    ) -> Result<NodeInfo> {
        let start = Instant::now();
        loop {
            if let Some(l) = cluster_info.read().unwrap().get_gossip_top_leader() {
                return Ok(l.clone());
            }

            let elapsed = start.elapsed();
            if elapsed > timeout {
                return Err(result::Error::IO(io::Error::new(
                    ErrorKind::TimedOut,
                    "Timed out waiting to receive any blocks",
                )));
            }

            sleep(Duration::from_millis(900));
            info!("{}", cluster_info.read().unwrap().node_info_trace());
        }
    }

    fn poll_for_last_id_and_entry_height(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Result<(String, u64)> {
        for _ in 0..10 {
            let rpc_client = {
                let cluster_info = cluster_info.read().unwrap();
                let rpc_peers = cluster_info.rpc_peers();
                debug!("rpc peers: {:?}", rpc_peers);
                let node_idx = thread_rng().gen_range(0, rpc_peers.len());
                RpcClient::new_from_socket(rpc_peers[node_idx].rpc)
            };

            let storage_last_id = rpc_client
                .make_rpc_request(2, RpcRequest::GetStorageMiningLastId, None)
                .expect("rpc request")
                .to_string();
            let storage_entry_height = rpc_client
                .make_rpc_request(2, RpcRequest::GetStorageMiningEntryHeight, None)
                .expect("rpc request")
                .as_u64()
                .unwrap();
            if get_segment_from_entry(storage_entry_height) != 0 {
                return Ok((storage_last_id, storage_entry_height));
            }
            info!("max entry_height: {}", storage_entry_height);
            sleep(Duration::from_secs(3));
        }
        Err(Error::new(
            ErrorKind::Other,
            "Couldn't get last_id or entry_height",
        ))?
    }

    fn get_airdrop_tokens(client: &mut ThinClient, keypair: &Keypair, leader_info: &NodeInfo) {
        if retry_get_balance(client, &keypair.pubkey(), None).is_none() {
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
    }
}

#[cfg(test)]
mod tests {
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
        solana_logger::setup();
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
