use log::*;
use solana_client::rpc_response::{RpcContactInfo, RpcLeaderSchedule};
use std::net::{SocketAddr, UdpSocket};

pub fn get_leader_tpu(
    slot_index: u64,
    leader_schedule: Option<&RpcLeaderSchedule>,
    cluster_nodes: Option<&Vec<RpcContactInfo>>,
) -> Option<SocketAddr> {
    leader_schedule?
        .iter()
        .find(|(_pubkey, slots)| slots.iter().any(|slot| *slot as u64 == slot_index))
        .and_then(|(pubkey, _)| {
            cluster_nodes?
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
