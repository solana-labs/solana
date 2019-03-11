//! The `genesis_block` module is a library for generating the chain's genesis block.

use crate::hash::{hash, Hash};
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::timing::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT};
use std::fs::File;
use std::io::Write;
use std::path::Path;

// The default (and minimal) amount of lamports given to the bootstrap leader:
// * 1 lamports for the bootstrap leader ID account
// * 1 lamport for the bootstrap leader vote account
pub const BOOTSTRAP_LEADER_LAMPORTS: u64 = 2;

#[derive(Serialize, Deserialize, Debug)]
pub struct GenesisBlock {
    pub bootstrap_leader_id: Pubkey,
    pub bootstrap_leader_lamports: u64,
    pub bootstrap_leader_vote_account_id: Pubkey,
    pub mint_id: Pubkey,
    pub lamports: u64,
    pub ticks_per_slot: u64,
    pub slots_per_epoch: u64,
    pub stakers_slot_offset: u64,
    pub epoch_warmup: bool,
}

impl GenesisBlock {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(lamports: u64) -> (Self, Keypair) {
        let lamports = lamports
            .checked_add(BOOTSTRAP_LEADER_LAMPORTS)
            .unwrap_or(lamports);
        Self::new_with_leader(
            lamports,
            &Keypair::new().pubkey(),
            BOOTSTRAP_LEADER_LAMPORTS,
        )
    }

    pub fn new_with_leader(
        lamports: u64,
        bootstrap_leader_id: &Pubkey,
        bootstrap_leader_lamports: u64,
    ) -> (Self, Keypair) {
        let mint_keypair = Keypair::new();
        let bootstrap_leader_vote_account_keypair = Keypair::new();
        (
            Self {
                bootstrap_leader_id: *bootstrap_leader_id,
                bootstrap_leader_lamports,
                bootstrap_leader_vote_account_id: bootstrap_leader_vote_account_keypair.pubkey(),
                mint_id: mint_keypair.pubkey(),
                lamports,
                ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
                slots_per_epoch: DEFAULT_SLOTS_PER_EPOCH,
                stakers_slot_offset: DEFAULT_SLOTS_PER_EPOCH,
                epoch_warmup: true,
            },
            mint_keypair,
        )
    }

    pub fn hash(&self) -> Hash {
        let serialized = serde_json::to_string(self).unwrap();
        hash(&serialized.into_bytes())
    }

    pub fn load(ledger_path: &str) -> Result<Self, std::io::Error> {
        let file = File::open(&Path::new(ledger_path).join("genesis.json"))?;
        let genesis_block = serde_json::from_reader(file)?;
        Ok(genesis_block)
    }

    pub fn write(&self, ledger_path: &str) -> Result<(), std::io::Error> {
        let serialized = serde_json::to_string(self)?;
        let mut file = File::create(&Path::new(ledger_path).join("genesis.json"))?;
        file.write_all(&serialized.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_block_new() {
        let (genesis_block, mint) = GenesisBlock::new(10_000);
        assert_eq!(genesis_block.lamports, 10_000 + BOOTSTRAP_LEADER_LAMPORTS);
        assert_eq!(genesis_block.mint_id, mint.pubkey());
        assert!(genesis_block.bootstrap_leader_id != Pubkey::default());
        assert!(genesis_block.bootstrap_leader_vote_account_id != Pubkey::default());
        assert_eq!(
            genesis_block.bootstrap_leader_lamports,
            BOOTSTRAP_LEADER_LAMPORTS
        );
    }

    #[test]
    fn test_genesis_block_new_with_leader() {
        let leader_keypair = Keypair::new();
        let (genesis_block, mint) =
            GenesisBlock::new_with_leader(20_000, &leader_keypair.pubkey(), 123);

        assert_eq!(genesis_block.lamports, 20_000);
        assert_eq!(genesis_block.mint_id, mint.pubkey());
        assert_eq!(genesis_block.bootstrap_leader_id, leader_keypair.pubkey());
        assert_eq!(genesis_block.bootstrap_leader_lamports, 123);
    }
}
