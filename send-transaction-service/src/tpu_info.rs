use std::net::SocketAddr;

pub trait TpuInfo {
    fn refresh_recent_peers(&mut self);
    fn get_leader_tpus(&self, extended: bool, max_count: u64) -> Vec<SocketAddr>;
}

pub struct NullTpuInfo;

impl TpuInfo for NullTpuInfo {
    fn refresh_recent_peers(&mut self) {}
    fn get_leader_tpus(&self, _extended: bool, _max_count: u64) -> Vec<SocketAddr> {
        vec![]
    }
}
