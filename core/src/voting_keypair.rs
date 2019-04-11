//! The `vote_signer_proxy` votes on the `blockhash` of the bank at a regular cadence

use jsonrpc_core;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_request::RpcRequest;
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
        let rpc_client = RpcClient::new_socket(signer);
        Self { rpc_client }
    }
}

impl VoteSigner for RemoteVoteSigner {
    fn register(
        &self,
        pubkey: &Pubkey,
        sig: &Signature,
        msg: &[u8],
    ) -> jsonrpc_core::Result<Pubkey> {
        let params = json!([pubkey, sig, msg]);
        let resp = self
            .rpc_client
            .retry_make_rpc_request(&RpcRequest::RegisterNode, Some(params), 5)
            .unwrap();
        let vote_account: Pubkey = serde_json::from_value(resp).unwrap();
        Ok(vote_account)
    }
    fn sign(
        &self,
        pubkey: &Pubkey,
        sig: &Signature,
        msg: &[u8],
    ) -> jsonrpc_core::Result<Signature> {
        let params = json!([pubkey, sig, msg]);
        let resp = self
            .rpc_client
            .retry_make_rpc_request(&RpcRequest::SignVote, Some(params), 0)
            .unwrap();
        let vote_signature: Signature = serde_json::from_value(resp).unwrap();
        Ok(vote_signature)
    }
    fn deregister(&self, pubkey: &Pubkey, sig: &Signature, msg: &[u8]) -> jsonrpc_core::Result<()> {
        let params = json!([pubkey, sig, msg]);
        let _resp = self
            .rpc_client
            .retry_make_rpc_request(&RpcRequest::DeregisterNode, Some(params), 5)
            .unwrap();
        Ok(())
    }
}

impl KeypairUtil for VotingKeypair {
    /// Return a local VotingKeypair with a new keypair. Used for unit-tests.
    fn new() -> Self {
        Self::new_with_signer(
            &Arc::new(Keypair::new()),
            Box::new(LocalVoteSigner::default()),
        )
    }

    /// Return the public key of the keypair used to sign votes
    fn pubkey(&self) -> Pubkey {
        self.vote_account
    }

    fn sign_message(&self, msg: &[u8]) -> Signature {
        let sig = self.keypair.sign_message(msg);
        self.signer
            .sign(&self.keypair.pubkey(), &sig, &msg)
            .unwrap()
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
            .register(&keypair.pubkey(), &sig, msg.as_bytes())
            .unwrap();
        Self {
            keypair: keypair.clone(),
            signer,
            vote_account,
        }
    }
}

#[cfg(test)]
pub mod tests {
    use solana_runtime::bank::Bank;
    use solana_sdk::instruction::Instruction;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;
    use solana_vote_api::vote_instruction;
    use solana_vote_api::vote_state::Vote;

    fn process_instructions<T: KeypairUtil>(bank: &Bank, keypairs: &[&T], ixs: Vec<Instruction>) {
        let blockhash = bank.last_blockhash();
        let tx = Transaction::new_signed_instructions(keypairs, ixs, blockhash);
        bank.process_transaction(&tx).unwrap();
    }

    pub fn new_vote_account(
        from_keypair: &Keypair,
        voting_keypair: &Keypair,
        node_id: &Pubkey,
        bank: &Bank,
        lamports: u64,
    ) {
        let voting_pubkey = voting_keypair.pubkey();
        let ixs = vote_instruction::create_account(
            &from_keypair.pubkey(),
            &voting_pubkey,
            node_id,
            0,
            lamports,
        );
        process_instructions(bank, &[from_keypair], ixs);
    }

    pub fn push_vote<T: KeypairUtil>(voting_keypair: &T, bank: &Bank, slot: u64) {
        let ix = vote_instruction::vote(&voting_keypair.pubkey(), vec![Vote::new(slot)]);
        process_instructions(bank, &[voting_keypair], vec![ix]);
    }

    pub fn new_vote_account_with_vote<T: KeypairUtil>(
        from_keypair: &T,
        voting_keypair: &T,
        node_id: &Pubkey,
        bank: &Bank,
        lamports: u64,
        slot: u64,
    ) {
        let voting_pubkey = voting_keypair.pubkey();
        let mut ixs = vote_instruction::create_account(
            &from_keypair.pubkey(),
            &voting_pubkey,
            node_id,
            0,
            lamports,
        );
        ixs.push(vote_instruction::vote(
            &voting_pubkey,
            vec![Vote::new(slot)],
        ));
        process_instructions(bank, &[from_keypair, voting_keypair], ixs);
    }
}
