//! The `vote_stage` votes on the `last_id` of the bank at a regular cadence

use crate::bank::Bank;
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::packet::SharedBlob;
use crate::result::{Error, Result};
use crate::streamer::BlobSender;
use bincode::serialize;
use log::Level;
use packet::SharedBlob;
use result::{Error, Result};
use rpc_request::{RpcClient, RpcRequest};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_program::Vote;
use solana_sdk::vote_transaction::VoteTransaction;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};

#[derive(Debug, PartialEq, Eq)]
pub enum VoteError {
    NoValidSupermajority,
    NoLeader,
    LeaderInfoNotFound,
}

// TODO: Change voting to be on fixed tick intervals based on bank state
pub fn create_new_signed_vote_blob(
    last_id: &Hash,
    keypair: &Arc<Keypair>,
    vote_account: &Pubkey,
    rpc_client: &RpcClient,
    bank: &Arc<Bank>,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
) -> Result<SharedBlob> {
    let shared_blob = SharedBlob::default();
    let tick_height = bank.tick_height();

    let leader_tpu = get_leader_tpu(&bank, cluster_info)?;
    //TODO: doesn't seem like there is a synchronous call to get height and id
    debug!("voting on {:?}", &last_id.as_ref()[..8]);
    let vote = Vote { tick_height };
    let tx = Transaction::vote_new(&vote_account, vote, *last_id, 0);

    let msg = tx.get_sign_data();
    let sig = Signature::new(&keypair.sign(&msg).as_ref());

    let params = json!([keypair.pubkey().to_string(), sig, &msg]);
    let resp = RpcRequest::SignVote
        .make_rpc_request(&rpc_client, 1, Some(params))
        .unwrap();
    let vote_signature: Signature = serde_json::from_value(resp).unwrap();

    let signed_tx = Transaction {
        signatures: vec![vote_signature],
        account_keys: tx.account_keys,
        last_id: tx.last_id,
        fee: tx.fee,
        program_ids: tx.program_ids,
        instructions: tx.instructions,
    };

    {
        let mut blob = shared_blob.write().unwrap();
        let bytes = serialize(&signed_tx)?;
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

pub fn send_validator_vote(
    bank: &Arc<Bank>,
    keypair: &Arc<Keypair>,
    vote_account: &Pubkey,
    vote_signer_rpc: &RpcClient,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    vote_blob_sender: &BlobSender,
) -> Result<()> {
    let last_id = bank.last_id();

    if let Ok(shared_blob) =
        create_new_signed_vote_blob(&last_id, vote_account, vote_signer_rpc, bank, cluster_info)
    {
        inc_new_counter_info!("validator-vote_sent", 1);
        vote_blob_sender.send(vec![shared_blob])?;
    }

    Ok(())
}
