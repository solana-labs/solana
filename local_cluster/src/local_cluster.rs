use crate::cluster::{Cluster, ClusterValidatorInfo, ValidatorInfo};
use solana_client::thin_client::{create_client, ThinClient};
use solana_core::{
    blocktree::create_new_tmp_ledger,
    cluster_info::{Node, FULLNODE_PORT_RANGE},
    contact_info::ContactInfo,
    genesis_utils::{create_genesis_block_with_leader, GenesisBlockInfo},
    gossip_service::discover_cluster,
    replicator::Replicator,
    service::Service,
    validator::{Validator, ValidatorConfig},
};
use solana_sdk::{
    client::SyncClient,
    clock::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_SLOTS_PER_SEGMENT, DEFAULT_TICKS_PER_SLOT},
    genesis_block::GenesisBlock,
    message::Message,
    poh_config::PohConfig,
    pubkey::Pubkey,
    signature::{Keypair, KeypairUtil},
    system_transaction,
    transaction::Transaction,
};
use solana_stake_api::{
    config as stake_config, stake_instruction,
    stake_state::{Authorized as StakeAuthorized, StakeState},
};
use solana_storage_api::{
    storage_contract,
    storage_instruction::{self, StorageAccountType},
};
use solana_vote_api::{
    vote_instruction,
    vote_state::{VoteInit, VoteState},
};
use std::{
    collections::HashMap,
    fs::remove_dir_all,
    io::{Error, ErrorKind, Result},
    path::PathBuf,
    sync::Arc,
};

pub struct ReplicatorInfo {
    pub replicator_storage_pubkey: Pubkey,
    pub ledger_path: PathBuf,
}

