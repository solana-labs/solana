use crate::bank::Bank;
use crate::cluster_info::FULLNODE_PORT_RANGE;
use crate::netutil::find_available_port_in_range;
use crate::result::Result;
use crate::rpc_request::{RpcClient, RpcRequest};
use solana_sdk::hash::Hash;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, KeypairUtil, Signature};
use solana_sdk::transaction::Transaction;
use solana_sdk::vote_transaction::*;
use solana_vote_signer::rpc::VoteSignerRpcService;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{Builder, JoinHandle};

pub fn local_vote_signer_service() -> io::Result<(SocketAddr, JoinHandle<()>, Arc<AtomicBool>)> {
    let addr = match find_available_port_in_range(FULLNODE_PORT_RANGE) {
        Ok(port) => SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port),
        Err(e) => return Err(e),
    };
    let service_addr = addr.clone();
    let exit = Arc::new(AtomicBool::new(false));
    let thread_exit = exit.clone();
    let thread = Builder::new()
        .name("solana-vote-signer".to_string())
        .spawn(move || {
            let service = VoteSignerRpcService::new(service_addr, thread_exit);
            service.join().unwrap();
            ()
        })
        .unwrap();
    Ok((addr, thread, exit))
}

pub fn stop_local_vote_signer_service(t: JoinHandle<()>, exit: &Arc<AtomicBool>) {
    exit.store(true, Ordering::Relaxed);
    t.join().unwrap();
}

pub fn create_vote_account(
    node_keypair: &Keypair,
    bank: &Bank,
    num_tokens: u64,
    last_id: Hash,
    rpc_client: &RpcClient,
) -> Result<Pubkey> {
    let msg = "Registering a new node";
    let sig = Signature::new(&node_keypair.sign(msg.as_bytes()).as_ref());
    let params = json!([node_keypair.pubkey().to_string(), sig, msg.as_bytes()]);
    let resp = RpcRequest::RegisterNode
        .make_rpc_request(&rpc_client, 1, Some(params))
        .unwrap();
    let new_vote_account: Pubkey = serde_json::from_value(resp).unwrap();

    // Create and register the new vote account
    let tx = Transaction::vote_account_new(node_keypair, new_vote_account, last_id, num_tokens, 0);
    bank.process_transaction(&tx)?;

    Ok(new_vote_account)
}
