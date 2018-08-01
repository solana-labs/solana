use crdt::NodeInfo;
use nat::udp_random_bind;
use std::time::Duration;
use thin_client::ThinClient;

pub fn mk_client(r: &NodeInfo) -> ThinClient {
    let requests_socket = udp_random_bind(8000, 10000, 5).unwrap();
    let transactions_socket = udp_random_bind(8000, 10000, 5).unwrap();

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
