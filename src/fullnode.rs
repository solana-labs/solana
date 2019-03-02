//! The `fullnode` module hosts all the fullnode microservices.

use crate::bank_forks::BankForks;
use crate::blocktree::Blocktree;
use crate::blocktree_processor::{self, BankForksInfo};
use crate::cluster_info::{ClusterInfo, Node, NodeInfo};
use crate::entry::create_ticks;
use crate::entry::next_entry_mut;
use crate::entry::Entry;
use crate::gossip_service::GossipService;
use crate::poh_recorder::PohRecorder;
use crate::poh_service::{PohService, PohServiceConfig};
use crate::rpc_pubsub_service::PubSubService;
use crate::rpc_service::JsonRpcService;
use crate::rpc_subscriptions::RpcSubscriptions;
use crate::service::Service;
use crate::storage_stage::StorageState;
use crate::tpu::Tpu;
use crate::tvu::{Sockets, Tvu};
use crate::voting_keypair::VotingKeypair;
use solana_metrics::counter::Counter;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_sdk::timing::timestamp;
use solana_sdk::vote_transaction::VoteTransaction;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::sleep;
use std::thread::JoinHandle;
use std::thread::{spawn, Result};
use std::time::Duration;

struct NodeServices {
    tpu: Tpu,
    tvu: Tvu,
}

impl NodeServices {
    fn new(tpu: Tpu, tvu: Tvu) -> Self {
        NodeServices { tpu, tvu }
    }

    fn join(self) -> Result<()> {
        self.tpu.join()?;
        //tvu will never stop unless exit is signaled
        self.tvu.join()?;
        Ok(())
    }

    fn exit(&self) {
        self.tpu.exit();
        self.tvu.exit();
    }
}

pub struct FullnodeConfig {
    pub sigverify_disabled: bool,
    pub voting_disabled: bool,
    pub blockstream: Option<String>,
    pub storage_rotate_count: u64,
    pub tick_config: PohServiceConfig,
    pub account_paths: Option<String>,
}
impl Default for FullnodeConfig {
    fn default() -> Self {
        // TODO: remove this, temporary parameter to configure
        // storage amount differently for test configurations
        // so tests don't take forever to run.
        const NUM_HASHES_FOR_STORAGE_ROTATE: u64 = 1024;
        Self {
            sigverify_disabled: false,
            voting_disabled: false,
            blockstream: None,
            storage_rotate_count: NUM_HASHES_FOR_STORAGE_ROTATE,
            tick_config: PohServiceConfig::default(),
            account_paths: None,
        }
    }
}

pub struct Fullnode {
    id: Pubkey,
    exit: Arc<AtomicBool>,
    rpc_service: Option<JsonRpcService>,
    rpc_pubsub_service: Option<PubSubService>,
    rpc_working_bank_handle: JoinHandle<()>,
    gossip_service: GossipService,
    node_services: NodeServices,
    poh_service: PohService,
    poh_recorder: Arc<Mutex<PohRecorder>>,
}

