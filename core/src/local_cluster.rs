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
use solana_drone::drone;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::timing::DEFAULT_SLOTS_PER_EPOCH;
use solana_sdk::timing::DEFAULT_TICKS_PER_SLOT;
use solana_sdk::transaction::Transaction;
use solana_vote_api::vote_instruction;
use solana_vote_api::vote_state::VoteState;
use std::collections::HashMap;
use std::fs::remove_dir_all;
use std::io::{Error, ErrorKind, Result};
use std::net::SocketAddr;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Duration;

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

pub struct LocalCluster {
    /// The pubkey of the funding account
    pub funding_pubkey: Pubkey,
    pub fullnode_config: FullnodeConfig,
    /// Entry point from which the rest of the network can be discovered
    pub entry_point_info: ContactInfo,
    pub drone_addr: SocketAddr,
    pub fullnode_infos: HashMap<Pubkey, FullnodeInfo>,
    fullnodes: HashMap<Pubkey, Fullnode>,
    genesis_ledger_path: String,
    pub genesis_block: GenesisBlock,
    replicators: Vec<Replicator>,
    pub replicator_infos: HashMap<Pubkey, ReplicatorInfo>,
}

impl LocalCluster {
    pub fn new(num_nodes: usize, cluster_lamports: u64, lamports_per_node: u64) -> Self {
        let stakes: Vec<_> = (0..num_nodes).map(|_| lamports_per_node).collect();
        Self::new_with_config(&stakes, cluster_lamports, &FullnodeConfig::default())
    }

    pub fn new_with_config(
        node_stakes: &[u64],
        cluster_lamports: u64,
        fullnode_config: &FullnodeConfig,
    ) -> Self {
        Self::new_with_config_replicators(
            node_stakes,
            cluster_lamports,
            fullnode_config,
            0,
            DEFAULT_TICKS_PER_SLOT,
            DEFAULT_SLOTS_PER_EPOCH,
        )
    }

    pub fn new_with_tick_config(
        node_stakes: &[u64],
        cluster_lamports: u64,
        fullnode_config: &FullnodeConfig,
        ticks_per_slot: u64,
        slots_per_epoch: u64,
    ) -> Self {
        Self::new_with_config_replicators(
            node_stakes,
            cluster_lamports,
            fullnode_config,
            0,
            ticks_per_slot,
            slots_per_epoch,
        )
    }

