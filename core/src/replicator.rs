use crate::blob_fetch_stage::BlobFetchStage;
use crate::blocktree::Blocktree;
use crate::chacha::{chacha_cbc_encrypt_ledger, CHACHA_BLOCK_SIZE};
use crate::cluster_info::{ClusterInfo, Node, FULLNODE_PORT_RANGE};
use crate::contact_info::ContactInfo;
use crate::gossip_service::GossipService;
use crate::packet::to_shared_blob;
use crate::repair_service::{RepairService, RepairSlotRange, RepairStrategy};
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::{blob_receiver, receiver, responder};
use crate::window_service::WindowService;
use crate::{repair_service, window_service};
use bincode::deserialize;
use rand::thread_rng;
use rand::Rng;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_request::RpcRequest;
use solana_client::thin_client::ThinClient;
use solana_ed25519_dalek as ed25519_dalek;
use solana_netutil::bind_in_range;
use solana_sdk::account_utils::State;
use solana_sdk::client::{AsyncClient, SyncClient};
use solana_sdk::hash::{Hash, Hasher};
use solana_sdk::message::Message;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::timing::timestamp;
use solana_sdk::transaction::Transaction;
use solana_sdk::transport::TransportError;
use solana_storage_api::storage_contract::StorageContract;
use solana_storage_api::{get_segment_from_slot, storage_instruction, SLOTS_PER_SEGMENT};
use std::fs::File;
use std::io::{self, BufReader, ErrorKind, Read, Seek, SeekFrom};
use std::mem::size_of;
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, RwLock};
use std::thread::{sleep, spawn, JoinHandle};
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
    blockhash: Hash,
    signature: ed25519_dalek::Signature,
    cluster_info: Arc<RwLock<ClusterInfo>>,
    ledger_data_file_encrypted: PathBuf,
    sampling_offsets: Vec<u64>,
    sha_state: Hash,
    num_chacha_blocks: usize,
    blocktree: Arc<Blocktree>,
}