impl Fullnode {
    pub fn new<T>(
        mut node: Node,
        keypair: &Arc<Keypair>,
        ledger_path: &str,
        voting_keypair: T,
        entrypoint_info_option: Option<&NodeInfo>,
        config: &FullnodeConfig,
    ) -> Self
    where
        T: 'static + KeypairUtil + Sync + Send,
    {
        info!("creating bank...");

        let id = keypair.pubkey();
        assert_eq!(id, node.info.id);

        let (bank_forks, bank_forks_info, blocktree, ledger_signal_receiver) =
            new_banks_from_blocktree(ledger_path, config.account_paths.clone());

        let exit = Arc::new(AtomicBool::new(false));
        let bank_info = &bank_forks_info[0];
        let bank = bank_forks[bank_info.bank_id].clone();

        info!("starting PoH... {} {}", bank.tick_height(), bank.last_id(),);
        let poh_recorder = Arc::new(Mutex::new(PohRecorder::new(
            bank.tick_height(),
            bank.last_id(),
        )));
        let poh_service = PohService::new(poh_recorder.clone(), &config.tick_config, exit.clone());

        info!("node info: {:?}", node.info);
        info!("node entrypoint_info: {:?}", entrypoint_info_option);
        info!(
            "node local gossip address: {}",
            node.sockets.gossip.local_addr().unwrap()
        );

        let blocktree = Arc::new(blocktree);
        let bank_forks = Arc::new(RwLock::new(bank_forks));

        node.info.wallclock = timestamp();
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_keypair(
            node.info.clone(),
            keypair.clone(),
        )));

        // TODO: The RPC service assumes that there is a drone running on the cluster
        //       entrypoint, which is a bad assumption.
        //       See https://github.com/solana-labs/solana/issues/1830 for the removal of drone
        //       from the RPC API
        let drone_addr = {
            let mut entrypoint_drone_addr = match entrypoint_info_option {
                Some(entrypoint_info_info) => entrypoint_info_info.rpc,
                None => node.info.rpc,
            };
            entrypoint_drone_addr.set_port(solana_drone::drone::DRONE_PORT);
            entrypoint_drone_addr
        };

        let storage_state = StorageState::new();

        let mut rpc_service = JsonRpcService::new(
            &cluster_info,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), node.info.rpc.port()),
            drone_addr,
            storage_state.clone(),
        );

        let subscriptions = Arc::new(RpcSubscriptions::default());
        let rpc_pubsub_service = PubSubService::new(
            &subscriptions,
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
                node.info.rpc_pubsub.port(),
            ),
        );

        let gossip_service = GossipService::new(
            &cluster_info,
            Some(blocktree.clone()),
            Some(bank_forks.clone()),
            node.sockets.gossip,
            exit.clone(),
        );

        // Insert the entrypoint info, should only be None if this node
        // is the bootstrap leader
        if let Some(entrypoint_info) = entrypoint_info_option {
            cluster_info
                .write()
                .unwrap()
                .insert_info(entrypoint_info.clone());
        }

        let sockets = Sockets {
            repair: node
                .sockets
                .repair
                .try_clone()
                .expect("Failed to clone repair socket"),
            retransmit: node
                .sockets
                .retransmit
                .try_clone()
                .expect("Failed to clone retransmit socket"),
            fetch: node
                .sockets
                .tvu
                .iter()
                .map(|s| s.try_clone().expect("Failed to clone TVU Sockets"))
                .collect(),
        };

        let voting_keypair_option = if config.voting_disabled {
            None
        } else {
            Some(Arc::new(voting_keypair))
        };

        // Setup channel for rotation indications
        let (bank_sender, bank_receiver) = channel();

        let tvu = Tvu::new(
            voting_keypair_option,
            &bank_forks,
            &bank_forks_info,
            &cluster_info,
            sockets,
            blocktree.clone(),
            config.storage_rotate_count,
            &storage_state,
            bank_sender,
            config.blockstream.as_ref(),
            ledger_signal_receiver,
            &subscriptions,
            &poh_recorder,
        );
        let tpu = Tpu::new(
            id,
            &cluster_info,
            bank_receiver,
            &poh_recorder,
            node.sockets.tpu,
            node.sockets.broadcast,
            config.sigverify_disabled,
            &blocktree,
        );
        let exit_ = exit.clone();
        let bank_forks_ = bank_forks.clone();
        let rpc_working_bank_handle = spawn(move || loop {
            if exit_.load(Ordering::Relaxed) {
                break;
            }
            rpc_service.set_bank(&bank_forks_.read().unwrap().working_bank());
            let timer = Duration::from_millis(100);
            sleep(timer);
        });

        inc_new_counter_info!("fullnode-new", 1);
        Self {
            id,
            gossip_service,
            rpc_service: Some(rpc_service),
            rpc_pubsub_service: Some(rpc_pubsub_service),
            rpc_working_bank_handle,
            node_services: NodeServices::new(tpu, tvu),
            exit,
            poh_service,
            poh_recorder,
        }
    }

    // Used for notifying many nodes in parallel to exit
    pub fn exit(&self) {
        self.exit.store(true, Ordering::Relaxed);
        // Need to force the poh_recorder to drop the WorkingBank,
        // which contains the channel to BroadcastStage. This should be
        // sufficient as long as no other rotations are happening that
        // can cause the Tpu to restart a BankingStage and reset a
        // WorkingBank in poh_recorder. It follows no other rotations can be
        // in motion because exit()/close() are only called by the run() loop
        // which is the sole initiator of rotations.
        self.poh_recorder.lock().unwrap().clear_bank();
        if let Some(ref rpc_service) = self.rpc_service {
            rpc_service.exit();
        }
        if let Some(ref rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.exit();
        }
        self.node_services.exit();
        self.poh_service.exit()
    }

    pub fn close(&mut self) -> Result<()> {
        self.exit();
        self.join_mut_ref()
    }
    fn join_mut_ref(self) -> Result<()> {
        if let Some(rpc_service) = self.rpc_service {
            rpc_service.join()?;
        }
        if let Some(rpc_pubsub_service) = self.rpc_pubsub_service {
            rpc_pubsub_service.join()?;
        }

        self.rpc_working_bank_handle.join()?;
        self.gossip_service.join()?;
        self.node_services.join()?;
        trace!("exit node_services!");
        self.poh_service.join()?;
        trace!("exit poh!");
        Ok(())
    }
}
impl Drop for Fullnode {
    fn drop(&mut self) {
        self.close();
    }
}

