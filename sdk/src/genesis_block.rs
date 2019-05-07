//! The `genesis_block` module is a library for generating the chain's genesis block.

use crate::account::Account;
use crate::hash::{hash, Hash};
use crate::pubkey::Pubkey;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_program;
use crate::timing::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_TICKS_PER_SLOT};
use std::fs::File;
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug)]
pub struct GenesisBlock {
    pub bootstrap_leader_id: Pubkey,
    pub ticks_per_slot: u64,
    pub slots_per_epoch: u64,
    pub stakers_slot_offset: u64,
    pub epoch_warmup: bool,
    pub accounts: Vec<(Pubkey, Account)>,
    pub native_instruction_processors: Vec<(String, Pubkey)>,
}

// useful for basic tests
pub fn create_genesis_block(lamports: u64) -> (GenesisBlock, Keypair) {
    let mint_keypair = Keypair::new();
    (
        GenesisBlock::new(
            &Pubkey::default(),
            &[(
                mint_keypair.pubkey(),
                Account::new(lamports, 0, &system_program::id()),
            )],
            &[],
        ),
        mint_keypair,
    )
}

impl GenesisBlock {
    pub fn new(
        bootstrap_leader_id: &Pubkey,
        accounts: &[(Pubkey, Account)],
        native_instruction_processors: &[(String, Pubkey)],
    ) -> Self {
        Self {
            bootstrap_leader_id: *bootstrap_leader_id, // TODO: leader_schedule to derive from actual stakes, instead ;)
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_epoch: DEFAULT_SLOTS_PER_EPOCH,
            stakers_slot_offset: DEFAULT_SLOTS_PER_EPOCH,
            epoch_warmup: true,
            accounts: accounts.to_vec(),
            native_instruction_processors: native_instruction_processors.to_vec(),
        }
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

        let dir = Path::new(ledger_path);
        std::fs::create_dir_all(&dir)?;

        let mut file = File::create(&dir.join("genesis.json"))?;
        file.write_all(&serialized.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{Keypair, KeypairUtil};

    fn make_tmp_path(name: &str) -> String {
        let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "target".to_string());
        let keypair = Keypair::new();

        let path = format!("{}/tmp/{}-{}", out_dir, name, keypair.pubkey());

        // whack any possible collision
        let _ignored = std::fs::remove_dir_all(&path);
        // whack any possible collision
        let _ignored = std::fs::remove_file(&path);

        path
    }

    #[test]
    fn test_genesis_block() {
        let mint_keypair = Keypair::new();
        let block = GenesisBlock::new(
            &Pubkey::default(),
            &[
                (
                    mint_keypair.pubkey(),
                    Account::new(10_000, 0, &Pubkey::default()),
                ),
                (Pubkey::new_rand(), Account::new(1, 0, &Pubkey::default())),
            ],
            &[("hi".to_string(), Pubkey::new_rand())],
        );
        assert_eq!(block.accounts.len(), 2);
        assert!(block.accounts.iter().any(
            |(pubkey, account)| *pubkey == mint_keypair.pubkey() && account.lamports == 10_000
        ));

        let path = &make_tmp_path("genesis_block");
        block.write(&path).expect("write");
        let loaded_block = GenesisBlock::load(&path).expect("load");
        assert_eq!(block.hash(), loaded_block.hash());
        let _ignored = std::fs::remove_file(&path);
    }

}
