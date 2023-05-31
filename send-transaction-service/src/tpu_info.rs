use {solana_client::connection_cache::Protocol, std::net::SocketAddr};

pub trait TpuInfo {
    fn refresh_recent_peers(&mut self, protocol: Protocol);
    fn get_leader_tpus(&self, max_count: u64) -> Vec<&SocketAddr>;
}

#[derive(Clone)]
pub struct NullTpuInfo;

impl TpuInfo for NullTpuInfo {
    fn refresh_recent_peers(&mut self, _protocol: Protocol) {}
    fn get_leader_tpus(&self, _max_count: u64) -> Vec<&SocketAddr> {
        vec![]
    }
}
