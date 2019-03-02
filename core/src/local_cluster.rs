use crate::blocktree::{create_new_tmp_ledger, tmp_copy_blocktree};
use crate::client::mk_client;
use crate::cluster_info::{Node, NodeInfo};
use crate::fullnode::{Fullnode, FullnodeConfig};
use crate::gossip_service::discover;
use crate::thin_client::retry_get_balance;
use crate::thin_client::ThinClient;
use crate::voting_keypair::VotingKeypair;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use solana_vote_api::vote_state::VoteState;
use solana_vote_api::vote_transaction::VoteTransaction;
use std::fs::remove_dir_all;
use std::io::{Error, ErrorKind, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

pub struct LocalCluster {
    /// Keypair with funding to particpiate in the network
    pub funding_keypair: Keypair,
    /// Entry point from which the rest of the network can be discovered
    pub entry_point_info: NodeInfo,
    fullnode_hdls: Vec<(JoinHandle<()>, Arc<AtomicBool>)>,
    ledger_paths: Vec<String>,
}

impl LocalCluster {
    pub fn new(num_nodes: usize, cluster_lamports: u64, lamports_per_node: u64) -> Self {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let (genesis_block, mint_keypair) =
            GenesisBlock::new_with_leader(cluster_lamports, leader_pubkey, lamports_per_node);
        let (genesis_ledger_path, _blockhash) = create_new_tmp_ledger!(&genesis_block);
        let leader_ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
        let mut ledger_paths = vec![];
        ledger_paths.push(genesis_ledger_path.clone());
        ledger_paths.push(leader_ledger_path.clone());
        let voting_keypair = VotingKeypair::new_local(&leader_keypair);
        let fullnode_config = FullnodeConfig::default();
        let leader_node_info = leader_node.info.clone();
        let leader_server = Fullnode::new(
            leader_node,
            &leader_keypair,
            &leader_ledger_path,
            voting_keypair,
            None,
            &fullnode_config,
        );
        let (thread, exit, _) = leader_server.start(None);
        let mut fullnode_hdls = vec![(thread, exit)];
        let mut client = mk_client(&leader_node_info);
        for _ in 0..(num_nodes - 1) {
            let validator_keypair = Arc::new(Keypair::new());
            let voting_keypair = VotingKeypair::new_local(&validator_keypair);
            let validator_pubkey = validator_keypair.pubkey();
            let validator_node = Node::new_localhost_with_pubkey(validator_keypair.pubkey());
            let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
            ledger_paths.push(ledger_path.clone());

            // Send each validator some tokens to vote
            let validator_balance = Self::transfer(
                &mut client,
                &mint_keypair,
                &validator_pubkey,
                lamports_per_node,
            );
            info!(
                "validator {} balance {}",
                validator_pubkey, validator_balance
            );

            Self::create_and_fund_vote_account(
                &mut client,
                voting_keypair.pubkey(),
                &validator_keypair,
                1,
            )
            .unwrap();
            let validator_server = Fullnode::new(
                validator_node,
                &validator_keypair,
                &ledger_path,
                voting_keypair,
                Some(&leader_node_info),
                &fullnode_config,
            );
            let (thread, exit, _) = validator_server.start(None);
            fullnode_hdls.push((thread, exit));
        }
        discover(&leader_node_info, num_nodes);
        Self {
            funding_keypair: mint_keypair,
            entry_point_info: leader_node_info,
            fullnode_hdls,
            ledger_paths,
        }
    }

    pub fn exit(&self) {
        for node in &self.fullnode_hdls {
            node.1.store(true, Ordering::Relaxed);
        }
    }
    pub fn close(&mut self) {
        self.exit();
        while let Some(node) = self.fullnode_hdls.pop() {
            node.0.join().expect("join");
        }
        for path in &self.ledger_paths {
            remove_dir_all(path).unwrap();
        }
    }

    fn transfer(
        client: &mut ThinClient,
        source_keypair: &Keypair,
        dest_pubkey: &Pubkey,
        lamports: u64,
    ) -> u64 {
        trace!("getting leader blockhash");
        let blockhash = client.get_recent_blockhash();
        let mut tx =
            SystemTransaction::new_account(&source_keypair, *dest_pubkey, lamports, blockhash, 0);
        info!(
            "executing transfer of {} from {} to {}",
            lamports,
            source_keypair.pubkey(),
            *dest_pubkey
        );
        client
            .retry_transfer(&source_keypair, &mut tx, 5)
            .expect("client transfer");
        retry_get_balance(client, dest_pubkey, Some(lamports)).expect("get balance")
    }

    fn create_and_fund_vote_account(
        client: &mut ThinClient,
        vote_account: Pubkey,
        from_account: &Arc<Keypair>,
        amount: u64,
    ) -> Result<()> {
        // Create the vote account if necessary
        if client.poll_get_balance(&vote_account).unwrap_or(0) == 0 {
            let mut transaction = VoteTransaction::fund_staking_account(
                from_account,
                vote_account,
                client.get_recent_blockhash(),
                amount,
                1,
            );

            client
                .retry_transfer(&from_account, &mut transaction, 5)
                .expect("client transfer");
            retry_get_balance(client, &vote_account, Some(amount)).expect("get balance");
        }

        info!("Checking for vote account registration");
        let vote_account_user_data = client.get_account_userdata(&vote_account);
        if let Ok(Some(vote_account_user_data)) = vote_account_user_data {
            if let Ok(vote_state) = VoteState::deserialize(&vote_account_user_data) {
                if vote_state.delegate_id == from_account.pubkey() {
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

impl Drop for LocalCluster {
    fn drop(&mut self) {
        self.close()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_local_cluster_start_and_exit() {
        solana_logger::setup();
        let network = LocalCluster::new(1, 100, 3);
        drop(network)
    }
}
