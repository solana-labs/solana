use {
    log::{error, info},
    solana_client::connection_cache::ConnectionCache as ClientConnectionCache,
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{pubkey::Pubkey, signature::Signer, signer::keypair::Keypair},
    solana_streamer::streamer::StakedNodes,
    std::{
        collections::HashMap,
        net::IpAddr,
        sync::{Arc, RwLock},
    },
};

/// Request information about node's stake
/// If fail to get requested information, return error
/// Otherwise return stake of the node
/// along with total activated stake of the network
fn find_node_activated_stake(
    rpc_client: Arc<RpcClient>,
    node_id: Pubkey,
) -> Result<(u64, u64), ()> {
    let vote_accounts = rpc_client.get_vote_accounts();
    if let Err(error) = vote_accounts {
        error!("Failed to get vote accounts, error: {}", error);
        return Err(());
    }

    let vote_accounts = vote_accounts.unwrap();

    let total_active_stake: u64 = vote_accounts
        .current
        .iter()
        .map(|vote_account| vote_account.activated_stake)
        .sum();

    let node_id_as_str = node_id.to_string();
    let find_result = vote_accounts
        .current
        .iter()
        .find(|&vote_account| vote_account.node_pubkey == node_id_as_str);
    match find_result {
        Some(value) => Ok((value.activated_stake, total_active_stake)),
        None => {
            error!("Failed to find stake for requested node");
            Err(())
        }
    }
}

pub fn create_connection_cache(
    tpu_connection_pool_size: usize,
    use_quic: bool,
    bind_address: IpAddr,
    client_node_id: Option<&Keypair>,
    rpc_client: Arc<RpcClient>,
) -> ClientConnectionCache {
    if !use_quic {
        return ClientConnectionCache::with_udp(
            "solana-tps-connection_cache_udp",
            tpu_connection_pool_size,
        );
    }
    if client_node_id.is_none() {
        return ClientConnectionCache::new_quic(
            "solana-tps-connection_cache_quic",
            tpu_connection_pool_size,
        );
    }

    let client_node_id = client_node_id.unwrap();
    let (stake, total_stake) =
        find_node_activated_stake(rpc_client, client_node_id.pubkey()).unwrap_or_default();
    info!("Stake for specified client_node_id: {stake}, total stake: {total_stake}");
    let stakes = HashMap::from([
        (client_node_id.pubkey(), stake),
        (
            Pubkey::new_unique(),
            total_stake.checked_sub(stake).unwrap(),
        ),
    ]);
    let staked_nodes = Arc::new(RwLock::new(StakedNodes::new(
        Arc::new(stakes),
        HashMap::<Pubkey, u64>::default(), // overrides
    )));
    ClientConnectionCache::new_with_client_options(
        "solana-tps-connection_cache_quic",
        tpu_connection_pool_size,
        None,
        Some((client_node_id, bind_address)),
        Some((&staked_nodes, &client_node_id.pubkey())),
    )
}
