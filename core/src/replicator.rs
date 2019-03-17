use crate::blob_fetch_stage::BlobFetchStage;
use crate::blocktree::Blocktree;
#[cfg(feature = "chacha")]
use crate::chacha::{chacha_cbc_encrypt_ledger, CHACHA_BLOCK_SIZE};
use crate::cluster_info::{ClusterInfo, Node, FULLNODE_PORT_RANGE};
use crate::contact_info::ContactInfo;
use crate::gossip_service::GossipService;
use crate::repair_service::RepairSlotRange;
use crate::result::Result;
use crate::service::Service;
use crate::storage_stage::{get_segment_from_entry, ENTRIES_PER_SEGMENT};
use crate::window_service::WindowService;
use rand::thread_rng;
use rand::Rng;
use solana_client::rpc_request::{RpcClient, RpcRequest};
use solana_client::thin_client::{create_client, retry_get_balance, ThinClient};
use solana_drone::drone::{request_airdrop_transaction, DRONE_PORT};
use solana_sdk::hash::{Hash, Hasher};
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_storage_api::StorageTransaction;
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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::spawn;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct Replicator {
    gossip_service: GossipService,
    fetch_stage: BlobFetchStage,
    window_service: WindowService,
    t_retransmit: JoinHandle<()>,
    exit: Arc<AtomicBool>,
    slot: u64,
    ledger_path: String,
    keypair: Arc<Keypair>,
    signature: ring::signature::Signature,
    cluster_entrypoint: ContactInfo,
    node_info: ContactInfo,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    ledger_data_file_encrypted: PathBuf,
    sampling_offsets: Vec<u64>,
    hash: Hash,
    blocktree: Arc<Blocktree>,
    #[cfg(feature = "chacha")]
    num_chacha_blocks: usize,
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

fn get_entry_heights_from_blockhash(
    signature: &ring::signature::Signature,
    storage_entry_height: u64,
) -> u64 {
    let signature_vec = signature.as_ref();
    let mut segment_index = u64::from(signature_vec[0])
        | (u64::from(signature_vec[1]) << 8)
        | (u64::from(signature_vec[1]) << 16)
        | (u64::from(signature_vec[2]) << 24);
    let max_segment_index = get_segment_from_entry(storage_entry_height);
    segment_index %= max_segment_index as u64;
    segment_index * ENTRIES_PER_SEGMENT
}

impl Replicator {
    /// Returns a Result that contains a replicator on success
    ///
    /// # Arguments
    /// * `ledger_path` - path to where the ledger will be stored.
    /// Causes panic if none
    /// * `node` - The replicator node
    /// * `cluster_entrypoint` - ContactInfo representing an entry into the network
    /// * `keypair` - Keypair for this replicator
    /// * `timeout` - (optional) timeout for polling for leader/downloading the ledger. Defaults to
    /// 30 seconds
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_path: &str,
        node: Node,
        cluster_entrypoint: ContactInfo,
        keypair: Arc<Keypair>,
        _timeout: Option<Duration>,
    ) -> Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));

        info!("Replicator: id: {}", keypair.pubkey());
        info!("Creating cluster info....");
        let mut cluster_info = ClusterInfo::new(node.info.clone(), keypair.clone());
        cluster_info.set_entrypoint(cluster_entrypoint.clone());
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        // Create Blocktree, eventually will simply repurpose the input
        // ledger path as the Blocktree path once we replace the ledger with
        // Blocktree. Note for now, this ledger will not contain any of the existing entries
        // in the ledger located at ledger_path, and will only append on newly received
        // entries after being passed to window_service
        let blocktree =
            Blocktree::open(ledger_path).expect("Expected to be able to open database ledger");

        let blocktree = Arc::new(blocktree);

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(blocktree.clone()),
            None,
            node.sockets.gossip,
            &exit,
        );

        info!("Looking for leader at {:?}", cluster_entrypoint);
        crate::gossip_service::discover(&cluster_entrypoint.gossip, 1)?;

        let (storage_blockhash, storage_entry_height) =
            Self::poll_for_blockhash_and_entry_height(&cluster_info)?;

        let node_info = node.info.clone();
        let signature = keypair.sign(storage_blockhash.as_ref());
        let slot = get_entry_heights_from_blockhash(&signature, storage_entry_height);
        info!("replicating slot: {}", slot);

        let mut repair_slot_range = RepairSlotRange::default();
        repair_slot_range.end = slot + ENTRIES_PER_SEGMENT;
        repair_slot_range.start = slot;

        let repair_socket = Arc::new(node.sockets.repair);
        let mut blob_sockets: Vec<Arc<UdpSocket>> =
            node.sockets.tvu.into_iter().map(Arc::new).collect();
        blob_sockets.push(repair_socket.clone());
        let (blob_fetch_sender, blob_fetch_receiver) = channel();
        let fetch_stage = BlobFetchStage::new_multi_socket(blob_sockets, &blob_fetch_sender, &exit);

        let (retransmit_sender, retransmit_receiver) = channel();

        let window_service = WindowService::new(
            blocktree.clone(),
            cluster_info.clone(),
            blob_fetch_receiver,
            retransmit_sender,
            repair_socket,
            &exit,
            repair_slot_range,
        );

        // receive blobs from retransmit and drop them.
        let exit2 = exit.clone();
        let t_retransmit = spawn(move || loop {
            let _ = retransmit_receiver.recv_timeout(Duration::from_secs(1));
            if exit2.load(Ordering::Relaxed) {
                break;
            }
        });

        Ok(Self {
            gossip_service,
            fetch_stage,
            window_service,
            t_retransmit,
            exit,
            slot,
            ledger_path: ledger_path.to_string(),
            keypair: keypair.clone(),
            signature,
            cluster_entrypoint,
            node_info,
            cluster_info,
            ledger_data_file_encrypted: PathBuf::default(),
            sampling_offsets: vec![],
            hash: Hash::default(),
            blocktree,
            #[cfg(feature = "chacha")]
            num_chacha_blocks: 0,
        })
    }

    pub fn run(&mut self) {
        self.wait_for_ledger_download();
        self.encrypt_ledger()
            .expect("ledger encrypt not successful");
        loop {
            self.create_sampling_offsets();
            if self.sample_file_to_create_mining_hash().is_err() {
                info!("Error sampling file, exiting...");
                break;
            }
            self.submit_mining_proof();
        }
    }

    fn wait_for_ledger_download(&self) {
        info!("window created, waiting for ledger download done");
        let _start = Instant::now();
        let mut _received_so_far = 0;

        loop {
            if let Ok(entries) = self.blocktree.get_slot_entries(self.slot, 0, None) {
                if !entries.is_empty() {
                    break;
                }
            }
            sleep(Duration::from_secs(1));
        }

        info!("Done receiving entries from window_service");

        let mut contact_info = self.node_info.clone();
        contact_info.tvu = "0.0.0.0:0".parse().unwrap();
        {
            let mut cluster_info_w = self.cluster_info.write().unwrap();
            cluster_info_w.insert_self(contact_info);
        }

        info!("Done downloading ledger at {}", self.ledger_path);
    }

    fn encrypt_ledger(&mut self) -> Result<()> {
        let ledger_path = Path::new(&self.ledger_path);
        self.ledger_data_file_encrypted = ledger_path.join("ledger.enc");

        #[cfg(feature = "chacha")]
        {
            let mut ivec = [0u8; 64];
            ivec.copy_from_slice(self.signature.as_ref());

            let num_encrypted_bytes = chacha_cbc_encrypt_ledger(
                &self.blocktree,
                self.slot,
                &self.ledger_data_file_encrypted,
                &mut ivec,
            )?;

            self.num_chacha_blocks = num_encrypted_bytes / CHACHA_BLOCK_SIZE;
        }

        info!("Done encrypting the ledger");
        Ok(())
    }

    fn create_sampling_offsets(&mut self) {
        self.sampling_offsets.clear();

        #[cfg(not(feature = "chacha"))]
        self.sampling_offsets.push(0);

        #[cfg(feature = "chacha")]
        {
            use crate::storage_stage::NUM_STORAGE_SAMPLES;
            use rand::{Rng, SeedableRng};
            use rand_chacha::ChaChaRng;

            let mut rng_seed = [0u8; 32];
            rng_seed.copy_from_slice(&self.signature.as_ref()[0..32]);
            let mut rng = ChaChaRng::from_seed(rng_seed);
            for _ in 0..NUM_STORAGE_SAMPLES {
                self.sampling_offsets
                    .push(rng.gen_range(0, self.num_chacha_blocks) as u64);
            }
        }
    }

    fn sample_file_to_create_mining_hash(&mut self) -> Result<()> {
        self.hash = sample_file(&self.ledger_data_file_encrypted, &self.sampling_offsets)?;
        info!("sampled hash: {}", self.hash);
        Ok(())
    }

    fn submit_mining_proof(&self) {
        let client = create_client(
            self.cluster_entrypoint.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );
        Self::get_airdrop_lamports(&client, &self.keypair, &self.cluster_entrypoint);

        let blockhash = client.get_recent_blockhash();
        let mut tx = StorageTransaction::new_mining_proof(
            &self.keypair,
            self.hash,
            blockhash,
            self.slot,
            Signature::new(self.signature.as_ref()),
        );
        client
            .retry_transfer(&self.keypair, &mut tx, 10)
            .expect("transfer didn't work!");
    }

    pub fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.join()
    }

    pub fn join(self) {
        self.gossip_service.join().unwrap();
        self.fetch_stage.join().unwrap();
        self.window_service.join().unwrap();
        self.t_retransmit.join().unwrap();
    }

    pub fn entry_height(&self) -> u64 {
        self.slot
    }

    fn poll_for_blockhash_and_entry_height(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Result<(String, u64)> {
        for _ in 0..10 {
            let rpc_client = {
                let cluster_info = cluster_info.read().unwrap();
                let rpc_peers = cluster_info.rpc_peers();
                debug!("rpc peers: {:?}", rpc_peers);
                let node_idx = thread_rng().gen_range(0, rpc_peers.len());
                RpcClient::new_socket(rpc_peers[node_idx].rpc)
            };
            let storage_blockhash = rpc_client
                .retry_make_rpc_request(&RpcRequest::GetStorageBlockhash, None, 0)
                .expect("rpc request")
                .to_string();
            let storage_entry_height = rpc_client
                .retry_make_rpc_request(&RpcRequest::GetStorageEntryHeight, None, 0)
                .expect("rpc request")
                .as_u64()
                .unwrap();
            info!("max entry_height: {}", storage_entry_height);
            if get_segment_from_entry(storage_entry_height) != 0 {
                return Ok((storage_blockhash, storage_entry_height));
            }
            sleep(Duration::from_secs(3));
        }
        Err(Error::new(
            ErrorKind::Other,
            "Couldn't get blockhash or entry_height",
        ))?
    }

    fn get_airdrop_lamports(
        client: &ThinClient,
        keypair: &Keypair,
        cluster_entrypoint: &ContactInfo,
    ) {
        if retry_get_balance(client, &keypair.pubkey(), None).is_none() {
            let mut drone_addr = cluster_entrypoint.tpu;
            drone_addr.set_port(DRONE_PORT);

            let airdrop_amount = 1;

            let blockhash = client.get_recent_blockhash();
            match request_airdrop_transaction(
                &drone_addr,
                &keypair.pubkey(),
                airdrop_amount,
                blockhash,
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