    pub fn new_with_config_replicators(
        node_stakes: &[u64],
        cluster_lamports: u64,
        fullnode_config: &FullnodeConfig,
        num_replicators: usize,
        ticks_per_slot: u64,
        slots_per_epoch: u64,
    ) -> Self {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
        let (mut genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(cluster_lamports, &leader_pubkey, node_stakes[0]);
        genesis_block.ticks_per_slot = ticks_per_slot;
        genesis_block.slots_per_epoch = slots_per_epoch;
        let (genesis_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
        let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        let voting_keypair = Keypair::new();
        let leader_contact_info = leader_node.info.clone();
        let funding_pubkey = mint_keypair.pubkey();
        let mut fullnode_config = fullnode_config.clone();

        // start a drone
        let (addr_sender, addr_receiver) = channel();
        drone::run_local_drone(mint_keypair, addr_sender);
        let drone_addr = addr_receiver.recv_timeout(Duration::from_secs(2)).unwrap();
        fullnode_config.rpc_config.drone_addr = Some(drone_addr.clone());

        let leader_server = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            None,
            &fullnode_config,
        );

        let mut fullnodes = HashMap::new();
        let mut fullnode_infos = HashMap::new();
        fullnodes.insert(leader_pubkey, leader_server);
        fullnode_infos.insert(
            leader_pubkey,
            FullnodeInfo::new(leader_keypair.clone(), leader_ledger_path),
        );

        let mut cluster = Self {
            funding_pubkey,
            entry_point_info: leader_contact_info,
            drone_addr,
            fullnodes,
            replicators: vec![],
            genesis_ledger_path,
            genesis_block,
            fullnode_infos,
            replicator_infos: HashMap::new(),
            fullnode_config: fullnode_config.clone(),
        };

        for stake in &node_stakes[1..] {
            cluster.add_validator(&fullnode_config, *stake);
        }

        discover_nodes(&cluster.entry_point_info.gossip, node_stakes.len()).unwrap();

        for _ in 0..num_replicators {
            cluster.add_replicator();
        }

        discover_nodes(
            &cluster.entry_point_info.gossip,
            node_stakes.len() + num_replicators,
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
        assert!(stake > 2);
        let validator_keypair = Arc::new(Keypair::new());
        let voting_keypair = Keypair::new();
        let validator_pubkey = validator_keypair.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
        let ledger_path = tmp_copy_blocktree!(&self.genesis_ledger_path);

        // Send each validator some lamports to vote
        let validator_balance = Self::request_airdrop(&client, &validator_pubkey, stake);
        info!(
            "validator {} balance {}",
            validator_pubkey, validator_balance
        );

        Self::create_and_fund_vote_account(&client, &voting_keypair, &validator_keypair, stake - 1)
            .unwrap();

        let validator_server = Fullnode::new(
            validator_node,
            &validator_keypair,
            &ledger_path,
            &voting_keypair.pubkey(),
            voting_keypair,
            Some(&self.entry_point_info),
            fullnode_config,
        );

        self.fullnodes
            .insert(validator_keypair.pubkey(), validator_server);
        self.fullnode_infos.insert(
            validator_keypair.pubkey(),
            FullnodeInfo::new(validator_keypair.clone(), ledger_path),
        );
    }

    fn add_replicator(&mut self) {
        let replicator_keypair = Arc::new(Keypair::new());
        let replicator_id = replicator_keypair.pubkey();
        let storage_keypair = Arc::new(Keypair::new());
        let storage_id = storage_keypair.pubkey();
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

    pub fn transfer(&self, dest_pubkey: &Pubkey, lamports: u64) -> u64 {
        let client = create_client(
            self.entry_point_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );
        Self::request_airdrop(&client, dest_pubkey, lamports)
    }

    fn request_airdrop(client: &ThinClient, dest_pubkey: &Pubkey, lamports: u64) -> u64 {
        client
            .request_airdrop(dest_pubkey, lamports)
            .expect("couldn't transfer");
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
        let delegate_id = from_account.pubkey();
        // Create the vote account if necessary
        if client.poll_get_balance(&vote_account_pubkey).unwrap_or(0) == 0 {
            // 1) Create vote account
            let instructions = vote_instruction::create_account(
                &from_account.pubkey(),
                &vote_account_pubkey,
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

            // 2) Set delegate for new vote account
            let vote_instruction =
                vote_instruction::delegate_stake(&vote_account_pubkey, &delegate_id);

            let mut transaction = Transaction::new_signed_instructions(
                &[vote_account],
                vec![vote_instruction],
                client.get_recent_blockhash().unwrap(),
            );

            client
                .retry_transfer(&vote_account, &mut transaction, 5)
                .expect("client transfer 2");
        }

        info!("Checking for vote account registration");
        let vote_account_user_data = client.get_account_data(&vote_account_pubkey);
        if let Ok(vote_account_user_data) = vote_account_user_data {
            if let Ok(vote_state) = VoteState::deserialize(&vote_account_user_data) {
                if vote_state.delegate_id == delegate_id {
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

    fn get_node_ids(&self) -> Vec<Pubkey> {
        self.fullnodes.keys().cloned().collect()
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

    #[test]
    fn test_local_cluster_start_and_exit() {
        solana_logger::setup();
        let num_nodes = 1;
        let cluster = LocalCluster::new(num_nodes, 100, 3);
        assert_eq!(cluster.fullnodes.len(), num_nodes);
        assert_eq!(cluster.replicators.len(), 0);
    }

    #[test]
    fn test_local_cluster_start_and_exit_with_config() {
        solana_logger::setup();
        let mut fullnode_exit = FullnodeConfig::default();
        fullnode_exit.rpc_config.enable_fullnode_exit = true;
        const NUM_NODES: usize = 1;
        let num_replicators = 1;
        let cluster = LocalCluster::new_with_config_replicators(
            &[3; NUM_NODES],
            100,
            &fullnode_exit,
            num_replicators,
            16,
            16,
        );
        assert_eq!(cluster.fullnodes.len(), NUM_NODES);
        assert_eq!(cluster.replicators.len(), num_replicators);
    }
}
