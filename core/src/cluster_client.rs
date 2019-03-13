use crate::cluster_info::FULLNODE_PORT_RANGE;
use crate::contact_info::ContactInfo;
use solana_client::thin_client::ThinClient;
use std::time::Duration;

pub fn mk_client(r: &ContactInfo) -> ThinClient {
    let (_, transactions_socket) = solana_netutil::bind_in_range(FULLNODE_PORT_RANGE).unwrap();
    ThinClient::new(r.rpc, r.tpu, transactions_socket)
}

pub fn mk_client_with_timeout(r: &ContactInfo, timeout: Duration) -> ThinClient {
    let (_, transactions_socket) = solana_netutil::bind_in_range(FULLNODE_PORT_RANGE).unwrap();
    ThinClient::new_with_timeout(r.rpc, r.tpu, transactions_socket, timeout)
}
