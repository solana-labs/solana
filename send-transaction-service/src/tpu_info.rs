use std::net::SocketAddr;

pub trait TpuInfo {
    fn refresh_recent_peers(&mut self, use_quic: bool);
    fn get_leader_tpus(&self, max_count: u64) -> Vec<&SocketAddr>;
}

#[derive(Clone)]
pub struct NullTpuInfo;

impl TpuInfo for NullTpuInfo {
    fn refresh_recent_peers(&mut self, _use_quic: bool) {}
    fn get_leader_tpus(&self, _max_count: u64) -> Vec<&SocketAddr> {
        vec![]
    }
}
