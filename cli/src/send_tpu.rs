use log::*;
use solana_client::rpc_response::{RpcContactInfo, RpcLeaderSchedule};
use solana_sdk::clock::NUM_CONSECUTIVE_LEADER_SLOTS;
use std::net::{SocketAddr, UdpSocket};

pub fn get_leader_tpus(
    slot_index: u64,
    num_leaders: u64,
    leader_schedule: Option<&RpcLeaderSchedule>,
    cluster_nodes: Option<&Vec<RpcContactInfo>>,
) -> Vec<SocketAddr> {
    let leaders: Vec<_> = (0..num_leaders)
        .filter_map(|i| {
            leader_schedule?
                .iter()
                .find(|(_pubkey, slots)| {
                    slots.iter().any(|slot| {
                        *slot as u64 == (slot_index + (i * NUM_CONSECUTIVE_LEADER_SLOTS))
                    })
                })
                .and_then(|(pubkey, _)| {
                    cluster_nodes?
                        .iter()
                        .find(|contact_info| contact_info.pubkey == *pubkey)
                        .and_then(|contact_info| contact_info.tpu)
                })
        })
        .collect();
    let mut unique_leaders = vec![];
    for leader in leaders.into_iter() {
        if !unique_leaders.contains(&leader) {
            unique_leaders.push(leader);
        }
    }
    unique_leaders
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
