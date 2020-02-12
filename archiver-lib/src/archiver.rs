use crate::result::ArchiverError;
use crossbeam_channel::unbounded;
use rand::{thread_rng, Rng, SeedableRng};
use rand_chacha::ChaChaRng;
use solana_archiver_utils::sample_file;
use solana_chacha::chacha::{chacha_cbc_encrypt_ledger, CHACHA_BLOCK_SIZE};
use solana_client::{
    rpc_client::RpcClient, rpc_request::RpcRequest, rpc_response::RpcStorageTurn,
    thin_client::ThinClient,
};
use solana_core::{
    cluster_info::{ClusterInfo, Node, VALIDATOR_PORT_RANGE},
    contact_info::ContactInfo,
    gossip_service::GossipService,
    packet::{limited_deserialize, PACKET_DATA_SIZE},
    repair_service,
    repair_service::{RepairService, RepairSlotRange, RepairStrategy},
    serve_repair::ServeRepair,
    shred_fetch_stage::ShredFetchStage,
    sigverify_stage::{DisabledSigVerifier, SigVerifyStage},
    storage_stage::NUM_STORAGE_SAMPLES,
    streamer::{receiver, responder, PacketReceiver},
    window_service::WindowService,
};
use solana_ledger::{
    blockstore::Blockstore, leader_schedule_cache::LeaderScheduleCache, shred::Shred,
};
use solana_net_utils::bind_in_range;
use solana_perf::packet::Packets;
use solana_perf::recycler::Recycler;
use solana_sdk::packet::Packet;
use solana_sdk::{
    account_utils::StateMut,
    client::{AsyncClient, SyncClient},
    clock::{get_complete_segment_from_slot, get_segment_from_slot, Slot},
    commitment_config::CommitmentConfig,
    hash::Hash,
    message::Message,
    signature::{Keypair, KeypairUtil, Signature},
    timing::timestamp,
    transaction::Transaction,
    transport::TransportError,
};
use solana_storage_program::{
    storage_contract::StorageContract,
    storage_instruction::{self, StorageAccountType},
};
use std::{
    io::{self, ErrorKind},
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    result,
    sync::atomic::{AtomicBool, Ordering},
    sync::mpsc::{channel, Receiver, Sender},
    sync::{Arc, RwLock},
    thread::{sleep, spawn, JoinHandle},
    time::Duration,
};

type Result<T> = std::result::Result<T, ArchiverError>;

static ENCRYPTED_FILENAME: &str = "ledger.enc";

#[derive(Serialize, Deserialize)]
pub enum ArchiverRequest {
    GetSlotHeight(SocketAddr),
}

pub struct Archiver {
    thread_handles: Vec<JoinHandle<()>>,
    exit: Arc<AtomicBool>,
}

// Shared Archiver Meta struct used internally
#[derive(Default)]
struct ArchiverMeta {
    slot: Slot,
    slots_per_segment: u64,
    ledger_path: PathBuf,
    signature: Signature,
    ledger_data_file_encrypted: PathBuf,
    sampling_offsets: Vec<u64>,
    blockhash: Hash,
    sha_state: Hash,
    num_chacha_blocks: usize,
    client_commitment: CommitmentConfig,
}

fn get_slot_from_signature(
    signature: &Signature,
    storage_turn: u64,
    slots_per_segment: u64,
) -> u64 {
    let signature_vec = signature.as_ref();
    let mut segment_index = u64::from(signature_vec[0])
        | (u64::from(signature_vec[1]) << 8)
        | (u64::from(signature_vec[1]) << 16)
        | (u64::from(signature_vec[2]) << 24);
    let max_segment_index =
        get_complete_segment_from_slot(storage_turn, slots_per_segment).unwrap();
    segment_index %= max_segment_index as u64;
    segment_index * slots_per_segment
}

