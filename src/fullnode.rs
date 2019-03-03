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
    pub id: Pubkey,
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
        let (poh_recorder, entry_receiver) = PohRecorder::new(bank.tick_height(), bank.last_id());
        let poh_recorder = Arc::new(Mutex::new(poh_recorder));
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

        let rpc_service = JsonRpcService::new(
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
        let tvu = Tvu::new(
            voting_keypair_option,
            &bank_forks,
            &bank_forks_info,
            &cluster_info,
            sockets,
            blocktree.clone(),
            config.storage_rotate_count,
            &storage_state,
            config.blockstream.as_ref(),
            ledger_signal_receiver,
            &subscriptions,
            &poh_recorder,
        );
        let tpu = Tpu::new(
            id,
            &cluster_info,
            &poh_recorder,
            entry_receiver,
            node.sockets.tpu,
            node.sockets.broadcast,
            config.sigverify_disabled,
            &blocktree,
        );
        let exit_ = exit.clone();
        let bank_forks_ = bank_forks.clone();
        let rpc_service_rp = rpc_service.request_processor.clone();
        let rpc_working_bank_handle = spawn(move || loop {
            if exit_.load(Ordering::Relaxed) {
                break;
            }
            let bank = bank_forks_.read().unwrap().working_bank();
            trace!("rpc working bank {} {}", bank.slot(), bank.last_id());
            rpc_service_rp
                .write()
                .unwrap()
                .set_bank(&bank_forks_.read().unwrap().working_bank());
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

    pub fn close(self) -> Result<()> {
        self.exit();
        self.join()
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

    fn join(self) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::create_new_tmp_ledger;
    use std::fs::remove_dir_all;

    #[test]
    fn validator_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let validator_keypair = Keypair::new();
        let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
        let (genesis_block, _mint_keypair) =
            GenesisBlock::new_with_leader(10_000, leader_keypair.pubkey(), 1000);
        let (validator_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);

        let validator = Fullnode::new(
            validator_node,
            &Arc::new(validator_keypair),
            &validator_ledger_path,
            Keypair::new(),
            Some(&leader_node.info),
            &FullnodeConfig::default(),
        );
        validator.close().unwrap();
        remove_dir_all(validator_ledger_path).unwrap();
    }

    #[test]
    fn validator_parallel_exit() {
        let leader_keypair = Keypair::new();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());

        let mut ledger_paths = vec![];
        let validators: Vec<Fullnode> = (0..2)
            .map(|_| {
                let validator_keypair = Keypair::new();
                let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
                let (genesis_block, _mint_keypair) =
                    GenesisBlock::new_with_leader(10_000, leader_keypair.pubkey(), 1000);
                let (validator_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
                ledger_paths.push(validator_ledger_path.clone());
                Fullnode::new(
                    validator_node,
                    &Arc::new(validator_keypair),
                    &validator_ledger_path,
                    Keypair::new(),
                    Some(&leader_node.info),
                    &FullnodeConfig::default(),
                )
            })
            .collect();

        // Each validator can exit in parallel to speed many sequential calls to `join`
        validators.iter().for_each(|v| v.exit());
        // While join is called sequentially, the above exit call notified all the
        // validators to exit from all their threads
        validators.into_iter().for_each(|validator| {
            validator.join().unwrap();
        });

        for path in ledger_paths {
            remove_dir_all(path).unwrap();
        }
    }
}