pub(crate) fn sample_file(in_path: &Path, sample_offsets: &[u64]) -> io::Result<Hash> {
    let in_file = File::open(in_path)?;
    let metadata = in_file.metadata()?;
    let mut buffer_file = BufReader::new(in_file);

    let mut hasher = Hasher::default();
    let sample_size = size_of::<Hash>();
    let sample_size64 = sample_size as u64;
    let mut buf = vec![0; sample_size];

    let file_len = metadata.len();
    if file_len < sample_size64 {
        return Err(io::Error::new(ErrorKind::Other, "file too short!"));
    }
    for offset in sample_offsets {
        if *offset > (file_len - sample_size64) / sample_size64 {
            return Err(io::Error::new(ErrorKind::Other, "offset too large"));
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
        let (nodes, _) =
            match crate::gossip_service::discover_cluster(&cluster_entrypoint.gossip, 1) {
                Ok(nodes_and_replicators) => nodes_and_replicators,
                Err(e) => {
                    //shutdown services before exiting
                    exit.store(true, Ordering::Relaxed);
                    gossip_service.join()?;
                    return Err(Error::from(e));
                }
            };
        let client = crate::gossip_service::get_client(&nodes);

        let (storage_blockhash, storage_slot) =
            match Self::poll_for_blockhash_and_slot(&cluster_info, &Hash::default()) {
                Ok(blockhash_and_slot) => blockhash_and_slot,
                Err(e) => {
                    //shutdown services before exiting
                    exit.store(true, Ordering::Relaxed);
                    gossip_service.join()?;
                    return Err(e);
                }
            };

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
            |_, _, _| true,
        );

        if let Err(e) = Self::setup_mining_account(&client, &keypair, &storage_keypair) {
            //shutdown services before exiting
            exit.store(true, Ordering::Relaxed);
            gossip_service.join()?;
            window_service.join()?;
            fetch_stage.join()?;
            return Err(e);
        };
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
            let node_info = node.info.clone();
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
            blockhash: storage_blockhash,
            signature,
            cluster_info,
            ledger_data_file_encrypted: PathBuf::default(),
            sampling_offsets: vec![],
            sha_state: Hash::default(),
            num_chacha_blocks: 0,
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
            if let Err(err) = self.sample_file_to_create_mining_hash() {
                warn!("Error sampling file, exiting: {:?}", err);
                break;
            }
            self.submit_mining_proof();

            // Todo make this a lot more frequent by picking a "new" blockhash instead of picking a storage blockhash
            // prep the next proof
            let (storage_blockhash, _) =
                match Self::poll_for_blockhash_and_slot(&self.cluster_info, &self.blockhash) {
                    Ok(blockhash_and_slot) => blockhash_and_slot,
                    Err(e) => {
                        warn!(
                            "Error couldn't get a newer blockhash than {:?}. {:?}",
                            self.blockhash, e
                        );
                        break;
                    }
                };
            self.blockhash = storage_blockhash;
            self.redeem_rewards();
        }
    }

    fn redeem_rewards(&self) {
        let nodes = self.cluster_info.read().unwrap().tvu_peers();
        let client = crate::gossip_service::get_client(&nodes);

        if let Ok(Some(account)) = client.get_account(&self.storage_keypair.pubkey()) {
            if let Ok(StorageContract::ReplicatorStorage {
                reward_validations, ..
            }) = account.state()
            {
                if !reward_validations.is_empty() {
                    let ix = storage_instruction::claim_reward(
                        &self.keypair.pubkey(),
                        &self.storage_keypair.pubkey(),
                    );
                    let message = Message::new_with_payer(vec![ix], Some(&self.keypair.pubkey()));
                    if let Err(e) = client.send_message(&[&self.keypair], message) {
                        error!("unable to redeem reward, tx failed: {:?}", e);
                    } else {
                        info!(
                            "collected mining rewards: Account balance {:?}",
                            client.get_balance(&self.keypair.pubkey())
                        );
                    }
                }
            }
        } else {
            info!("Redeem mining reward: No account data found");
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
            while blocktree.is_full(current_slot) {
                current_slot += 1;
                info!("current slot: {}", current_slot);
                if current_slot >= start_slot + SLOTS_PER_SEGMENT {
                    break 'outer;
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

        info!(
            "Done encrypting the ledger: {:?}",
            self.ledger_data_file_encrypted
        );
        Ok(())
    }

    fn create_sampling_offsets(&mut self) {
        self.sampling_offsets.clear();

        {
            use crate::storage_stage::NUM_STORAGE_SAMPLES;
            use rand::SeedableRng;
            use rand_chacha::ChaChaRng;

            let mut rng_seed = [0u8; 32];
            rng_seed.copy_from_slice(&self.blockhash.as_ref());
            let mut rng = ChaChaRng::from_seed(rng_seed);
            for _ in 0..NUM_STORAGE_SAMPLES {
                self.sampling_offsets
                    .push(rng.gen_range(0, self.num_chacha_blocks) as u64);
            }
        }
    }

    fn sample_file_to_create_mining_hash(&mut self) -> Result<()> {
        self.sha_state = sample_file(&self.ledger_data_file_encrypted, &self.sampling_offsets)?;
        info!("sampled sha_state: {}", self.sha_state);
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
                "keypair account has no balance",
            ))?
        }

        // check if the storage account exists
        let balance = client.poll_get_balance(&storage_keypair.pubkey());
        if balance.is_err() || balance.unwrap() == 0 {
            let (blockhash, _fee_calculator) = client.get_recent_blockhash().expect("blockhash");

            let ix = storage_instruction::create_replicator_storage_account(
                &keypair.pubkey(),
                &keypair.pubkey(),
                &storage_keypair.pubkey(),
                1,
            );
            let tx = Transaction::new_signed_instructions(&[keypair], ix, blockhash);
            let signature = client.async_send_transaction(tx)?;
            client
                .poll_for_signature(&signature)
                .map_err(|err| match err {
                    TransportError::IoError(e) => e,
                    TransportError::TransactionError(_) => io::Error::new(
                        ErrorKind::Other,
                        "setup_mining_account: signature not found",
                    ),
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
        let balance = client.poll_get_balance(&self.keypair.pubkey()).unwrap();
        if balance == 0 {
            error!("Unable to submit mining proof, insufficient Replicator Account balance");
            return;
        }

        let (blockhash, _) = client.get_recent_blockhash().expect("No recent blockhash");
        let instruction = storage_instruction::mining_proof(
            &self.storage_keypair.pubkey(),
            self.sha_state,
            get_segment_from_slot(self.slot),
            Signature::new(&self.signature.to_bytes()),
            self.blockhash,
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

    /// Poll for a different blockhash and associated max_slot than `previous_blockhash`
    fn poll_for_blockhash_and_slot(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        previous_blockhash: &Hash,
    ) -> result::Result<(Hash, u64), Error> {
        for _ in 0..10 {
            let rpc_peers = {
                let cluster_info = cluster_info.read().unwrap();
                cluster_info.rpc_peers()
            };
            debug!("rpc peers: {:?}", rpc_peers);
            if !rpc_peers.is_empty() {
                let rpc_client = {
                    let node_index = thread_rng().gen_range(0, rpc_peers.len());
                    RpcClient::new_socket(rpc_peers[node_index].rpc)
                };
                let response = rpc_client
                    .retry_make_rpc_request(&RpcRequest::GetStorageBlockhash, None, 0)
                    .map_err(|err| {
                        warn!("Error while making rpc request {:?}", err);
                        Error::IO(io::Error::new(ErrorKind::Other, "rpc error"))
                    })?;
                let storage_blockhash =
                    serde_json::from_value::<(String)>(response).map_err(|err| {
                        io::Error::new(
                            io::ErrorKind::Other,
                            format!("Couldn't parse response: {:?}", err),
                        )
                    })?;
                let storage_blockhash = storage_blockhash.parse().map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Blockhash parse failure: {:?} on {:?}",
                            err, storage_blockhash
                        ),
                    )
                })?;
                if storage_blockhash != *previous_blockhash {
                    let storage_slot = rpc_client
                        .retry_make_rpc_request(&RpcRequest::GetStorageSlot, None, 0)
                        .map_err(|err| {
                            warn!("Error while making rpc request {:?}", err);
                            Error::IO(io::Error::new(ErrorKind::Other, "rpc error"))
                        })?
                        .as_u64()
                        .unwrap();
                    info!("storage slot: {}", storage_slot);
                    if get_segment_from_slot(storage_slot) != 0 {
                        return Ok((storage_blockhash, storage_slot));
                    }
                }
            }
            info!("waiting for segment...");
            sleep(Duration::from_secs(5));
        }
        Err(io::Error::new(
            ErrorKind::Other,
            "Couldn't get blockhash or slot",
        ))?
    }

    /// Ask a replicator to populate a given blocktree with its segment.
    /// Return the slot at the start of the replicator's segment
    ///
    /// It is recommended to use a temporary blocktree for this since the download will not verify
    /// blobs received and might impact the chaining of blobs across slots
    pub fn download_from_replicator(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        replicator_info: &ContactInfo,
        blocktree: &Arc<Blocktree>,
    ) -> Result<(u64)> {
        // Create a client which downloads from the replicator and see that it
        // can respond with blobs.
        let start_slot = Self::get_replicator_segment_slot(replicator_info.storage_addr);
        info!("Replicator download: start at {}", start_slot);

        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let repair_socket = Arc::new(bind_in_range(FULLNODE_PORT_RANGE).unwrap().1);
        let t_receiver = blob_receiver(repair_socket.clone(), &exit, s_reader);
        let id = cluster_info.read().unwrap().id();
        info!(
            "Sending repair requests from: {} to: {}",
            cluster_info.read().unwrap().my_data().id,
            replicator_info.gossip
        );
        let repair_slot_range = RepairSlotRange {
            start: start_slot,
            end: start_slot + SLOTS_PER_SEGMENT,
        };
        // try for upto 180 seconds //TODO needs tuning if segments are huge
        for _ in 0..120 {
            // Strategy used by replicators
            let repairs = RepairService::generate_repairs_in_range(
                blocktree,
                repair_service::MAX_REPAIR_LENGTH,
                &repair_slot_range,
            );
            //iter over the repairs and send them
            if let Ok(repairs) = repairs {
                let reqs: Vec<_> = repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        cluster_info
                            .read()
                            .unwrap()
                            .map_repair_request(&repair_request)
                            .map(|result| ((replicator_info.gossip, result), repair_request))
                            .ok()
                    })
                    .collect();

                for ((to, req), repair_request) in reqs {
                    if let Ok(local_addr) = repair_socket.local_addr() {
                        datapoint_info!(
                            "replicator_download",
                            ("repair_request", format!("{:?}", repair_request), String),
                            ("to", to.to_string(), String),
                            ("from", local_addr.to_string(), String),
                            ("id", id.to_string(), String)
                        );
                    }
                    repair_socket
                        .send_to(&req, replicator_info.gossip)
                        .unwrap_or_else(|e| {
                            error!("{} repair req send_to({}) error {:?}", id, to, e);
                            0
                        });
                }
            }
            let res = r_reader.recv_timeout(Duration::new(1, 0));
            if let Ok(blobs) = res {
                window_service::process_blobs(&blobs, blocktree)?;
            }
            // check if all the slots in the segment are complete
            if Self::segment_complete(start_slot, blocktree) {
                break;
            }
            sleep(Duration::from_millis(500));
        }
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().unwrap();

        // check if all the slots in the segment are complete
        if !Self::segment_complete(start_slot, blocktree) {
            Err(io::Error::new(
                ErrorKind::Other,
                "Unable to download the full segment",
            ))?
        }
        Ok(start_slot)
    }

    fn segment_complete(start_slot: u64, blocktree: &Arc<Blocktree>) -> bool {
        for slot in start_slot..(start_slot + SLOTS_PER_SEGMENT) {
            if !blocktree.is_full(slot) {
                return false;
            }
        }
        true
    }

    fn get_replicator_segment_slot(to: SocketAddr) -> u64 {
        let (_port, socket) = bind_in_range(FULLNODE_PORT_RANGE).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        let req = ReplicatorRequest::GetSlotHeight(socket.local_addr().unwrap());
        let serialized_req = bincode::serialize(&req).unwrap();
        for _ in 0..10 {
            socket.send_to(&serialized_req, to).unwrap();
            let mut buf = [0; 1024];
            if let Ok((size, _addr)) = socket.recv_from(&mut buf) {
                return deserialize(&buf[..size]).unwrap();
            }
            sleep(Duration::from_millis(500));
        }
        panic!("Couldn't get slot height!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, remove_file};
    use std::io::Write;

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
