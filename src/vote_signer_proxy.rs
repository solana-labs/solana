//! The `vote_signer_proxy` votes on the `last_id` of the bank at a regular cadence

use crate::bank::Bank;
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::jsonrpc_core;
use crate::packet::SharedBlob;
use crate::result::{Error, Result};
use crate::rpc_request::{RpcClient, RpcRequest, RpcRequestHandler};
use crate::streamer::BlobSender;
use bincode::serialize;
use log::Level;
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program::Vote;
use solana_sdk::vote_transaction::VoteTransaction;
use solana_vote_signer::rpc::VoteSigner;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

#[derive(Debug, PartialEq, Eq)]
pub enum VoteError {
    NoValidSupermajority,
    NoLeader,
    LeaderInfoNotFound,
}

pub struct RemoteVoteSigner {
    rpc_client: RpcClient,
}

impl RemoteVoteSigner {
    pub fn new(signer: SocketAddr) -> Self {
        Self {
            rpc_client: RpcClient::new_from_socket(signer),
        }
    }
}

impl VoteSigner for RemoteVoteSigner {
    fn register(
        &self,
        pubkey: Pubkey,
        sig: &Signature,
        msg: &[u8],
    ) -> jsonrpc_core::Result<Pubkey> {
        let params = json!([pubkey, sig, msg]);
        let resp = RpcRequest::RegisterNode
            .retry_make_rpc_request(&self.rpc_client, 1, Some(params), 5)
            .unwrap();
        let vote_account: Pubkey = serde_json::from_value(resp).unwrap();
        Ok(vote_account)
    }
    fn sign(&self, pubkey: Pubkey, sig: &Signature, msg: &[u8]) -> jsonrpc_core::Result<Signature> {
        let params = json!([pubkey, sig, msg]);
        let resp = RpcRequest::SignVote
            .make_rpc_request(&self.rpc_client, 1, Some(params))
            .unwrap();
        let vote_signature: Signature = serde_json::from_value(resp).unwrap();
        Ok(vote_signature)
    }
    fn deregister(&self, pubkey: Pubkey, sig: &Signature, msg: &[u8]) -> jsonrpc_core::Result<()> {
        let params = json!([pubkey, sig, msg]);
        let _resp = RpcRequest::DeregisterNode
            .retry_make_rpc_request(&self.rpc_client, 1, Some(params), 5)
            .unwrap();
        Ok(())
    }
}

pub struct VoteSignerProxy {
    keypair: Arc<Keypair>,
    signer: Box<VoteSigner + Send + Sync>,
    pub vote_account: Pubkey,
}

impl VoteSignerProxy {
    pub fn new(keypair: &Arc<Keypair>, signer: Box<VoteSigner + Send + Sync>) -> Self {
        let msg = "Registering a new node";
        let sig = Signature::new(&keypair.sign(msg.as_bytes()).as_ref());
        let vote_account = signer
            .register(keypair.pubkey(), &sig, msg.as_bytes())
            .unwrap();
        Self {
            keypair: keypair.clone(),
            signer,
            vote_account,
        }
    }

    pub fn new_vote_account(&self, bank: &Bank, num_tokens: u64, last_id: Hash) -> Result<()> {
        // Create and register the new vote account
        let tx =
            Transaction::vote_account_new(&self.keypair, self.vote_account, last_id, num_tokens, 0);
        bank.process_transaction(&tx)?;
        Ok(())
    }

    pub fn send_validator_vote(
        &self,
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        vote_blob_sender: &BlobSender,
    ) -> Result<()> {
        let last_id = bank.last_id();

        if let Ok(shared_blob) = self.new_signed_vote_blob(&last_id, bank, cluster_info) {
            inc_new_counter_info!("validator-vote_sent", 1);
            vote_blob_sender.send(vec![shared_blob])?;
        }

        Ok(())
    }

    pub fn new_signed_vote_transaction(&self, last_id: &Hash, tick_height: u64) -> Transaction {
        let vote = Vote { tick_height };
        let tx = Transaction::vote_new(&self.vote_account, vote, *last_id, 0);

        let msg = tx.get_sign_data();
        let sig = Signature::new(&self.keypair.sign(&msg).as_ref());

        let keypair = self.keypair.clone();
        let vote_signature = self.signer.sign(keypair.pubkey(), &sig, &msg).unwrap();
        Transaction {
            signatures: vec![vote_signature],
            account_keys: tx.account_keys,
            last_id: tx.last_id,
            fee: tx.fee,
            program_ids: tx.program_ids,
            instructions: tx.instructions,
        }
    }

    // TODO: Change voting to be on fixed tick intervals based on bank state
    fn new_signed_vote_blob(
        &self,
        last_id: &Hash,
        bank: &Arc<Bank>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
    ) -> Result<SharedBlob> {
        let shared_blob = SharedBlob::default();
        let tick_height = bank.tick_height();

        let leader_tpu = VoteSignerProxy::get_leader_tpu(&bank, cluster_info)?;
        //TODO: doesn't seem like there is a synchronous call to get height and id
        debug!("voting on {:?}", &last_id.as_ref()[..8]);
        let tx = self.new_signed_vote_transaction(last_id, tick_height);

        {
            let mut blob = shared_blob.write().unwrap();
            let bytes = serialize(&tx)?;
            let len = bytes.len();
            blob.data[..len].copy_from_slice(&bytes);
            blob.meta.set_addr(&leader_tpu);
            blob.meta.size = len;
        };

        Ok(shared_blob)
    }

    fn get_leader_tpu(bank: &Bank, cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<SocketAddr> {
        let leader_id = match bank.get_current_leader() {
            Some((leader_id, _)) => leader_id,
            None => return Err(Error::VoteError(VoteError::NoLeader)),
        };

        let rcluster_info = cluster_info.read().unwrap();
        let leader_tpu = rcluster_info.lookup(leader_id).map(|leader| leader.tpu);
        if let Some(leader_tpu) = leader_tpu {
            Ok(leader_tpu)
        } else {
            Err(Error::VoteError(VoteError::LeaderInfoNotFound))
        }
    }
}
