use crate::thin_client::ThinClient;
use std::net::SocketAddr;
use std::time::Duration;

pub fn create_client((rpc, tpu): (SocketAddr, SocketAddr), range: (u16, u16)) -> ThinClient {
    let (_, transactions_socket) = solana_netutil::bind_in_range(range).unwrap();
    ThinClient::new(rpc, tpu, transactions_socket)
}

pub fn create_client_with_timeout(
    (rpc, tpu): (SocketAddr, SocketAddr),
    range: (u16, u16),
    timeout: Duration,
) -> ThinClient {
    let (_, transactions_socket) = solana_netutil::bind_in_range(range).unwrap();
    ThinClient::new_with_timeout(rpc, tpu, transactions_socket, timeout)
}
