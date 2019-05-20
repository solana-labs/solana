use crate::blob_fetch_stage::BlobFetchStage;
use crate::blocktree::Blocktree;
#[cfg(feature = "chacha")]
use crate::chacha::{chacha_cbc_encrypt_ledger, CHACHA_BLOCK_SIZE};
use crate::cluster_info::{ClusterInfo, Node};
use crate::contact_info::ContactInfo;
use crate::gossip_service::GossipService;
use crate::packet::to_shared_blob;
use crate::repair_service::{RepairSlotRange, RepairStrategy};
use crate::result::Result;
use crate::service::Service;
use crate::streamer::receiver;
use crate::streamer::responder;
use crate::window_service::WindowService;
use bincode::deserialize;
use rand::thread_rng;
use rand::Rng;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_request::RpcRequest;
use solana_client::thin_client::ThinClient;
use solana_ed25519_dalek as ed25519_dalek;
use solana_runtime::bank::Bank;
use solana_sdk::client::{AsyncClient, SyncClient};
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::{Hash, Hasher};
use solana_sdk::message::Message;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::system_transaction;
use solana_sdk::timing::timestamp;
use solana_sdk::transaction::Transaction;
use solana_sdk::transport::TransportError;
use solana_storage_api::{get_segment_from_slot, storage_instruction, SLOTS_PER_SEGMENT};
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::{Error, ErrorKind};
use std::mem::size_of;
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::path::PathBuf;
use std::result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::thread::spawn;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
pub enum ReplicatorRequest {
    GetSlotHeight(SocketAddr),
}