fn create_request_processor(
    socket: UdpSocket,
    exit: &Arc<AtomicBool>,
    slot_receiver: Receiver<u64>,
) -> Vec<JoinHandle<()>> {
    let mut thread_handles = vec![];
    let (s_reader, r_reader) = channel();
    let (s_responder, r_responder) = channel();
    let storage_socket = Arc::new(socket);
    let recycler = Recycler::default();
    let t_receiver = receiver(storage_socket.clone(), exit, s_reader, recycler, "archiver");
    thread_handles.push(t_receiver);

    let t_responder = responder("archiver-responder", storage_socket, r_responder);
    thread_handles.push(t_responder);

    let exit = exit.clone();
    let t_processor = spawn(move || {
        let slot = poll_for_slot(slot_receiver, &exit);

        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            let packets = r_reader.recv_timeout(Duration::from_secs(1));

            if let Ok(packets) = packets {
                for packet in &packets.packets {
                    let req: result::Result<ArchiverRequest, Box<bincode::ErrorKind>> =
                        limited_deserialize(&packet.data[..packet.meta.size]);
                    match req {
                        Ok(ArchiverRequest::GetSlotHeight(from)) => {
                            let packet = Packet::from_data(&from, slot);
                            let _ = s_responder.send(Packets::new(vec![packet]));
                        }
                        Err(e) => {
                            info!("invalid request: {:?}", e);
                        }
                    }
                }
            }
        }
    });
    thread_handles.push(t_processor);
    thread_handles
}

fn poll_for_slot(receiver: Receiver<u64>, exit: &Arc<AtomicBool>) -> u64 {
    loop {
        let slot = receiver.recv_timeout(Duration::from_secs(1));
        if let Ok(slot) = slot {
            return slot;
        }
        if exit.load(Ordering::Relaxed) {
            return 0;
        }
    }
}

