use crdt::{NodeInfo, SOLANA_PORT_RANGE};
use nat::bind_in_range;
use std::time::Duration;
use thin_client::ThinClient;

pub fn mk_client(r: &NodeInfo) -> ThinClient {
    let requests_socket = bind_in_range(SOLANA_PORT_RANGE).unwrap();
    let transactions_socket = bind_in_range(SOLANA_PORT_RANGE).unwrap();

    requests_socket
        .set_read_timeout(Some(Duration::new(1, 0)))
        .unwrap();

    ThinClient::new(
        r.contact_info.rpu,
        requests_socket,
        r.contact_info.tpu,
        transactions_socket,
    )
}
