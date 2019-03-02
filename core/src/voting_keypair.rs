//! The `vote_signer_proxy` votes on the `last_id` of the bank at a regular cadence

use crate::rpc_request::{RpcClient, RpcRequest};
use jsonrpc_core;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_vote_signer::rpc::LocalVoteSigner;
use solana_vote_signer::rpc::VoteSigner;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct RemoteVoteSigner {
    rpc_client: RpcClient,
}

impl RemoteVoteSigner {
    pub fn new(signer: SocketAddr) -> Self {
        let rpc_client = RpcClient::new_from_socket(signer);
        Self { rpc_client }
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

impl KeypairUtil for VotingKeypair {
    /// Return a local VotingKeypair with a new keypair. Used for unit-tests.
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

pub struct VotingKeypair {
    keypair: Arc<Keypair>,
    signer: Box<VoteSigner + Send + Sync>,
    vote_account: Pubkey,
}

impl VotingKeypair {
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
}

#[cfg(test)]
pub mod tests {
    use solana_runtime::bank::Bank;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::vote_transaction::VoteTransaction;

    pub fn new_vote_account(
        from_keypair: &Keypair,
        voting_pubkey: &Pubkey,
        bank: &Bank,
        num_tokens: u64,
    ) {
        let last_id = bank.last_block_hash();
        let tx = VoteTransaction::fund_staking_account(
            from_keypair,
            *voting_pubkey,
            last_id,
            num_tokens,
            0,
        );
        bank.process_transaction(&tx).unwrap();
    }

    pub fn push_vote<T: KeypairUtil>(voting_keypair: &T, bank: &Bank, slot_height: u64) {
        let last_id = bank.last_block_hash();
        let tx = VoteTransaction::new_vote(voting_keypair, slot_height, last_id, 0);
        bank.process_transaction(&tx).unwrap();
    }

    pub fn new_vote_account_with_vote<T: KeypairUtil>(
        from_keypair: &Keypair,
        voting_keypair: &T,
        bank: &Bank,
        num_tokens: u64,
        slot_height: u64,
    ) {
        new_vote_account(from_keypair, &voting_keypair.pubkey(), bank, num_tokens);
        push_vote(voting_keypair, bank, slot_height);
    }
}