pub fn new_banks_from_blocktree(
    blocktree_path: &str,
    account_paths: Option<String>,
) -> (BankForks, Vec<BankForksInfo>, Blocktree, Receiver<bool>) {
    let genesis_block =
        GenesisBlock::load(blocktree_path).expect("Expected to successfully open genesis block");

    let (blocktree, ledger_signal_receiver) =
        Blocktree::open_with_config_signal(blocktree_path, genesis_block.ticks_per_slot)
            .expect("Expected to successfully open database ledger");

    let (bank_forks, bank_forks_info) =
        blocktree_processor::process_blocktree(&genesis_block, &blocktree, account_paths)
            .expect("process_blocktree failed");

    (
        bank_forks,
        bank_forks_info,
        blocktree,
        ledger_signal_receiver,
    )
}

impl Service for Fullnode {
    type JoinReturnType = ();

    fn join(mut self) -> Result<()> {
        self.join_mut_ref()
    }
}

// Create entries such the node identified by active_keypair will be added to the active set for
// leader selection, and append `num_ending_ticks` empty tick entries.
pub fn make_active_set_entries(
    active_keypair: &Arc<Keypair>,
    token_source: &Keypair,
    stake: u64,
    slot_height_to_vote_on: u64,
    last_id: &Hash,
    num_ending_ticks: u64,
) -> (Vec<Entry>, VotingKeypair) {
    // 1) Assume the active_keypair node has no tokens staked
    let transfer_tx =
        SystemTransaction::new_account(&token_source, active_keypair.pubkey(), stake, *last_id, 0);
    let mut last_entry_hash = *last_id;
    let transfer_entry = next_entry_mut(&mut last_entry_hash, 1, vec![transfer_tx]);

    // 2) Create and register a vote account for active_keypair
    let voting_keypair = VotingKeypair::new_local(active_keypair);
    let vote_account_id = voting_keypair.pubkey();

    let new_vote_account_tx =
        VoteTransaction::fund_staking_account(active_keypair, vote_account_id, *last_id, 1, 1);
    let new_vote_account_entry = next_entry_mut(&mut last_entry_hash, 1, vec![new_vote_account_tx]);

    // 3) Create vote entry
    let vote_tx = VoteTransaction::new_vote(&voting_keypair, slot_height_to_vote_on, *last_id, 0);
    let vote_entry = next_entry_mut(&mut last_entry_hash, 1, vec![vote_tx]);

    // 4) Create `num_ending_ticks` empty ticks
    let mut entries = vec![transfer_entry, new_vote_account_entry, vote_entry];
    let empty_ticks = create_ticks(num_ending_ticks, last_entry_hash);
    entries.extend(empty_ticks);

    (entries, voting_keypair)
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use crate::blocktree::{create_new_tmp_ledger, tmp_copy_blocktree};
//     use crate::entry::make_consecutive_blobs;
//     use crate::streamer::responder;
//     use solana_sdk::hash::Hash;
//     use solana_sdk::timing::DEFAULT_SLOTS_PER_EPOCH;
//     use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
//     use std::fs::remove_dir_all;
//
//     #[test]
//     fn validator_exit() {
//         let leader_keypair = Keypair::new();
//         let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
//
//         let validator_keypair = Keypair::new();
//         let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
//         let (genesis_block, _mint_keypair) =
//             GenesisBlock::new_with_leader(10_000, leader_keypair.pubkey(), 1000);
//         let (validator_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
//
//         let validator = Fullnode::new(
//             validator_node,
//             &Arc::new(validator_keypair),
//             &validator_ledger_path,
//             Keypair::new(),
//             Some(&leader_node.info),
//             &FullnodeConfig::default(),
//         );
//         validator.close().unwrap();
//         remove_dir_all(validator_ledger_path).unwrap();
//     }
//
//     #[test]
//     fn validator_parallel_exit() {
//         let leader_keypair = Keypair::new();
//         let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
//
//         let mut ledger_paths = vec![];
//         let validators: Vec<Fullnode> = (0..2)
//             .map(|_| {
//                 let validator_keypair = Keypair::new();
//                 let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
//                 let (genesis_block, _mint_keypair) =
//                     GenesisBlock::new_with_leader(10_000, leader_keypair.pubkey(), 1000);
//                 let (validator_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
//                 ledger_paths.push(validator_ledger_path.clone());
//                 Fullnode::new(
//                     validator_node,
//                     &Arc::new(validator_keypair),
//                     &validator_ledger_path,
//                     Keypair::new(),
//                     Some(&leader_node.info),
//                     &FullnodeConfig::default(),
//                 )
//             })
//             .collect();
//
//         // Each validator can exit in parallel to speed many sequential calls to `join`
//         validators.iter().for_each(|v| v.exit());
//         // While join is called sequentially, the above exit call notified all the
//         // validators to exit from all their threads
//         validators.into_iter().for_each(|validator| {
//             validator.join().unwrap();
//         });
//
//         for path in ledger_paths {
//             remove_dir_all(path).unwrap();
//         }
//     }
//
//     #[test]
//     fn test_leader_to_leader_transition() {
//         solana_logger::setup();
//
//         let bootstrap_leader_keypair = Keypair::new();
//         let bootstrap_leader_node =
//             Node::new_localhost_with_pubkey(bootstrap_leader_keypair.pubkey());
//
//         // Once the bootstrap leader hits the second epoch, because there are no other choices in
//         // the active set, this leader will remain the leader in the second epoch. In the second
//         // epoch, check that the same leader knows to shut down and restart as a leader again.
//         let ticks_per_slot = 5;
//         let slots_per_epoch = 2;
//
//         let voting_keypair = Keypair::new();
//         let fullnode_config = FullnodeConfig::default();
//
//         let (mut genesis_block, _mint_keypair) =
//             GenesisBlock::new_with_leader(10_000, bootstrap_leader_keypair.pubkey(), 500);
//         genesis_block.ticks_per_slot = ticks_per_slot;
//         genesis_block.slots_per_epoch = slots_per_epoch;
//
//         let (bootstrap_leader_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
//
//         // Start the bootstrap leader
//         let bootstrap_leader = Fullnode::new(
//             bootstrap_leader_node,
//             &Arc::new(bootstrap_leader_keypair),
//             &bootstrap_leader_ledger_path,
//             voting_keypair,
//             None,
//             &fullnode_config,
//         );
//
//         let (rotation_sender, rotation_receiver) = channel();
//         let bootstrap_leader_exit = bootstrap_leader.run(Some(rotation_sender));
//
//         // Wait for the bootstrap leader to transition.  Since there are no other nodes in the
//         // cluster it will continue to be the leader
//         assert_eq!(rotation_receiver.recv().unwrap(), 1);
//         bootstrap_leader_exit();
//     }
//
//     #[test]
//     #[ignore]
//     fn test_ledger_role_transition() {
//         solana_logger::setup();
//
//         let fullnode_config = FullnodeConfig::default();
//         let ticks_per_slot = DEFAULT_TICKS_PER_SLOT;
//
//         // Create the leader and validator nodes
//         let bootstrap_leader_keypair = Arc::new(Keypair::new());
//         let validator_keypair = Arc::new(Keypair::new());
//         let (bootstrap_leader_node, validator_node, bootstrap_leader_ledger_path, _, _) =
//             setup_leader_validator(
//                 &bootstrap_leader_keypair,
//                 &validator_keypair,
//                 ticks_per_slot,
//                 0,
//             );
//         let bootstrap_leader_info = bootstrap_leader_node.info.clone();
//
//         let validator_ledger_path = tmp_copy_blocktree!(&bootstrap_leader_ledger_path);
//
//         let ledger_paths = vec![
//             bootstrap_leader_ledger_path.clone(),
//             validator_ledger_path.clone(),
//         ];
//
//         {
//             // Test that a node knows to transition to a validator based on parsing the ledger
//             let bootstrap_leader = Fullnode::new(
//                 bootstrap_leader_node,
//                 &bootstrap_leader_keypair,
//                 &bootstrap_leader_ledger_path,
//                 Keypair::new(),
//                 Some(&bootstrap_leader_info),
//                 &fullnode_config,
//             );
//             let (rotation_sender, rotation_receiver) = channel();
//             let bootstrap_leader_exit = bootstrap_leader.run(Some(rotation_sender));
//             assert_eq!(rotation_receiver.recv().unwrap(), (DEFAULT_SLOTS_PER_EPOCH));
//
//             // Test that a node knows to transition to a leader based on parsing the ledger
//             let validator = Fullnode::new(
//                 validator_node,
//                 &validator_keypair,
//                 &validator_ledger_path,
//                 Keypair::new(),
//                 Some(&bootstrap_leader_info),
//                 &fullnode_config,
//             );
//
//             let (rotation_sender, rotation_receiver) = channel();
//             let validator_exit = validator.run(Some(rotation_sender));
//             assert_eq!(rotation_receiver.recv().unwrap(), (DEFAULT_SLOTS_PER_EPOCH));
//
//             validator_exit();
//             bootstrap_leader_exit();
//         }
//         for path in ledger_paths {
//             Blocktree::destroy(&path).expect("Expected successful database destruction");
//             let _ignored = remove_dir_all(&path);
//         }
//     }
//
//     // TODO: Rework this test or TVU (make_consecutive_blobs sends blobs that can't be handled by
//     //       the replay_stage)
//     #[test]
//     #[ignore]
//     fn test_validator_to_leader_transition() {
//         solana_logger::setup();
//         // Make leader and validator node
//         let ticks_per_slot = 10;
//         let slots_per_epoch = 4;
//         let leader_keypair = Arc::new(Keypair::new());
//         let validator_keypair = Arc::new(Keypair::new());
//         let fullnode_config = FullnodeConfig::default();
//         let (leader_node, validator_node, validator_ledger_path, ledger_initial_len, last_id) =
//             setup_leader_validator(&leader_keypair, &validator_keypair, ticks_per_slot, 0);
//
//         let leader_id = leader_keypair.pubkey();
//         let validator_info = validator_node.info.clone();
//
//         info!("leader: {:?}", leader_id);
//         info!("validator: {:?}", validator_info.id);
//
//         let voting_keypair = Keypair::new();
//
//         // Start the validator
//         let validator = Fullnode::new(
//             validator_node,
//             &validator_keypair,
//             &validator_ledger_path,
//             voting_keypair,
//             Some(&leader_node.info),
//             &fullnode_config,
//         );
//
//         let blobs_to_send = slots_per_epoch * ticks_per_slot + ticks_per_slot;
//
//         // Send blobs to the validator from our mock leader
//         let t_responder = {
//             let (s_responder, r_responder) = channel();
//             let blob_sockets: Vec<Arc<UdpSocket>> =
//                 leader_node.sockets.tvu.into_iter().map(Arc::new).collect();
//             let t_responder = responder(
//                 "test_validator_to_leader_transition",
//                 blob_sockets[0].clone(),
//                 r_responder,
//             );
//
//             let tvu_address = &validator_info.tvu;
//
//             let msgs = make_consecutive_blobs(
//                 &leader_id,
//                 blobs_to_send,
//                 ledger_initial_len,
//                 last_id,
//                 &tvu_address,
//             )
//             .into_iter()
//             .rev()
//             .collect();
//             s_responder.send(msgs).expect("send");
//             t_responder
//         };
//
//         info!("waiting for validator to rotate into the leader role");
//         let (rotation_sender, rotation_receiver) = channel();
//         let validator_exit = validator.run(Some(rotation_sender));
//         let rotation = rotation_receiver.recv().unwrap();
//         assert_eq!(rotation, blobs_to_send);
//
//         // Close the validator so that rocksdb has locks available
//         validator_exit();
//         let (bank_forks, bank_forks_info, _, _) =
//             new_banks_from_blocktree(&validator_ledger_path, None);
//         let bank = bank_forks.working_bank();
//         let entry_height = bank_forks_info[0].entry_height;
//
//         assert!(bank.tick_height() >= bank.ticks_per_slot() * bank.slots_per_epoch());
//
//         assert!(entry_height >= ledger_initial_len);
//
//         // Shut down
//         t_responder.join().expect("responder thread join");
//         Blocktree::destroy(&validator_ledger_path)
//             .expect("Expected successful database destruction");
//         let _ignored = remove_dir_all(&validator_ledger_path).unwrap();
//     }
//
//     fn setup_leader_validator(
//         leader_keypair: &Arc<Keypair>,
//         validator_keypair: &Arc<Keypair>,
//         ticks_per_slot: u64,
//         num_ending_slots: u64,
//     ) -> (Node, Node, String, u64, Hash) {
//         info!("validator: {}", validator_keypair.pubkey());
//         info!("leader: {}", leader_keypair.pubkey());
//
//         let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
//         let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
//
//         let (mut genesis_block, mint_keypair) =
//             GenesisBlock::new_with_leader(10_000, leader_node.info.id, 500);
//         genesis_block.ticks_per_slot = ticks_per_slot;
//
//         let (ledger_path, last_id) = create_new_tmp_ledger!(&genesis_block);
//
//         // Add entries so that the validator is in the active set, then finish up the slot with
//         // ticks (and maybe add extra slots full of empty ticks)
//         let (entries, _) = make_active_set_entries(
//             validator_keypair,
//             &mint_keypair,
//             10,
//             0,
//             &last_id,
//             ticks_per_slot * (num_ending_slots + 1),
//         );
//
//         let blocktree = Blocktree::open_config(&ledger_path, ticks_per_slot).unwrap();
//         let last_id = entries.last().unwrap().hash;
//         let entry_height = ticks_per_slot + entries.len() as u64;
//         blocktree.write_entries(1, 0, 0, entries).unwrap();
//
//         (
//             leader_node,
//             validator_node,
//             ledger_path,
//             entry_height,
//             last_id,
//         )
//     }
// }