impl Archiver {
    /// Returns a Result that contains an archiver on success
    ///
    /// # Arguments
    /// * `ledger_path` - path to where the ledger will be stored.
    /// Causes panic if none
    /// * `node` - The archiver node
    /// * `cluster_entrypoint` - ContactInfo representing an entry into the network
    /// * `keypair` - Keypair for this archiver
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        ledger_path: &Path,
        node: Node,
        cluster_entrypoint: ContactInfo,
        keypair: Arc<Keypair>,
        storage_keypair: Arc<Keypair>,
        client_commitment: CommitmentConfig,
    ) -> Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));

        info!("Archiver: id: {}", keypair.pubkey());
        info!("Creating cluster info....");
        let mut cluster_info = ClusterInfo::new(node.info.clone(), keypair.clone());
        cluster_info.set_entrypoint(cluster_entrypoint.clone());
        let cluster_info = Arc::new(RwLock::new(cluster_info));

        // Note for now, this ledger will not contain any of the existing entries
        // in the ledger located at ledger_path, and will only append on newly received
        // entries after being passed to window_service
        let blockstore = Arc::new(
            Blockstore::open(ledger_path).expect("Expected to be able to open database ledger"),
        );

        let gossip_service = GossipService::new(&cluster_info, None, node.sockets.gossip, &exit);

        info!("Connecting to the cluster via {:?}", cluster_entrypoint);
        let (nodes, _) =
            match solana_core::gossip_service::discover_cluster(&cluster_entrypoint.gossip, 1) {
                Ok(nodes_and_archivers) => nodes_and_archivers,
                Err(e) => {
                    //shutdown services before exiting
                    exit.store(true, Ordering::Relaxed);
                    gossip_service.join()?;
                    return Err(e.into());
                }
            };
        let client = solana_core::gossip_service::get_client(&nodes);

        info!("Setting up mining account...");
        if let Err(e) = Self::setup_mining_account(
            &client,
            &keypair,
            &storage_keypair,
            client_commitment.clone(),
        ) {
            //shutdown services before exiting
            exit.store(true, Ordering::Relaxed);
            gossip_service.join()?;
            return Err(e);
        };

        let repair_socket = Arc::new(node.sockets.repair);
        let shred_sockets: Vec<Arc<UdpSocket>> =
            node.sockets.tvu.into_iter().map(Arc::new).collect();
        let shred_forward_sockets: Vec<Arc<UdpSocket>> = node
            .sockets
            .tvu_forwards
            .into_iter()
            .map(Arc::new)
            .collect();
        let (shred_fetch_sender, shred_fetch_receiver) = channel();
        let fetch_stage = ShredFetchStage::new(
            shred_sockets,
            shred_forward_sockets,
            repair_socket.clone(),
            &shred_fetch_sender,
            &exit,
        );
        let (slot_sender, slot_receiver) = channel();
        let request_processor =
            create_request_processor(node.sockets.storage.unwrap(), &exit, slot_receiver);

        let t_archiver = {
            let exit = exit.clone();
            let node_info = node.info.clone();
            let mut meta = ArchiverMeta {
                ledger_path: ledger_path.to_path_buf(),
                client_commitment,
                ..ArchiverMeta::default()
            };
            spawn(move || {
                // setup archiver
                let window_service = match Self::setup(
                    &mut meta,
                    cluster_info.clone(),
                    &blockstore,
                    &exit,
                    &node_info,
                    &storage_keypair,
                    repair_socket,
                    shred_fetch_receiver,
                    slot_sender,
                ) {
                    Ok(window_service) => window_service,
                    Err(e) => {
                        //shutdown services before exiting
                        error!("setup failed {:?}; archiver thread exiting...", e);
                        exit.store(true, Ordering::Relaxed);
                        request_processor
                            .into_iter()
                            .for_each(|t| t.join().unwrap());
                        fetch_stage.join().unwrap();
                        gossip_service.join().unwrap();
                        return;
                    }
                };

                info!("setup complete");
                // run archiver
                Self::run(
                    &mut meta,
                    &blockstore,
                    cluster_info,
                    &keypair,
                    &storage_keypair,
                    &exit,
                );
                // wait until exit
                request_processor
                    .into_iter()
                    .for_each(|t| t.join().unwrap());
                fetch_stage.join().unwrap();
                gossip_service.join().unwrap();
                window_service.join().unwrap()
            })
        };

        Ok(Self {
            thread_handles: vec![t_archiver],
            exit,
        })
    }

    fn run(
        meta: &mut ArchiverMeta,
        blockstore: &Arc<Blockstore>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        archiver_keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        exit: &Arc<AtomicBool>,
    ) {
        // encrypt segment
        Self::encrypt_ledger(meta, blockstore).expect("ledger encrypt not successful");
        let enc_file_path = meta.ledger_data_file_encrypted.clone();
        // do replicate
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }

            // TODO check if more segments are available - based on space constraints
            Self::create_sampling_offsets(meta);
            let sampling_offsets = &meta.sampling_offsets;
            meta.sha_state =
                match Self::sample_file_to_create_mining_hash(&enc_file_path, sampling_offsets) {
                    Ok(hash) => hash,
                    Err(err) => {
                        warn!("Error sampling file, exiting: {:?}", err);
                        break;
                    }
                };

            Self::submit_mining_proof(meta, &cluster_info, archiver_keypair, storage_keypair);

            // TODO make this a lot more frequent by picking a "new" blockhash instead of picking a storage blockhash
            // prep the next proof
            let (storage_blockhash, _) = match Self::poll_for_blockhash_and_slot(
                &cluster_info,
                meta.slots_per_segment,
                &meta.blockhash,
                exit,
            ) {
                Ok(blockhash_and_slot) => blockhash_and_slot,
                Err(e) => {
                    warn!(
                        "Error couldn't get a newer blockhash than {:?}. {:?}",
                        meta.blockhash, e
                    );
                    break;
                }
            };
            meta.blockhash = storage_blockhash;
            Self::redeem_rewards(
                &cluster_info,
                archiver_keypair,
                storage_keypair,
                meta.client_commitment.clone(),
            );
        }
        exit.store(true, Ordering::Relaxed);
    }

    fn redeem_rewards(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        archiver_keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
        client_commitment: CommitmentConfig,
    ) {
        let nodes = cluster_info.read().unwrap().tvu_peers();
        let client = solana_core::gossip_service::get_client(&nodes);

        if let Ok(Some(account)) =
            client.get_account_with_commitment(&storage_keypair.pubkey(), client_commitment.clone())
        {
            if let Ok(StorageContract::ArchiverStorage { validations, .. }) = account.state() {
                if !validations.is_empty() {
                    let ix = storage_instruction::claim_reward(
                        &archiver_keypair.pubkey(),
                        &storage_keypair.pubkey(),
                    );
                    let message =
                        Message::new_with_payer(vec![ix], Some(&archiver_keypair.pubkey()));
                    if let Err(e) = client.send_message(&[&archiver_keypair], message) {
                        error!("unable to redeem reward, tx failed: {:?}", e);
                    } else {
                        info!(
                            "collected mining rewards: Account balance {:?}",
                            client.get_balance_with_commitment(
                                &archiver_keypair.pubkey(),
                                client_commitment
                            )
                        );
                    }
                }
            }
        } else {
            info!("Redeem mining reward: No account data found");
        }
    }

    // Find a segment to replicate and download it.
    fn setup(
        meta: &mut ArchiverMeta,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        blockstore: &Arc<Blockstore>,
        exit: &Arc<AtomicBool>,
        node_info: &ContactInfo,
        storage_keypair: &Arc<Keypair>,
        repair_socket: Arc<UdpSocket>,
        shred_fetch_receiver: PacketReceiver,
        slot_sender: Sender<u64>,
    ) -> Result<WindowService> {
        let slots_per_segment =
            match Self::get_segment_config(&cluster_info, meta.client_commitment.clone()) {
                Ok(slots_per_segment) => slots_per_segment,
                Err(e) => {
                    error!("unable to get segment size configuration, exiting...");
                    //shutdown services before exiting
                    exit.store(true, Ordering::Relaxed);
                    return Err(e);
                }
            };
        let (segment_blockhash, segment_slot) = match Self::poll_for_segment(
            &cluster_info,
            slots_per_segment,
            &Hash::default(),
            exit,
        ) {
            Ok(blockhash_and_slot) => blockhash_and_slot,
            Err(e) => {
                //shutdown services before exiting
                exit.store(true, Ordering::Relaxed);
                return Err(e);
            }
        };
        let signature = storage_keypair.sign_message(segment_blockhash.as_ref());
        let slot = get_slot_from_signature(&signature, segment_slot, slots_per_segment);
        info!("replicating slot: {}", slot);
        slot_sender.send(slot)?;
        meta.slot = slot;
        meta.slots_per_segment = slots_per_segment;
        meta.signature = signature;
        meta.blockhash = segment_blockhash;

        let mut repair_slot_range = RepairSlotRange::default();
        repair_slot_range.end = slot + slots_per_segment;
        repair_slot_range.start = slot;

        let (retransmit_sender, _) = channel();

        let (verified_sender, verified_receiver) = unbounded();

        let _sigverify_stage = SigVerifyStage::new(
            shred_fetch_receiver,
            verified_sender,
            DisabledSigVerifier::default(),
        );

        let window_service = WindowService::new(
            blockstore.clone(),
            cluster_info.clone(),
            verified_receiver,
            retransmit_sender,
            repair_socket,
            &exit,
            RepairStrategy::RepairRange(repair_slot_range),
            &Arc::new(LeaderScheduleCache::default()),
            |_, _, _, _| true,
        );
        info!("waiting for ledger download");
        Self::wait_for_segment_download(
            slot,
            slots_per_segment,
            &blockstore,
            &exit,
            &node_info,
            cluster_info,
        );
        Ok(window_service)
    }

    fn wait_for_segment_download(
        start_slot: Slot,
        slots_per_segment: u64,
        blockstore: &Arc<Blockstore>,
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
            while blockstore.is_full(current_slot) {
                current_slot += 1;
                info!("current slot: {}", current_slot);
                if current_slot >= start_slot + slots_per_segment {
                    break 'outer;
                }
            }
            if exit.load(Ordering::Relaxed) {
                break;
            }
            sleep(Duration::from_secs(1));
        }

        info!("Done receiving entries from window_service");

        // Remove archiver from the data plane
        let mut contact_info = node_info.clone();
        contact_info.tvu = "0.0.0.0:0".parse().unwrap();
        contact_info.wallclock = timestamp();
        // copy over the adopted shred_version from the entrypoint
        contact_info.shred_version = cluster_info.read().unwrap().my_data().shred_version;
        {
            let mut cluster_info_w = cluster_info.write().unwrap();
            cluster_info_w.insert_self(contact_info);
        }
    }

    fn encrypt_ledger(meta: &mut ArchiverMeta, blockstore: &Arc<Blockstore>) -> Result<()> {
        meta.ledger_data_file_encrypted = meta.ledger_path.join(ENCRYPTED_FILENAME);

        {
            let mut ivec = [0u8; 64];
            ivec.copy_from_slice(&meta.signature.as_ref());

            let num_encrypted_bytes = chacha_cbc_encrypt_ledger(
                blockstore,
                meta.slot,
                meta.slots_per_segment,
                &meta.ledger_data_file_encrypted,
                &mut ivec,
            )?;

            meta.num_chacha_blocks = num_encrypted_bytes / CHACHA_BLOCK_SIZE;
        }

        info!(
            "Done encrypting the ledger: {:?}",
            meta.ledger_data_file_encrypted
        );
        Ok(())
    }

    fn create_sampling_offsets(meta: &mut ArchiverMeta) {
        meta.sampling_offsets.clear();
        let mut rng_seed = [0u8; 32];
        rng_seed.copy_from_slice(&meta.blockhash.as_ref());
        let mut rng = ChaChaRng::from_seed(rng_seed);
        for _ in 0..NUM_STORAGE_SAMPLES {
            meta.sampling_offsets
                .push(rng.gen_range(0, meta.num_chacha_blocks) as u64);
        }
    }

    fn sample_file_to_create_mining_hash(
        enc_file_path: &Path,
        sampling_offsets: &[u64],
    ) -> Result<Hash> {
        let sha_state = sample_file(enc_file_path, sampling_offsets)?;
        info!("sampled sha_state: {}", sha_state);
        Ok(sha_state)
    }

    fn setup_mining_account(
        client: &ThinClient,
        keypair: &Keypair,
        storage_keypair: &Keypair,
        client_commitment: CommitmentConfig,
    ) -> Result<()> {
        // make sure archiver has some balance
        info!("checking archiver keypair...");
        if client.poll_balance_with_timeout_and_commitment(
            &keypair.pubkey(),
            &Duration::from_millis(100),
            &Duration::from_secs(5),
            client_commitment.clone(),
        )? == 0
        {
            return Err(ArchiverError::EmptyStorageAccountBalance);
        }

        info!("checking storage account keypair...");
        // check if the storage account exists
        let balance = client
            .poll_get_balance_with_commitment(&storage_keypair.pubkey(), client_commitment.clone());
        if balance.is_err() || balance.unwrap() == 0 {
            let blockhash =
                match client.get_recent_blockhash_with_commitment(client_commitment.clone()) {
                    Ok((blockhash, _)) => blockhash,
                    Err(e) => {
                        return Err(ArchiverError::TransportError(e));
                    }
                };

            let ix = storage_instruction::create_storage_account(
                &keypair.pubkey(),
                &keypair.pubkey(),
                &storage_keypair.pubkey(),
                1,
                StorageAccountType::Archiver,
            );
            let tx = Transaction::new_signed_instructions(&[keypair], ix, blockhash);
            let signature = client.async_send_transaction(tx)?;
            client
                .poll_for_signature_with_commitment(&signature, client_commitment)
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

    fn submit_mining_proof(
        meta: &ArchiverMeta,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        archiver_keypair: &Arc<Keypair>,
        storage_keypair: &Arc<Keypair>,
    ) {
        // No point if we've got no storage account...
        let nodes = cluster_info.read().unwrap().tvu_peers();
        let client = solana_core::gossip_service::get_client(&nodes);
        let storage_balance = client.poll_get_balance_with_commitment(
            &storage_keypair.pubkey(),
            meta.client_commitment.clone(),
        );
        if storage_balance.is_err() || storage_balance.unwrap() == 0 {
            error!("Unable to submit mining proof, no storage account");
            return;
        }
        // ...or no lamports for fees
        let balance = client.poll_get_balance_with_commitment(
            &archiver_keypair.pubkey(),
            meta.client_commitment.clone(),
        );
        if balance.is_err() || balance.unwrap() == 0 {
            error!("Unable to submit mining proof, insufficient Archiver Account balance");
            return;
        }

        let blockhash =
            match client.get_recent_blockhash_with_commitment(meta.client_commitment.clone()) {
                Ok((blockhash, _)) => blockhash,
                Err(_) => {
                    error!("unable to get recent blockhash, can't submit proof");
                    return;
                }
            };
        let instruction = storage_instruction::mining_proof(
            &storage_keypair.pubkey(),
            meta.sha_state,
            get_segment_from_slot(meta.slot, meta.slots_per_segment),
            Signature::new(&meta.signature.as_ref()),
            meta.blockhash,
        );
        let message = Message::new_with_payer(vec![instruction], Some(&archiver_keypair.pubkey()));
        let mut transaction = Transaction::new(
            &[archiver_keypair.as_ref(), storage_keypair.as_ref()],
            message,
            blockhash,
        );
        if let Err(err) = client.send_and_confirm_transaction(
            &[&archiver_keypair, &storage_keypair],
            &mut transaction,
            10,
            0,
        ) {
            error!("Error: {:?}; while sending mining proof", err);
        }
    }

    pub fn close(self) {
        self.exit.store(true, Ordering::Relaxed);
        self.join()
    }

    pub fn join(self) {
        for handle in self.thread_handles {
            handle.join().unwrap();
        }
    }

    fn get_segment_config(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        client_commitment: CommitmentConfig,
    ) -> Result<u64> {
        let rpc_peers = {
            let cluster_info = cluster_info.read().unwrap();
            cluster_info.all_rpc_peers()
        };
        debug!("rpc peers: {:?}", rpc_peers);
        if !rpc_peers.is_empty() {
            let rpc_client = {
                let node_index = thread_rng().gen_range(0, rpc_peers.len());
                RpcClient::new_socket(rpc_peers[node_index].rpc)
            };
            Ok(rpc_client
                .send(
                    &RpcRequest::GetSlotsPerSegment,
                    serde_json::json!([client_commitment]),
                    0,
                )
                .map_err(|err| {
                    warn!("Error while making rpc request {:?}", err);
                    ArchiverError::ClientError(err)
                })?
                .as_u64()
                .unwrap())
        } else {
            Err(ArchiverError::NoRpcPeers)
        }
    }

    /// Waits until the first segment is ready, and returns the current segment
    fn poll_for_segment(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        slots_per_segment: u64,
        previous_blockhash: &Hash,
        exit: &Arc<AtomicBool>,
    ) -> Result<(Hash, u64)> {
        loop {
            let (blockhash, turn_slot) = Self::poll_for_blockhash_and_slot(
                cluster_info,
                slots_per_segment,
                previous_blockhash,
                exit,
            )?;
            if get_complete_segment_from_slot(turn_slot, slots_per_segment).is_some() {
                return Ok((blockhash, turn_slot));
            }
        }
    }

    /// Poll for a different blockhash and associated max_slot than `previous_blockhash`
    fn poll_for_blockhash_and_slot(
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        slots_per_segment: u64,
        previous_blockhash: &Hash,
        exit: &Arc<AtomicBool>,
    ) -> Result<(Hash, u64)> {
        info!("waiting for the next turn...");
        loop {
            let rpc_peers = {
                let cluster_info = cluster_info.read().unwrap();
                cluster_info.all_rpc_peers()
            };
            debug!("rpc peers: {:?}", rpc_peers);
            if !rpc_peers.is_empty() {
                let rpc_client = {
                    let node_index = thread_rng().gen_range(0, rpc_peers.len());
                    RpcClient::new_socket(rpc_peers[node_index].rpc)
                };
                let response = rpc_client
                    .send(
                        &RpcRequest::GetStorageTurn,
                        serde_json::value::Value::Null,
                        0,
                    )
                    .map_err(|err| {
                        warn!("Error while making rpc request {:?}", err);
                        ArchiverError::ClientError(err)
                    })?;
                let RpcStorageTurn {
                    blockhash: storage_blockhash,
                    slot: turn_slot,
                } = serde_json::from_value::<RpcStorageTurn>(response)
                    .map_err(ArchiverError::JsonError)?;
                let turn_blockhash = storage_blockhash.parse().map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "Blockhash parse failure: {:?} on {:?}",
                            err, storage_blockhash
                        ),
                    )
                })?;
                if turn_blockhash != *previous_blockhash {
                    info!("turn slot: {}", turn_slot);
                    if get_segment_from_slot(turn_slot, slots_per_segment) != 0 {
                        return Ok((turn_blockhash, turn_slot));
                    }
                }
            }
            if exit.load(Ordering::Relaxed) {
                return Err(ArchiverError::IO(io::Error::new(
                    ErrorKind::Other,
                    "exit signalled...",
                )));
            }
            sleep(Duration::from_secs(5));
        }
    }

    /// Ask an archiver to populate a given blockstore with its segment.
    /// Return the slot at the start of the archiver's segment
    ///
    /// It is recommended to use a temporary blockstore for this since the download will not verify
    /// shreds received and might impact the chaining of shreds across slots
    pub fn download_from_archiver(
        serve_repair: &ServeRepair,
        archiver_info: &ContactInfo,
        blockstore: &Arc<Blockstore>,
        slots_per_segment: u64,
    ) -> Result<u64> {
        // Create a client which downloads from the archiver and see that it
        // can respond with shreds.
        let start_slot = Self::get_archiver_segment_slot(archiver_info.storage_addr);
        info!("Archiver download: start at {}", start_slot);

        let exit = Arc::new(AtomicBool::new(false));
        let (s_reader, r_reader) = channel();
        let repair_socket = Arc::new(bind_in_range(VALIDATOR_PORT_RANGE).unwrap().1);
        let t_receiver = receiver(
            repair_socket.clone(),
            &exit,
            s_reader,
            Recycler::default(),
            "archiver_reeciver",
        );
        let id = serve_repair.keypair().pubkey();
        info!(
            "Sending repair requests from: {} to: {}",
            serve_repair.my_info().id,
            archiver_info.gossip
        );
        let repair_slot_range = RepairSlotRange {
            start: start_slot,
            end: start_slot + slots_per_segment,
        };
        // try for upto 180 seconds //TODO needs tuning if segments are huge
        for _ in 0..120 {
            // Strategy used by archivers
            let repairs = RepairService::generate_repairs_in_range(
                blockstore,
                repair_service::MAX_REPAIR_LENGTH,
                &repair_slot_range,
            );
            //iter over the repairs and send them
            if let Ok(repairs) = repairs {
                let reqs: Vec<_> = repairs
                    .into_iter()
                    .filter_map(|repair_request| {
                        serve_repair
                            .map_repair_request(&repair_request)
                            .map(|result| ((archiver_info.gossip, result), repair_request))
                            .ok()
                    })
                    .collect();

                for ((to, req), repair_request) in reqs {
                    if let Ok(local_addr) = repair_socket.local_addr() {
                        datapoint_info!(
                            "archiver_download",
                            ("repair_request", format!("{:?}", repair_request), String),
                            ("to", to.to_string(), String),
                            ("from", local_addr.to_string(), String),
                            ("id", id.to_string(), String)
                        );
                    }
                    repair_socket
                        .send_to(&req, archiver_info.gossip)
                        .unwrap_or_else(|e| {
                            error!("{} repair req send_to({}) error {:?}", id, to, e);
                            0
                        });
                }
            }
            let res = r_reader.recv_timeout(Duration::new(1, 0));
            if let Ok(mut packets) = res {
                while let Ok(mut more) = r_reader.try_recv() {
                    packets.packets.append_pinned(&mut more.packets);
                }
                let shreds: Vec<Shred> = packets
                    .packets
                    .into_iter()
                    .filter_map(|p| Shred::new_from_serialized_shred(p.data.to_vec()).ok())
                    .collect();
                blockstore.insert_shreds(shreds, None, false)?;
            }
            // check if all the slots in the segment are complete
            if Self::segment_complete(start_slot, slots_per_segment, blockstore) {
                break;
            }
            sleep(Duration::from_millis(500));
        }
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().unwrap();

        // check if all the slots in the segment are complete
        if !Self::segment_complete(start_slot, slots_per_segment, blockstore) {
            return Err(ArchiverError::SegmentDownloadError);
        }
        Ok(start_slot)
    }

    fn segment_complete(
        start_slot: Slot,
        slots_per_segment: u64,
        blockstore: &Arc<Blockstore>,
    ) -> bool {
        for slot in start_slot..(start_slot + slots_per_segment) {
            if !blockstore.is_full(slot) {
                return false;
            }
        }
        true
    }

    fn get_archiver_segment_slot(to: SocketAddr) -> u64 {
        let (_port, socket) = bind_in_range(VALIDATOR_PORT_RANGE).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        let req = ArchiverRequest::GetSlotHeight(socket.local_addr().unwrap());
        let serialized_req = bincode::serialize(&req).unwrap();
        for _ in 0..10 {
            socket.send_to(&serialized_req, to).unwrap();
            let mut buf = [0; 1024];
            if let Ok((size, _addr)) = socket.recv_from(&mut buf) {
                // Ignore bad packet and try again
                if let Ok(slot) = bincode::config()
                    .limit(PACKET_DATA_SIZE as u64)
                    .deserialize(&buf[..size])
                {
                    return slot;
                }
            }
            sleep(Duration::from_millis(500));
        }
        panic!("Couldn't get segment slot from archiver!");
    }
}
