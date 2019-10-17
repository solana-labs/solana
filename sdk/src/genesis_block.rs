//! The `genesis_block` module is a library for generating the chain's genesis block.

use crate::{
    account::Account,
    clock::{DEFAULT_SLOTS_PER_SEGMENT, DEFAULT_TICKS_PER_SLOT},
    epoch_schedule::EpochSchedule,
    fee_calculator::FeeCalculator,
    hash::{hash, Hash},
    inflation::Inflation,
    poh_config::PohConfig,
    pubkey::Pubkey,
    rent_calculator::RentCalculator,
    signature::{Keypair, KeypairUtil},
    system_program::{self, solana_system_program},
};
use bincode::{deserialize, serialize};
use memmap::Mmap;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub enum OperatingMode {
    SoftLaunch,  // Cluster features incrementally enabled over time
    Development, // All features (including experimental features) available immediately from genesis
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenesisBlock {
    pub accounts: Vec<(Pubkey, Account)>,
    pub native_instruction_processors: Vec<(String, Pubkey)>,
    pub rewards_pools: Vec<(Pubkey, Account)>,
    pub ticks_per_slot: u64,
    pub slots_per_segment: u64,
    pub poh_config: PohConfig,
    pub fee_calculator: FeeCalculator,
    pub rent_calculator: RentCalculator,
    pub inflation: Inflation,
    pub epoch_schedule: EpochSchedule,
    pub operating_mode: OperatingMode,
}

// useful for basic tests
pub fn create_genesis_block(lamports: u64) -> (GenesisBlock, Keypair) {
    let mint_keypair = Keypair::new();
    (
        GenesisBlock::new(
            &[(
                mint_keypair.pubkey(),
                Account::new(lamports, 0, &system_program::id()),
            )],
            &[solana_system_program()],
        ),
        mint_keypair,
    )
}

impl Default for GenesisBlock {
    fn default() -> Self {
        Self {
            accounts: Vec::new(),
            native_instruction_processors: Vec::new(),
            rewards_pools: Vec::new(),
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_segment: DEFAULT_SLOTS_PER_SEGMENT,
            poh_config: PohConfig::default(),
            inflation: Inflation::default(),
            fee_calculator: FeeCalculator::default(),
            rent_calculator: RentCalculator::default(),
            epoch_schedule: EpochSchedule::default(),
            operating_mode: OperatingMode::Development,
        }
    }
}

impl GenesisBlock {
    pub fn new(
        accounts: &[(Pubkey, Account)],
        native_instruction_processors: &[(String, Pubkey)],
    ) -> Self {
        Self {
            accounts: accounts.to_vec(),
            native_instruction_processors: native_instruction_processors.to_vec(),
            ..GenesisBlock::default()
        }
    }

    pub fn hash(&self) -> Hash {
        let serialized = serde_json::to_string(self).unwrap();
        hash(&serialized.into_bytes())
    }

    pub fn load(ledger_path: &Path) -> Result<Self, std::io::Error> {
        let file = OpenOptions::new()
            .read(true)
            .open(&Path::new(ledger_path).join("genesis.bin"))
            .expect("Unable to open genesis file");

        //UNSAFE: Required to create a Mmap
        let mem = unsafe { Mmap::map(&file).expect("failed to map the genesis file") };
        let genesis_block = deserialize(&mem)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", err)))?;
        Ok(genesis_block)
    }

    pub fn write(&self, ledger_path: &Path) -> Result<(), std::io::Error> {
        let serialized = serialize(&self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", err)))?;

        std::fs::create_dir_all(&ledger_path)?;

        let mut file = File::create(&ledger_path.join("genesis.bin"))?;
        file.write_all(&serialized)
    }

    pub fn add_account(&mut self, pubkey: Pubkey, account: Account) {
        self.accounts.push((pubkey, account));
    }

    pub fn add_native_instruction_processor(&mut self, name: String, program_id: Pubkey) {
        self.native_instruction_processors.push((name, program_id));
    }

    pub fn add_rewards_pool(&mut self, pubkey: Pubkey, account: Account) {
        self.rewards_pools.push((pubkey, account));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{Keypair, KeypairUtil};
    use std::path::PathBuf;

    fn make_tmp_path(name: &str) -> PathBuf {
        let out_dir = std::env::var("FARF_DIR").unwrap_or_else(|_| "farf".to_string());
        let keypair = Keypair::new();

        let path = [
            out_dir,
            "tmp".to_string(),
            format!("{}-{}", name, keypair.pubkey()),
        ]
        .iter()
        .collect();

        // whack any possible collision
        let _ignored = std::fs::remove_dir_all(&path);
        // whack any possible collision
        let _ignored = std::fs::remove_file(&path);

        path
    }

    #[test]
    fn test_genesis_block() {
        let mint_keypair = Keypair::new();
        let mut block = GenesisBlock::default();
        block.add_account(
            mint_keypair.pubkey(),
            Account::new(10_000, 0, &Pubkey::default()),
        );
        block.add_account(Pubkey::new_rand(), Account::new(1, 0, &Pubkey::default()));
        block.add_native_instruction_processor("hi".to_string(), Pubkey::new_rand());

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
