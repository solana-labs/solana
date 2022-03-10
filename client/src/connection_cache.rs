use {
    crate::{tpu_connection::TpuConnection, udp_client::UdpTpuConnection},
    lazy_static::lazy_static,
    std::{
        collections::HashMap,
        net::{SocketAddr, UdpSocket},
        sync::{Arc, Mutex},
    },
};

//TODO: figure out a way to get rid of this mutex if possible
//try_insert seems promising but it's still nightly-only...
lazy_static! {
    //TODO: all implementations of TpuConnection should be Sync + Send but make sure...
    static ref CONNECTION_MAP: Mutex<HashMap<SocketAddr, Arc<dyn TpuConnection + Sync + Send>>> = Mutex::new(HashMap::new());
}

#[allow(dead_code)]
pub fn get_connection(addr: &SocketAddr) -> Arc<dyn TpuConnection> {
    //todo: implement eviction of old connections
    let mut map = (*CONNECTION_MAP).lock().unwrap();
    if let Some(conn) = map.get(addr) {
        conn.clone()
    } else {
        // TODO: do we want the ability to let the caller specify the socket as well?
        // in that case, should each (socket, addr) pair be considered unique?
        // it would seem pointless to have the socket caller-specified if not...
        let send_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        //TODO: make it configurable (e.g. via the command line) whether to use UDP or Quic
        let conn = Arc::new(UdpTpuConnection::new(send_socket, *addr));
        map.insert(*addr, conn.clone());
        conn
    }
}
