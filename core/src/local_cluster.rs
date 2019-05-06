use crate::blocktree::{create_new_tmp_ledger, tmp_copy_blocktree};
use crate::cluster::Cluster;
use crate::cluster_info::{Node, FULLNODE_PORT_RANGE};
use crate::contact_info::ContactInfo;
use crate::fullnode::{Fullnode, FullnodeConfig};
use crate::gossip_service::discover_nodes;
use crate::replicator::Replicator;
use crate::service::Service;
use solana_client::thin_client::create_client;
use solana_client::thin_client::ThinClient;
use solana_sdk::client::SyncClient;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction;
use solana_sdk::timing::DEFAULT_SLOTS_PER_EPOCH;
use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
use solana_sdk::transaction::Transaction;
use solana_vote_api::vote_instruction;
use solana_vote_api::vote_state::VoteState;
use std::collections::HashMap;
use std::fs::remove_dir_all;
use std::io::{Error, ErrorKind, Result};
use std::sync::Arc;

pub struct FullnodeInfo {
    pub keypair: Arc<Keypair>,
    pub ledger_path: String,
}

impl FullnodeInfo {
    fn new(keypair: Arc<Keypair>, ledger_path: String) -> Self {
        Self {
            keypair,
            ledger_path,
        }
    }
}

pub struct ReplicatorInfo {
    pub replicator_storage_id: Pubkey,
    pub ledger_path: String,
}

