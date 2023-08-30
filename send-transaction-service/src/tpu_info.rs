use {solana_client::connection_cache::Protocol, solana_sdk::clock::Slot, std::net::SocketAddr};

pub trait TpuInfo {
    fn refresh_recent_peers(&mut self);
    fn get_leader_tpus(&self, max_count: u64, protocol: Protocol) -> Vec<&SocketAddr>;
    /// In addition to the the tpu address, also return the leader slot
    fn get_leader_tpus_with_slots(
        &self,
        max_count: u64,
        protocol: Protocol,
    ) -> Vec<(&SocketAddr, Slot)>;
}

#[derive(Clone)]
pub struct NullTpuInfo;

impl TpuInfo for NullTpuInfo {
    fn refresh_recent_peers(&mut self) {}
    fn get_leader_tpus(&self, _max_count: u64, _protocol: Protocol) -> Vec<&SocketAddr> {
        vec![]
    }
    fn get_leader_tpus_with_slots(
        &self,
        _max_count: u64,
        _protocol: Protocol,
    ) -> Vec<(&SocketAddr, Slot)> {
        vec![]
    }
}
