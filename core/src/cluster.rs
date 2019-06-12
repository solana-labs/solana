use solana_client::thin_client::ThinClient;
use solana_sdk::pubkey::Pubkey;

pub trait Cluster {
    fn get_node_pubkeys(&self) -> Vec<Pubkey>;
    fn get_validator_client(&self, pubkey: &Pubkey) -> Option<ThinClient>;
    fn restart_node(&mut self, pubkey: Pubkey);
}
