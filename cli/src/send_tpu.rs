use log::*;
use solana_client::{rpc_client::RpcClient, rpc_response::RpcLeaderSchedule};
use std::net::{SocketAddr, UdpSocket};

pub fn get_leader_tpu(
    rpc_client: &RpcClient,
    slot_index: u64,
    leader_schedule: Option<&RpcLeaderSchedule>,
) -> Option<SocketAddr> {
    let leader_schedule = leader_schedule?;
    leader_schedule
        .iter()
        .find(|(_pubkey, slots)| slots.iter().any(|slot| *slot as u64 == slot_index))
        .and_then(|(pubkey, _)| {
            let cluster_nodes = rpc_client.get_cluster_nodes().ok()?;
            cluster_nodes
                .iter()
                .find(|contact_info| contact_info.pubkey == *pubkey)
                .and_then(|contact_info| contact_info.tpu)
        })
}

pub fn send_transaction_tpu(
    send_socket: &UdpSocket,
    tpu_address: &SocketAddr,
    wire_transaction: &[u8],
) {
    if let Err(err) = send_socket.send_to(wire_transaction, tpu_address) {
        warn!("Failed to send transaction to {}: {:?}", tpu_address, err);
    }
}