impl ReplicatorInfo {
    fn new(storage_id: Pubkey, ledger_path: String) -> Self {
        Self {
            replicator_storage_id: storage_id,
            ledger_path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClusterConfig {
    /// The fullnode config that should be applied to every node in the cluster
    pub fullnode_config: FullnodeConfig,
    /// Number of replicators in the cluster
    /// Note- replicators will timeout if ticks_per_slot is much larger than the default 8
    pub num_replicators: u64,
    /// Number of nodes that are unstaked and not voting (a.k.a listening)
    pub num_listeners: u64,
    /// The stakes of each node
    pub node_stakes: Vec<u64>,
    /// The total lamports available to the cluster
    pub cluster_lamports: u64,
    pub ticks_per_slot: u64,
    pub slots_per_epoch: u64,
    pub native_instruction_processors: Vec<(String, Pubkey)>,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        ClusterConfig {
            fullnode_config: FullnodeConfig::default(),
            num_replicators: 0,
            num_listeners: 0,
            node_stakes: vec![],
            cluster_lamports: 0,
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_epoch: DEFAULT_SLOTS_PER_EPOCH,
            native_instruction_processors: vec![],
        }
    }
}

pub struct LocalCluster {
    /// Keypair with funding to participate in the network
    pub funding_keypair: Keypair,
    pub fullnode_config: FullnodeConfig,
    /// Entry point from which the rest of the network can be discovered
    pub entry_point_info: ContactInfo,
    pub fullnode_infos: HashMap<Pubkey, FullnodeInfo>,
    pub listener_infos: HashMap<Pubkey, FullnodeInfo>,
    fullnodes: HashMap<Pubkey, Fullnode>,
    genesis_ledger_path: String,
    pub genesis_block: GenesisBlock,
    replicators: Vec<Replicator>,
    pub replicator_infos: HashMap<Pubkey, ReplicatorInfo>,
}

impl LocalCluster {
    pub fn new_with_equal_stakes(
        num_nodes: usize,
        cluster_lamports: u64,
        lamports_per_node: u64,
    ) -> Self {
        let stakes: Vec<_> = (0..num_nodes).map(|_| lamports_per_node).collect();
        let config = ClusterConfig {
            node_stakes: stakes,
            cluster_lamports,
            ..ClusterConfig::default()
        };
        Self::new(&config)
    }

    pub fn new(config: &ClusterConfig) -> Self {
        let voting_keypair = Keypair::new();
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
        let (mut genesis_block, mint_keypair) = GenesisBlock::new_with_leader(
            config.cluster_lamports,
            &leader_pubkey,
            config.node_stakes[0],
        );
        genesis_block.ticks_per_slot = config.ticks_per_slot;
        genesis_block.slots_per_epoch = config.slots_per_epoch;
        genesis_block
            .native_instruction_processors
            .extend_from_slice(&config.native_instruction_processors);
        genesis_block.bootstrap_leader_vote_account_id = voting_keypair.pubkey();
        let (genesis_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
        let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        let leader_contact_info = leader_node.info.clone();

        let leader_server = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            None,
            &config.fullnode_config,
        );

        let mut fullnodes = HashMap::new();
        let mut fullnode_infos = HashMap::new();
        fullnodes.insert(leader_pubkey, leader_server);
        fullnode_infos.insert(
            leader_pubkey,
            FullnodeInfo::new(leader_keypair.clone(), leader_ledger_path),
        );

        let mut cluster = Self {
            funding_keypair: mint_keypair,
            entry_point_info: leader_contact_info,
            fullnodes,
            replicators: vec![],
            genesis_ledger_path,
            genesis_block,
            fullnode_infos,
            replicator_infos: HashMap::new(),
            fullnode_config: config.fullnode_config.clone(),
            listener_infos: HashMap::new(),
        };

        for stake in &config.node_stakes[1..] {
            cluster.add_validator(&config.fullnode_config, *stake);
        }

        let listener_config = FullnodeConfig {
            voting_disabled: true,
            ..config.fullnode_config.clone()
        };
        (0..config.num_listeners).for_each(|_| cluster.add_validator(&listener_config, 0));

        discover_nodes(
            &cluster.entry_point_info.gossip,
            config.node_stakes.len() + config.num_listeners as usize,
        )
        .unwrap();

        for _ in 0..config.num_replicators {
            cluster.add_replicator();
        }

        discover_nodes(
            &cluster.entry_point_info.gossip,
            config.node_stakes.len() + config.num_replicators as usize,
        )
        .unwrap();

        cluster
    }

    pub fn exit(&self) {
        for node in self.fullnodes.values() {
            node.exit();
        }
    }

    pub fn close_preserve_ledgers(&mut self) {
        self.exit();
        for (_, node) in self.fullnodes.drain() {
            node.join().unwrap();
        }

        while let Some(replicator) = self.replicators.pop() {
            replicator.close();
        }
    }

    fn add_validator(&mut self, fullnode_config: &FullnodeConfig, stake: u64) {
        let client = create_client(
            self.entry_point_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );

        // Must have enough tokens to fund vote account and set delegate
        let validator_keypair = Arc::new(Keypair::new());
        let voting_keypair = Keypair::new();
        let validator_pubkey = validator_keypair.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
        let ledger_path = tmp_copy_blocktree!(&self.genesis_ledger_path);

        if fullnode_config.voting_disabled {
            // setup as a listener
            info!("listener {} ", validator_pubkey,);
        } else {
            assert!(stake > 2);
            // Send each validator some lamports to vote
            let validator_balance = Self::transfer_with_client(
                &client,
                &self.funding_keypair,
                &validator_pubkey,
                stake,
            );
            info!(
                "validator {} balance {}",
                validator_pubkey, validator_balance
            );

            Self::create_and_fund_vote_account(
                &client,
                &voting_keypair,
                &validator_keypair,
                stake - 1,
            )
            .unwrap();
        }

        let validator_server = Fullnode::new(
            validator_node,
            &validator_keypair,
            &ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            Some(&self.entry_point_info),
            &fullnode_config,
        );

        self.fullnodes
            .insert(validator_keypair.pubkey(), validator_server);
        if fullnode_config.voting_disabled {
            self.listener_infos.insert(
                validator_keypair.pubkey(),
                FullnodeInfo::new(validator_keypair.clone(), ledger_path),
            );
        } else {
            self.fullnode_infos.insert(
                validator_keypair.pubkey(),
                FullnodeInfo::new(validator_keypair.clone(), ledger_path),
            );
        }
    }

    fn add_replicator(&mut self) {
        let replicator_keypair = Arc::new(Keypair::new());
        let replicator_id = replicator_keypair.pubkey();
        let storage_keypair = Arc::new(Keypair::new());
        let storage_id = storage_keypair.pubkey();
        let client = create_client(
            self.entry_point_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );

        Self::transfer_with_client(
            &client,
            &self.funding_keypair,
            &replicator_keypair.pubkey(),
            1,
        );
        let replicator_node = Node::new_localhost_replicator(&replicator_id);

        let (replicator_ledger_path, _blockhash) = create_new_tmp_ledger!(&self.genesis_block);
        let replicator = Replicator::new(
            &replicator_ledger_path,
            replicator_node,
            self.entry_point_info.clone(),
            replicator_keypair,
            storage_keypair,
            None,
        )
        .unwrap();

        self.replicators.push(replicator);
        self.replicator_infos.insert(
            replicator_id,
            ReplicatorInfo::new(storage_id, replicator_ledger_path),
        );
    }

    fn close(&mut self) {
        self.close_preserve_ledgers();
        for ledger_path in self
            .fullnode_infos
            .values()
            .map(|f| &f.ledger_path)
            .chain(self.replicator_infos.values().map(|info| &info.ledger_path))
        {
            remove_dir_all(&ledger_path)
                .unwrap_or_else(|_| panic!("Unable to remove {}", ledger_path));
        }
    }

    pub fn transfer(&self, source_keypair: &Keypair, dest_pubkey: &Pubkey, lamports: u64) -> u64 {
        let client = create_client(
            self.entry_point_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );
        Self::transfer_with_client(&client, source_keypair, dest_pubkey, lamports)
    }

    fn transfer_with_client(
        client: &ThinClient,
        source_keypair: &Keypair,
        dest_pubkey: &Pubkey,
        lamports: u64,
    ) -> u64 {
        trace!("getting leader blockhash");
        let blockhash = client.get_recent_blockhash().unwrap();
        let mut tx = system_transaction::create_user_account(
            &source_keypair,
            dest_pubkey,
            lamports,
            blockhash,
            0,
        );
        info!(
            "executing transfer of {} from {} to {}",
            lamports,
            source_keypair.pubkey(),
            *dest_pubkey
        );
        client
            .retry_transfer(&source_keypair, &mut tx, 5)
            .expect("client transfer");
        client
            .wait_for_balance(dest_pubkey, Some(lamports))
            .expect("get balance")
    }

    fn create_and_fund_vote_account(
        client: &ThinClient,
        vote_account: &Keypair,
        from_account: &Arc<Keypair>,
        amount: u64,
    ) -> Result<()> {
        let vote_account_pubkey = vote_account.pubkey();
        let node_id = from_account.pubkey();
        // Create the vote account if necessary
        if client.poll_get_balance(&vote_account_pubkey).unwrap_or(0) == 0 {
            // 1) Create vote account
            let instructions = vote_instruction::create_account(
                &from_account.pubkey(),
                &vote_account_pubkey,
                &node_id,
                0,
                amount,
            );
            let mut transaction = Transaction::new_signed_instructions(
                &[from_account.as_ref()],
                instructions,
                client.get_recent_blockhash().unwrap(),
            );

            client
                .retry_transfer(&from_account, &mut transaction, 5)
                .expect("client transfer");
            client
                .wait_for_balance(&vote_account_pubkey, Some(amount))
                .expect("get balance");
        }
        info!("Checking for vote account registration");
        let vote_account_user_data = client.get_account_data(&vote_account_pubkey);
        if let Ok(Some(vote_account_user_data)) = vote_account_user_data {
            if let Ok(vote_state) = VoteState::deserialize(&vote_account_user_data) {
                if vote_state.node_id == node_id {
                    return Ok(());
                }
            }
        }

        Err(Error::new(
            ErrorKind::Other,
            "expected successful vote account registration",
        ))
    }
}

impl Cluster for LocalCluster {
    fn get_node_ids(&self) -> Vec<Pubkey> {
        self.fullnodes.keys().cloned().collect()
    }

    fn restart_node(&mut self, pubkey: Pubkey) {
        // Shut down the fullnode
        let node = self.fullnodes.remove(&pubkey).unwrap();
        node.exit();
        node.join().unwrap();

        // Restart the node
        let fullnode_info = &self.fullnode_infos[&pubkey];
        let node = Node::new_localhost_with_pubkey(&fullnode_info.keypair.pubkey());
        if pubkey == self.entry_point_info.id {
            self.entry_point_info = node.info.clone();
        }
        let new_voting_keypair = Keypair::new();
        let restarted_node = Fullnode::new(
            node,
            &fullnode_info.keypair,
            &fullnode_info.ledger_path,
            &new_voting_keypair.pubkey(),
            new_voting_keypair,
            None,
            &self.fullnode_config,
        );

        self.fullnodes.insert(pubkey, restarted_node);
    }
}

impl Drop for LocalCluster {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::storage_stage::STORAGE_ROTATE_TEST_COUNT;
    use solana_runtime::bank::MINIMUM_SLOT_LENGTH;

    #[test]
    fn test_local_cluster_start_and_exit() {
        solana_logger::setup();
        let num_nodes = 1;
        let cluster = LocalCluster::new_with_equal_stakes(num_nodes, 100, 3);
        assert_eq!(cluster.fullnodes.len(), num_nodes);
        assert_eq!(cluster.replicators.len(), 0);
    }

    #[test]
    fn test_local_cluster_start_and_exit_with_config() {
        solana_logger::setup();
        let mut fullnode_config = FullnodeConfig::default();
        fullnode_config.rpc_config.enable_fullnode_exit = true;
        fullnode_config.storage_rotate_count = STORAGE_ROTATE_TEST_COUNT;
        const NUM_NODES: usize = 1;
        let num_replicators = 1;
        let config = ClusterConfig {
            fullnode_config,
            num_replicators: 1,
            node_stakes: vec![3; NUM_NODES],
            cluster_lamports: 100,
            ticks_per_slot: 8,
            slots_per_epoch: MINIMUM_SLOT_LENGTH as u64,
            ..ClusterConfig::default()
        };
        let cluster = LocalCluster::new(&config);
        assert_eq!(cluster.fullnodes.len(), NUM_NODES);
        assert_eq!(cluster.replicators.len(), num_replicators);
    }

}
