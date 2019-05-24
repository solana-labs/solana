use solana_sdk::pubkey::Pubkey;

pub trait Cluster {
    fn get_node_pubkeys(&self) -> Vec<Pubkey>;
    fn restart_node(&mut self, pubkey: Pubkey);
}
