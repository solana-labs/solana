use crate::blocktree::{create_new_tmp_ledger, tmp_copy_blocktree};
use crate::client::mk_client;
use crate::cluster_info::{Node, NodeInfo};
use crate::fullnode::{Fullnode, FullnodeConfig};
use crate::gossip_service::converge;
use crate::thin_client::retry_get_balance;
use crate::voting_keypair::VotingKeypair;
use solana_sdk::genesis_block::GenesisBlock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil};
use solana_sdk::system_transaction::SystemTransaction;
use std::fs::remove_dir_all;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

pub struct LocalCluster {
    pub mint: Keypair,
    pub contact_info: NodeInfo,
    network_nodes: Vec<(JoinHandle<()>, Arc<AtomicBool>)>,
    ledger_paths: Vec<String>,
}

impl LocalCluster {
    pub fn create_network(num_nodes: usize, network_tokens: u64, node_tokens: u64) -> Self {
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey().clone();
        let leader_node = Node::new_localhost_with_pubkey(leader_keypair.pubkey());
        let (genesis_block, mint) =
            GenesisBlock::new_with_leader(network_tokens, leader_pubkey, node_tokens);
        let (genesis_ledger_path, _last_id) = create_new_tmp_ledger!(&genesis_block);
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
        let mut network_nodes = vec![(thread, exit)];
        for _ in 0..(num_nodes - 1) {
            let keypair = Arc::new(Keypair::new());
            let validator_pubkey = keypair.pubkey().clone();
            let validator_node = Node::new_localhost_with_pubkey(keypair.pubkey());
            let ledger_path = tmp_copy_blocktree!(&genesis_ledger_path);
            ledger_paths.push(ledger_path.clone());

            // Send each validator some tokens to vote
            let validator_balance = Self::send_tx_and_retry_get_balance(
                &leader_node_info,
                &mint,
                &validator_pubkey,
                node_tokens,
                None,
            )
            .unwrap();
            info!(
                "validator {} balance {}",
                validator_pubkey, validator_balance
            );

            let voting_keypair = VotingKeypair::new_local(&keypair);
            let validator_server = Fullnode::new(
                validator_node,
                &keypair,
                &ledger_path,
                voting_keypair,
                Some(&leader_node_info),
                &FullnodeConfig::default(),
            );
            let (thread, exit, _) = validator_server.start(None);
            network_nodes.push((thread, exit));
        }
        converge(&leader_node_info, num_nodes);
        Self {
            mint,
            contact_info: leader_node_info,
            network_nodes,
            ledger_paths,
        }
    }

    pub fn exit(&self) {
        for node in &self.network_nodes {
            node.1.store(true, Ordering::Relaxed);
        }
    }
    pub fn close(&mut self) {
        self.exit();
        while let Some(node) = self.network_nodes.pop() {
            node.0.join().expect("join");
        }
        for path in &self.ledger_paths {
            remove_dir_all(path).unwrap();
        }
    }

    fn send_tx_and_retry_get_balance(
        leader: &NodeInfo,
        alice: &Keypair,
        bob_pubkey: &Pubkey,
        transfer_amount: u64,
        expected: Option<u64>,
    ) -> Option<u64> {
        let mut client = mk_client(leader);
        trace!("getting leader last_id");
        let last_id = client.get_last_id();
        let mut tx =
            SystemTransaction::new_account(&alice, *bob_pubkey, transfer_amount, last_id, 0);
        info!(
            "executing transfer of {} from {} to {}",
            transfer_amount,
            alice.pubkey(),
            *bob_pubkey
        );
        if client.retry_transfer(&alice, &mut tx, 5).is_err() {
            None
        } else {
            retry_get_balance(&mut client, bob_pubkey, expected)
        }
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
        let network = LocalCluster::create_network(1, 100, 2);
        drop(network)
    }
}