impl ReplicatorInfo {
    fn new(storage_pubkey: Pubkey, ledger_path: PathBuf) -> Self {
        Self {
            replicator_storage_pubkey: storage_pubkey,
            ledger_path,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ClusterConfig {
    /// The fullnode config that should be applied to every node in the cluster
    pub validator_configs: Vec<ValidatorConfig>,
    /// Number of replicators in the cluster
    /// Note- replicators will timeout if ticks_per_slot is much larger than the default 8
    pub num_replicators: usize,
    /// Number of nodes that are unstaked and not voting (a.k.a listening)
    pub num_listeners: u64,
    /// The stakes of each node
    pub node_stakes: Vec<u64>,
    /// The total lamports available to the cluster
    pub cluster_lamports: u64,
    pub ticks_per_slot: u64,
    pub slots_per_epoch: u64,
    pub slots_per_segment: u64,
    pub stakers_slot_offset: u64,
    pub native_instruction_processors: Vec<(String, Pubkey)>,
    pub poh_config: PohConfig,
}

impl Default for ClusterConfig {
    fn default() -> Self {
        ClusterConfig {
            validator_configs: vec![],
            num_replicators: 0,
            num_listeners: 0,
            node_stakes: vec![],
            cluster_lamports: 0,
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_epoch: DEFAULT_SLOTS_PER_EPOCH,
            slots_per_segment: DEFAULT_SLOTS_PER_SEGMENT,
            stakers_slot_offset: DEFAULT_SLOTS_PER_EPOCH,
            native_instruction_processors: vec![],
            poh_config: PohConfig::default(),
        }
    }
}

pub struct LocalCluster {
    /// Keypair with funding to participate in the network
    pub funding_keypair: Keypair,
    /// Entry point from which the rest of the network can be discovered
    pub entry_point_info: ContactInfo,
    pub fullnode_infos: HashMap<Pubkey, ClusterValidatorInfo>,
    pub listener_infos: HashMap<Pubkey, ClusterValidatorInfo>,
    fullnodes: HashMap<Pubkey, Validator>,
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
            validator_configs: vec![ValidatorConfig::default(); num_nodes],
            ..ClusterConfig::default()
        };
        Self::new(&config)
    }

    pub fn new(config: &ClusterConfig) -> Self {
        assert_eq!(config.validator_configs.len(), config.node_stakes.len());
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_node = Node::new_localhost_with_pubkey(&leader_keypair.pubkey());
        let GenesisBlockInfo {
            mut genesis_block,
            mint_keypair,
            voting_keypair,
        } = create_genesis_block_with_leader(
            config.cluster_lamports,
            &leader_pubkey,
            config.node_stakes[0],
        );
        genesis_block.ticks_per_slot = config.ticks_per_slot;
        genesis_block.slots_per_epoch = config.slots_per_epoch;
        genesis_block.slots_per_segment = config.slots_per_segment;
        genesis_block.stakers_slot_offset = config.stakers_slot_offset;
        genesis_block.poh_config = config.poh_config.clone();
        genesis_block
            .native_instruction_processors
            .extend_from_slice(&config.native_instruction_processors);
        genesis_block
            .native_instruction_processors
            .push(solana_storage_program!());

        let storage_keypair = Keypair::new();
        genesis_block.accounts.push((
            storage_keypair.pubkey(),
            storage_contract::create_validator_storage_account(leader_pubkey, 1),
        ));

        // Replace staking config
        genesis_block.accounts = genesis_block
            .accounts
            .into_iter()
            .filter(|(pubkey, _)| *pubkey != stake_config::id())
            .collect();
        genesis_block.accounts.push((
            stake_config::id(),
            stake_config::create_account(
                1,
                &stake_config::Config {
                    warmup_rate: 1_000_000_000.0f64,
                    cooldown_rate: 1_000_000_000.0f64,
                    slash_penalty: std::u8::MAX,
                },
            ),
        ));

        let (leader_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
        let leader_contact_info = leader_node.info.clone();
        let leader_storage_keypair = Arc::new(storage_keypair);
        let leader_voting_keypair = Arc::new(voting_keypair);
        let leader_server = Validator::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            &leader_voting_keypair.pubkey(),
            &leader_voting_keypair,
            &leader_storage_keypair,
            None,
            true,
            &config.validator_configs[0],
        );

        let mut fullnodes = HashMap::new();
        let mut fullnode_infos = HashMap::new();
        fullnodes.insert(leader_pubkey, leader_server);
        let leader_info = ValidatorInfo {
            keypair: leader_keypair,
            voting_keypair: leader_voting_keypair,
            storage_keypair: leader_storage_keypair,
            ledger_path: leader_ledger_path,
            contact_info: leader_contact_info.clone(),
        };

        let cluster_leader =
            ClusterValidatorInfo::new(leader_info, config.validator_configs[0].clone());

        fullnode_infos.insert(leader_pubkey, cluster_leader);

        let mut cluster = Self {
            funding_keypair: mint_keypair,
            entry_point_info: leader_contact_info,
            fullnodes,
            replicators: vec![],
            genesis_block,
            fullnode_infos,
            replicator_infos: HashMap::new(),
            listener_infos: HashMap::new(),
        };

        for (stake, validator_config) in (&config.node_stakes[1..])
            .iter()
            .zip((&config.validator_configs[1..]).iter())
        {
            cluster.add_validator(validator_config, *stake);
        }

        let listener_config = ValidatorConfig {
            voting_disabled: true,
            ..config.validator_configs[0].clone()
        };
        (0..config.num_listeners).for_each(|_| cluster.add_validator(&listener_config, 0));

        discover_cluster(
            &cluster.entry_point_info.gossip,
            config.node_stakes.len() + config.num_listeners as usize,
        )
        .unwrap();

        for _ in 0..config.num_replicators {
            cluster.add_replicator();
        }

        discover_cluster(
            &cluster.entry_point_info.gossip,
            config.node_stakes.len() + config.num_replicators as usize,
        )
        .unwrap();

        cluster
    }

    pub fn exit(&mut self) {
        for node in self.fullnodes.values_mut() {
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

    pub fn add_validator(&mut self, validator_config: &ValidatorConfig, stake: u64) {
        let client = create_client(
            self.entry_point_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );

        // Must have enough tokens to fund vote account and set delegate
        let validator_keypair = Arc::new(Keypair::new());
        let voting_keypair = Keypair::new();
        let storage_keypair = Arc::new(Keypair::new());
        let validator_pubkey = validator_keypair.pubkey();
        let validator_node = Node::new_localhost_with_pubkey(&validator_keypair.pubkey());
        let contact_info = validator_node.info.clone();
        let (ledger_path, _blockhash) = create_new_tmp_ledger!(&self.genesis_block);

        if validator_config.voting_disabled {
            // setup as a listener
            info!("listener {} ", validator_pubkey,);
        } else {
            // Give the validator some lamports to setup vote and storage accounts
            let validator_balance = Self::transfer_with_client(
                &client,
                &self.funding_keypair,
                &validator_pubkey,
                stake * 2 + 2,
            );
            info!(
                "validator {} balance {}",
                validator_pubkey, validator_balance
            );

            Self::setup_vote_and_stake_accounts(
                &client,
                &voting_keypair,
                &validator_keypair,
                stake,
            )
            .unwrap();

            Self::setup_storage_account(&client, &storage_keypair, &validator_keypair, false)
                .unwrap();
        }

        let voting_keypair = Arc::new(voting_keypair);
        let validator_server = Validator::new(
            validator_node,
            &validator_keypair,
            &ledger_path,
            &voting_keypair.pubkey(),
            &voting_keypair,
            &storage_keypair,
            Some(&self.entry_point_info),
            true,
            &validator_config,
        );

        self.fullnodes
            .insert(validator_keypair.pubkey(), validator_server);
        let validator_pubkey = validator_keypair.pubkey();
        let validator_info = ClusterValidatorInfo::new(
            ValidatorInfo {
                keypair: validator_keypair,
                voting_keypair,
                storage_keypair,
                ledger_path,
                contact_info,
            },
            validator_config.clone(),
        );

        if validator_config.voting_disabled {
            self.listener_infos.insert(validator_pubkey, validator_info);
        } else {
            self.fullnode_infos.insert(validator_pubkey, validator_info);
        }
    }

    fn add_replicator(&mut self) {
        let replicator_keypair = Arc::new(Keypair::new());
        let replicator_pubkey = replicator_keypair.pubkey();
        let storage_keypair = Arc::new(Keypair::new());
        let storage_pubkey = storage_keypair.pubkey();
        let client = create_client(
            self.entry_point_info.client_facing_addr(),
            FULLNODE_PORT_RANGE,
        );

        // Give the replicator some lamports to setup its storage accounts
        Self::transfer_with_client(
            &client,
            &self.funding_keypair,
            &replicator_keypair.pubkey(),
            42,
        );
        let replicator_node = Node::new_localhost_replicator(&replicator_pubkey);

        Self::setup_storage_account(&client, &storage_keypair, &replicator_keypair, true).unwrap();

        let (replicator_ledger_path, _blockhash) = create_new_tmp_ledger!(&self.genesis_block);
        let replicator = Replicator::new(
            &replicator_ledger_path,
            replicator_node,
            self.entry_point_info.clone(),
            replicator_keypair,
            storage_keypair,
        )
        .unwrap_or_else(|err| panic!("Replicator::new() failed: {:?}", err));

        self.replicators.push(replicator);
        self.replicator_infos.insert(
            replicator_pubkey,
            ReplicatorInfo::new(storage_pubkey, replicator_ledger_path),
        );
    }

    fn close(&mut self) {
        self.close_preserve_ledgers();
        for ledger_path in self
            .fullnode_infos
            .values()
            .map(|f| &f.info.ledger_path)
            .chain(self.replicator_infos.values().map(|info| &info.ledger_path))
        {
            remove_dir_all(&ledger_path)
                .unwrap_or_else(|_| panic!("Unable to remove {:?}", ledger_path));
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
        let (blockhash, _fee_calculator) = client.get_recent_blockhash().unwrap();
        let mut tx = system_transaction::create_user_account(
            &source_keypair,
            dest_pubkey,
            lamports,
            blockhash,
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

    fn setup_vote_and_stake_accounts(
        client: &ThinClient,
        vote_account: &Keypair,
        from_account: &Arc<Keypair>,
        amount: u64,
    ) -> Result<()> {
        let vote_account_pubkey = vote_account.pubkey();
        let node_pubkey = from_account.pubkey();
        let stake_account_keypair = Keypair::new();
        let stake_account_pubkey = stake_account_keypair.pubkey();

        // Create the vote account if necessary
        if client.poll_get_balance(&vote_account_pubkey).unwrap_or(0) == 0 {
            // 1) Create vote account

            let mut transaction = Transaction::new_signed_instructions(
                &[from_account.as_ref()],
                vote_instruction::create_account(
                    &from_account.pubkey(),
                    &vote_account_pubkey,
                    &VoteInit {
                        node_pubkey,
                        authorized_voter: vote_account_pubkey,
                        authorized_withdrawer: vote_account_pubkey,
                        commission: 0,
                    },
                    amount,
                ),
                client.get_recent_blockhash().unwrap().0,
            );
            client
                .retry_transfer(&from_account, &mut transaction, 5)
                .expect("fund vote");
            client
                .wait_for_balance(&vote_account_pubkey, Some(amount))
                .expect("get balance");

            let mut transaction = Transaction::new_signed_instructions(
                &[from_account.as_ref(), &stake_account_keypair],
                stake_instruction::create_stake_account_and_delegate_stake(
                    &from_account.pubkey(),
                    &stake_account_pubkey,
                    &vote_account_pubkey,
                    &StakeAuthorized::auto(&stake_account_pubkey),
                    amount,
                ),
                client.get_recent_blockhash().unwrap().0,
            );

            client
                .send_and_confirm_transaction(
                    &[from_account.as_ref(), &stake_account_keypair],
                    &mut transaction,
                    5,
                    0,
                )
                .expect("delegate stake");
            client
                .wait_for_balance(&stake_account_pubkey, Some(amount))
                .expect("get balance");
        } else {
            warn!(
                "{} vote_account already has a balance?!?",
                vote_account_pubkey
            );
        }
        info!("Checking for vote account registration of {}", node_pubkey);
        match (
            client.get_account(&stake_account_pubkey),
            client.get_account(&vote_account_pubkey),
        ) {
            (Ok(Some(stake_account)), Ok(Some(vote_account))) => {
                match (
                    StakeState::stake_from(&stake_account),
                    VoteState::from(&vote_account),
                ) {
                    (Some(stake_state), Some(vote_state)) => {
                        if stake_state.voter_pubkey != vote_account_pubkey
                            || stake_state.stake != amount
                        {
                            Err(Error::new(ErrorKind::Other, "invalid stake account state"))
                        } else if vote_state.node_pubkey != node_pubkey {
                            Err(Error::new(ErrorKind::Other, "invalid vote account state"))
                        } else {
                            info!("node {} {:?} {:?}", node_pubkey, stake_state, vote_state);

                            Ok(())
                        }
                    }
                    (None, _) => Err(Error::new(ErrorKind::Other, "invalid stake account data")),
                    (_, None) => Err(Error::new(ErrorKind::Other, "invalid vote account data")),
                }
            }
            (Ok(None), _) | (Err(_), _) => Err(Error::new(
                ErrorKind::Other,
                "unable to retrieve stake account data",
            )),
            (_, Ok(None)) | (_, Err(_)) => Err(Error::new(
                ErrorKind::Other,
                "unable to retrieve vote account data",
            )),
        }
    }

    /// Sets up the storage account for validators/replicators and assumes the funder is the owner
    fn setup_storage_account(
        client: &ThinClient,
        storage_keypair: &Keypair,
        from_keypair: &Arc<Keypair>,
        replicator: bool,
    ) -> Result<()> {
        let storage_account_type = if replicator {
            StorageAccountType::Replicator
        } else {
            StorageAccountType::Validator
        };
        let message = Message::new_with_payer(
            storage_instruction::create_storage_account(
                &from_keypair.pubkey(),
                &from_keypair.pubkey(),
                &storage_keypair.pubkey(),
                1,
                storage_account_type,
            ),
            Some(&from_keypair.pubkey()),
        );
        let signer_keys = vec![from_keypair.as_ref()];
        let blockhash = client.get_recent_blockhash().unwrap().0;
        let mut transaction = Transaction::new(&signer_keys, message, blockhash);
        client
            .retry_transfer(&from_keypair, &mut transaction, 5)
            .map(|_signature| ())
    }
}

impl Cluster for LocalCluster {
    fn get_node_pubkeys(&self) -> Vec<Pubkey> {
        self.fullnodes.keys().cloned().collect()
    }

    fn get_validator_client(&self, pubkey: &Pubkey) -> Option<ThinClient> {
        self.fullnode_infos.get(pubkey).map(|f| {
            create_client(
                f.info.contact_info.client_facing_addr(),
                FULLNODE_PORT_RANGE,
            )
        })
    }

    fn exit_node(&mut self, pubkey: &Pubkey) -> ClusterValidatorInfo {
        let mut node = self.fullnodes.remove(&pubkey).unwrap();

        // Shut down the fullnode
        node.exit();
        node.join().unwrap();

        self.fullnode_infos.remove(&pubkey).unwrap()
    }

    fn restart_node(&mut self, pubkey: &Pubkey, mut cluster_validator_info: ClusterValidatorInfo) {
        // Update the stored ContactInfo for this node
        let node = Node::new_localhost_with_pubkey(&pubkey);
        cluster_validator_info.info.contact_info = node.info.clone();

        let entry_point_info = {
            if *pubkey == self.entry_point_info.id {
                self.entry_point_info = node.info.clone();
                None
            } else {
                Some(&self.entry_point_info)
            }
        };

        // Restart the node
        let fullnode_info = &cluster_validator_info.info;

        let restarted_node = Validator::new(
            node,
            &fullnode_info.keypair,
            &fullnode_info.ledger_path,
            &fullnode_info.voting_keypair.pubkey(),
            &fullnode_info.voting_keypair,
            &fullnode_info.storage_keypair,
            entry_point_info,
            true,
            &cluster_validator_info.config,
        );

        self.fullnodes.insert(*pubkey, restarted_node);
        self.fullnode_infos.insert(*pubkey, cluster_validator_info);
    }

    fn exit_restart_node(&mut self, pubkey: &Pubkey, validator_config: ValidatorConfig) {
        let mut cluster_validator_info = self.exit_node(pubkey);
        cluster_validator_info.config = validator_config;
        self.restart_node(pubkey, cluster_validator_info);
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
    use solana_core::storage_stage::SLOTS_PER_TURN_TEST;
    use solana_runtime::epoch_schedule::MINIMUM_SLOTS_PER_EPOCH;

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
        let mut validator_config = ValidatorConfig::default();
        validator_config.rpc_config.enable_fullnode_exit = true;
        validator_config.storage_slots_per_turn = SLOTS_PER_TURN_TEST;
        const NUM_NODES: usize = 1;
        let num_replicators = 1;
        let config = ClusterConfig {
            validator_configs: vec![ValidatorConfig::default(); NUM_NODES],
            num_replicators,
            node_stakes: vec![3; NUM_NODES],
            cluster_lamports: 100,
            ticks_per_slot: 8,
            slots_per_epoch: MINIMUM_SLOTS_PER_EPOCH as u64,
            ..ClusterConfig::default()
        };
        let cluster = LocalCluster::new(&config);
        assert_eq!(cluster.fullnodes.len(), NUM_NODES);
        assert_eq!(cluster.replicators.len(), num_replicators);
    }
}
