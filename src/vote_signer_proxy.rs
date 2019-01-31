//! The `vote_signer_proxy` votes on the `last_id` of the bank at a regular cadence

use crate::bank::Bank;
use crate::jsonrpc_core;
use crate::result::Result;
use crate::rpc_request::{RpcClient, RpcRequest};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_transaction::VoteTransaction;
use solana_vote_signer::rpc::LocalVoteSigner;
use solana_vote_signer::rpc::VoteSigner;
use std::net::SocketAddr;
use std::sync::Arc;

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
        let resp = self
            .rpc_client
            .retry_make_rpc_request(1, &RpcRequest::RegisterNode, Some(params), 5)
            .unwrap();
        let vote_account: Pubkey = serde_json::from_value(resp).unwrap();
        Ok(vote_account)
    }
    fn sign(&self, pubkey: Pubkey, sig: &Signature, msg: &[u8]) -> jsonrpc_core::Result<Signature> {
        let params = json!([pubkey, sig, msg]);
        let resp = self
            .rpc_client
            .retry_make_rpc_request(1, &RpcRequest::SignVote, Some(params), 0)
            .unwrap();
        let vote_signature: Signature = serde_json::from_value(resp).unwrap();
        Ok(vote_signature)
    }
    fn deregister(&self, pubkey: Pubkey, sig: &Signature, msg: &[u8]) -> jsonrpc_core::Result<()> {
        let params = json!([pubkey, sig, msg]);
        let _resp = self
            .rpc_client
            .retry_make_rpc_request(1, &RpcRequest::DeregisterNode, Some(params), 5)
            .unwrap();
        Ok(())
    }
}

impl KeypairUtil for VoteSignerProxy {
    /// Return a local VoteSignerProxy with a new keypair. Used for unit-tests.
    fn new() -> Self {
        Self::new_local(&Arc::new(Keypair::new()))
    }

    /// Return the public key of the keypair used to sign votes
    fn pubkey(&self) -> Pubkey {
        self.vote_account
    }

    fn sign_message(&self, msg: &[u8]) -> Signature {
        let sig = self.keypair.sign_message(msg);
        self.signer.sign(self.keypair.pubkey(), &sig, &msg).unwrap()
    }
}

pub struct VoteSignerProxy {
    keypair: Arc<Keypair>,
    signer: Box<VoteSigner + Send + Sync>,
    vote_account: Pubkey,
}

impl VoteSignerProxy {
    pub fn new_with_signer(keypair: &Arc<Keypair>, signer: Box<VoteSigner + Send + Sync>) -> Self {
        let msg = "Registering a new node";
        let sig = keypair.sign_message(msg.as_bytes());
        let vote_account = signer
            .register(keypair.pubkey(), &sig, msg.as_bytes())
            .unwrap();
        Self {
            keypair: keypair.clone(),
            signer,
            vote_account,
        }
    }

    pub fn new_local(keypair: &Arc<Keypair>) -> Self {
        Self::new_with_signer(keypair, Box::new(LocalVoteSigner::default()))
    }

    pub fn new_vote_account(&self, bank: &Bank, num_tokens: u64, last_id: Hash) -> Result<()> {
        // Create and register the new vote account
        let tx =
            Transaction::vote_account_new(&self.keypair, self.vote_account, last_id, num_tokens, 0);
        bank.process_transaction(&tx)?;
        Ok(())
    }

    pub fn validator_vote(&self, bank: &Arc<Bank>) -> Transaction {
        Transaction::vote_new(self, bank.tick_height(), bank.last_id(), 0)
    }
}

#[cfg(test)]
mod test {
    //TODO simple tests that cover the signing
}