pub struct Replicator {
    gossip_service: GossipService,
    fetch_stage: BlobFetchStage,
    window_service: WindowService,
    thread_handles: Vec<JoinHandle<()>>,
    exit: Arc<AtomicBool>,
    slot: u64,
    ledger_path: String,
    keypair: Arc<Keypair>,
    storage_keypair: Arc<Keypair>,
    signature: ed25519_dalek::Signature,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    ledger_data_file_encrypted: PathBuf,
    sampling_offsets: Vec<u64>,
    hash: Hash,
    #[cfg(feature = "chacha")]
    num_chacha_blocks: usize,
    #[cfg(feature = "chacha")]
    blocktree: Arc<Blocktree>,
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

fn get_slot_from_blockhash(signature: &ed25519_dalek::Signature, storage_slot: u64) -> u64 {
    let signature_vec = signature.to_bytes();
    let mut segment_index = u64::from(signature_vec[0])
        | (u64::from(signature_vec[1]) << 8)
        | (u64::from(signature_vec[1]) << 16)
        | (u64::from(signature_vec[2]) << 24);
    let max_segment_index = get_segment_from_slot(storage_slot);
    segment_index %= max_segment_index as u64;
    segment_index * SLOTS_PER_SEGMENT
}

fn create_request_processor(
    socket: UdpSocket,
    exit: &Arc<AtomicBool>,
    slot: u64,
) -> Vec<JoinHandle<()>> {
    let mut thread_handles = vec![];
    let (s_reader, r_reader) = channel();
    let (s_responder, r_responder) = channel();
    let storage_socket = Arc::new(socket);
    let t_receiver = receiver(storage_socket.clone(), exit, s_reader);
    thread_handles.push(t_receiver);

    let t_responder = responder("replicator-responder", storage_socket.clone(), r_responder);
    thread_handles.push(t_responder);

    let exit = exit.clone();
    let t_processor = spawn(move || loop {
        let packets = r_reader.recv_timeout(Duration::from_secs(1));
        if let Ok(packets) = packets {
            for packet in &packets.packets {
                let req: result::Result<ReplicatorRequest, Box<bincode::ErrorKind>> =
                    deserialize(&packet.data[..packet.meta.size]);
                match req {
                    Ok(ReplicatorRequest::GetSlotHeight(from)) => {
                        if let Ok(blob) = to_shared_blob(slot, from) {
                            let _ = s_responder.send(vec![blob]);
                        }
                    }
                    Err(e) => {
                        info!("invalid request: {:?}", e);
                    }
                }
            }
        }
        if exit.load(Ordering::Relaxed) {
            break;
        }
    });
    thread_handles.push(t_processor);
    thread_handles
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
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_path: &str,
        node: Node,
        cluster_entrypoint: ContactInfo,
        keypair: Arc<Keypair>,
        storage_keypair: Arc<Keypair>,
    ) -> Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));

        info!("Replicator: id: {}", keypair.pubkey());
        info!("Creating cluster info....");
        let mut cluster_info = ClusterInfo::new(node.info.clone(), keypair.clone());
        cluster_info.set_entrypoint(cluster_entrypoint.clone());
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        // Note for now, this ledger will not contain any of the existing entries
        // in the ledger located at ledger_path, and will only append on newly received
        // entries after being passed to window_service
        let genesis_block =
            GenesisBlock::load(ledger_path).expect("Expected to successfully open genesis block");
        let bank = Bank::new_with_paths(&genesis_block, None);
        let genesis_blockhash = bank.last_blockhash();
        let blocktree = Arc::new(
            Blocktree::open(ledger_path).expect("Expected to be able to open database ledger"),
        );

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(blocktree.clone()),
            None,
            node.sockets.gossip,
            &exit,
        );

        info!("Connecting to the cluster via {:?}", cluster_entrypoint);
        let (nodes, _) = crate::gossip_service::discover_cluster(&cluster_entrypoint.gossip, 1)?;
        let client = crate::gossip_service::get_client(&nodes);

        let (storage_blockhash, storage_slot) = Self::poll_for_blockhash_and_slot(&cluster_info)?;

        let node_info = node.info.clone();
        let signature = storage_keypair.sign(storage_blockhash.as_ref());
        let slot = get_slot_from_blockhash(&signature, storage_slot);
        info!("replicating slot: {}", slot);

        let mut repair_slot_range = RepairSlotRange::default();
        repair_slot_range.end = slot + SLOTS_PER_SEGMENT;
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
            RepairStrategy::RepairRange(repair_slot_range),
            &genesis_blockhash,
            |_, _, _| true,
        );

        Self::setup_mining_account(&client, &keypair, &storage_keypair)?;
        let mut thread_handles =
            create_request_processor(node.sockets.storage.unwrap(), &exit, slot);

        // receive blobs from retransmit and drop them.
        let t_retransmit = {
            let exit = exit.clone();
            spawn(move || loop {
                let _ = retransmit_receiver.recv_timeout(Duration::from_secs(1));
                if exit.load(Ordering::Relaxed) {
                    break;
                }
            })
        };
        thread_handles.push(t_retransmit);

        let t_replicate = {
            let exit = exit.clone();
            let blocktree = blocktree.clone();
            let cluster_info = cluster_info.clone();
            spawn(move || {
                Self::wait_for_ledger_download(slot, &blocktree, &exit, &node_info, cluster_info)
            })
        };
        //always push this last
        thread_handles.push(t_replicate);

        Ok(Self {
            gossip_service,
            fetch_stage,
            window_service,
            thread_handles,
            exit,
            slot,
            ledger_path: ledger_path.to_string(),
            keypair,
            storage_keypair,
            signature,
            cluster_info,
            ledger_data_file_encrypted: PathBuf::default(),
            sampling_offsets: vec![],
            hash: Hash::default(),
            #[cfg(feature = "chacha")]
            num_chacha_blocks: 0,
            #[cfg(feature = "chacha")]
            blocktree,
        })
    }

    pub fn run(&mut self) {
        info!("waiting for ledger download");
        self.thread_handles.pop().unwrap().join().unwrap();
        self.encrypt_ledger()
            .expect("ledger encrypt not successful");
        loop {
            self.create_sampling_offsets();
            if self.sample_file_to_create_mining_hash().is_err() {
                info!("Error sampling file, exiting...");
                break;
            }
            self.submit_mining_proof();
            // TODO: Replicators should be submitting proofs as fast as possible
            sleep(Duration::from_secs(2));
        }
    }

    fn wait_for_ledger_download(
        start_slot: u64,
        blocktree: &Arc<Blocktree>,
        exit: &Arc<AtomicBool>,
        node_info: &ContactInfo,
        cluster_info: Arc<RwLock<ClusterInfo>>,
    ) {
        info!(
            "window created, waiting for ledger download starting at slot {:?}",
            start_slot
        );
        let mut current_slot = start_slot;
        'outer: loop {
            while let Ok(meta) = blocktree.meta(current_slot) {
                if let Some(meta) = meta {
                    if meta.is_full() {
                        current_slot += 1;
                        info!("current slot: {}", current_slot);
                        if current_slot >= start_slot + SLOTS_PER_SEGMENT {
                            break 'outer;
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            if exit.load(Ordering::Relaxed) {
                break;
            }
            sleep(Duration::from_secs(1));
        }

        info!("Done receiving entries from window_service");

        // Remove replicator from the data plane
        let mut contact_info = node_info.clone();
        contact_info.tvu = "0.0.0.0:0".parse().unwrap();
        contact_info.wallclock = timestamp();
        {
            let mut cluster_info_w = cluster_info.write().unwrap();
            cluster_info_w.insert_self(contact_info);
        }
    }

    fn encrypt_ledger(&mut self) -> Result<()> {
        let ledger_path = Path::new(&self.ledger_path);
        self.ledger_data_file_encrypted = ledger_path.join("ledger.enc");

        #[cfg(feature = "chacha")]
        {
            let mut ivec = [0u8; 64];
            ivec.copy_from_slice(&self.signature.to_bytes());

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
            rng_seed.copy_from_slice(&self.signature.to_bytes()[0..32]);
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

    fn setup_mining_account(
        client: &ThinClient,
        keypair: &Keypair,
        storage_keypair: &Keypair,
    ) -> Result<()> {
        // make sure replicator has some balance
        if client.poll_get_balance(&keypair.pubkey())? == 0 {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "No account has been setup",
            ))?
        }

        // check if the account exists
        let bal = client.poll_get_balance(&storage_keypair.pubkey());
        if bal.is_err() || bal.unwrap() == 0 {
            let (blockhash, _fee_calculator) = client.get_recent_blockhash().expect("blockhash");
            let tx = system_transaction::create_account(
                keypair,
                &storage_keypair.pubkey(),
                blockhash,
                1,
                1024 * 4, // TODO the account space needs to be well defined somewhere
                &solana_storage_api::id(),
                0,
            );
            let signature = client.async_send_transaction(tx)?;
            client
                .poll_for_signature(&signature)
                .map_err(|err| match err {
                    TransportError::IoError(e) => e,
                    TransportError::TransactionError(_) => {
                        io::Error::new(ErrorKind::Other, "signature not found")
                    }
                })?;
        }
        Ok(())
    }

    fn submit_mining_proof(&self) {
        // No point if we've got no storage account...
        let nodes = self.cluster_info.read().unwrap().tvu_peers();
        let client = crate::gossip_service::get_client(&nodes);
        assert!(
            client
                .poll_get_balance(&self.storage_keypair.pubkey())
                .unwrap()
                > 0
        );
        // ...or no lamports for fees
        assert!(client.poll_get_balance(&self.keypair.pubkey()).unwrap() > 0);

        let (blockhash, _) = client.get_recent_blockhash().expect("No recent blockhash");
        let instruction = storage_instruction::mining_proof(
            &self.storage_keypair.pubkey(),
            self.hash,
            self.slot,
            Signature::new(&self.signature.to_bytes()),
        );
        let message = Message::new_with_payer(vec![instruction], Some(&self.keypair.pubkey()));
        let mut transaction = Transaction::new(
            &[self.keypair.as_ref(), self.storage_keypair.as_ref()],
            message,
            blockhash,
        );
        client
            .send_and_confirm_transaction(
                &[&self.keypair, &self.storage_keypair],
                &mut transaction,
                10,
                0,
            )
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
        for handle in self.thread_handles {
            handle.join().unwrap();
        }
    }

    pub fn slot(&self) -> u64 {
        self.slot
    }

    fn poll_for_blockhash_and_slot(
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
            let storage_slot = rpc_client
                .retry_make_rpc_request(&RpcRequest::GetStorageSlot, None, 0)
                .expect("rpc request")
                .as_u64()
                .unwrap();
            info!("max slot: {}", storage_slot);
            if get_segment_from_slot(storage_slot) != 0 {
                return Ok((storage_blockhash, storage_slot));
            }
            sleep(Duration::from_secs(5));
        }
        Err(Error::new(
            ErrorKind::Other,
            "Couldn't get blockhash or slot",
        ))?
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
