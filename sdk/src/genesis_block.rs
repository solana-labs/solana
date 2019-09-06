//! The `genesis_block` module is a library for generating the chain's genesis block.

use crate::account::Account;
use crate::fee_calculator::FeeCalculator;
use crate::hash::{hash, Hash};
use crate::inflation::Inflation;
use crate::poh_config::PohConfig;
use crate::pubkey::Pubkey;
use crate::rent::Rent;
use crate::signature::{Keypair, KeypairUtil};
use crate::system_program::{self, solana_system_program};
use crate::clock::{DEFAULT_SLOTS_PER_EPOCH, DEFAULT_SLOTS_PER_SEGMENT, DEFAULT_TICKS_PER_SLOT};
use bincode::{deserialize, serialize};
use memmap::Mmap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenesisBlock {
    pub accounts: Vec<(Pubkey, Account)>,
    pub native_instruction_processors: Vec<(String, Pubkey)>,
    pub rewards_pools: Vec<(Pubkey, Account)>,
    pub slots_per_epoch: u64,
    pub stakers_slot_offset: u64,
    pub epoch_warmup: bool,
    pub ticks_per_slot: u64,
    pub slots_per_segment: u64,
    pub poh_config: PohConfig,
    pub fee_calculator: FeeCalculator,
    pub inflation: Inflation,
    pub rent: Rent,
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
            epoch_warmup: true,
            slots_per_epoch: DEFAULT_SLOTS_PER_EPOCH,
            stakers_slot_offset: DEFAULT_SLOTS_PER_EPOCH,
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_segment: DEFAULT_SLOTS_PER_SEGMENT,
            poh_config: PohConfig::default(),
            inflation: Inflation::default(),
            fee_calculator: FeeCalculator::default(),
            rent: Rent::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct Builder {
    genesis_block: GenesisBlock,
    already_have_stakers_slot_offset: bool,
}

impl Builder {
    pub fn new() -> Self {
        Builder::default()
    }
    // consuming builder because I don't want to clone all the accounts
    pub fn build(self) -> GenesisBlock {
        self.genesis_block
    }

    fn append<T: Clone>(items: &[T], mut dest: Vec<T>) -> Vec<T> {
        items.iter().cloned().for_each(|item| dest.push(item));
        dest
    }

    pub fn account(self, pubkey: Pubkey, account: Account) -> Self {
        self.accounts(&[(pubkey, account)])
    }
    pub fn accounts(mut self, accounts: &[(Pubkey, Account)]) -> Self {
        self.genesis_block.accounts = Self::append(accounts, self.genesis_block.accounts);
        self
    }
    pub fn native_instruction_processor(self, name: &str, pubkey: Pubkey) -> Self {
        self.native_instruction_processors(&[(name.to_string(), pubkey)])
    }
    pub fn native_instruction_processors(
        mut self,
        native_instruction_processors: &[(String, Pubkey)],
    ) -> Self {
        self.genesis_block.native_instruction_processors = Self::append(
            native_instruction_processors,
            self.genesis_block.native_instruction_processors,
        );
        self
    }
    pub fn rewards_pool(self, pubkey: Pubkey, account: Account) -> Self {
        self.rewards_pools(&[(pubkey, account)])
    }
    pub fn rewards_pools(mut self, rewards_pools: &[(Pubkey, Account)]) -> Self {
        self.genesis_block.rewards_pools =
            Self::append(rewards_pools, self.genesis_block.rewards_pools);
        self
    }
    // also sets stakers_slot_offset, unless already set explicitly
    pub fn slots_per_epoch(mut self, slots_per_epoch: u64) -> Self {
        self.genesis_block.slots_per_epoch = slots_per_epoch;
        if !self.already_have_stakers_slot_offset {
            self.genesis_block.stakers_slot_offset = slots_per_epoch;
        }
        self
    }
    pub fn stakers_slot_offset(mut self, stakers_slot_offset: u64) -> Self {
        self.genesis_block.stakers_slot_offset = stakers_slot_offset;
        self.already_have_stakers_slot_offset = true;
        self
    }
    pub fn epoch_warmup(mut self, epoch_warmup: bool) -> Self {
        self.genesis_block.epoch_warmup = epoch_warmup;
        self
    }
    pub fn ticks_per_slot(mut self, ticks_per_slot: u64) -> Self {
        self.genesis_block.ticks_per_slot = ticks_per_slot;
        self
    }
    pub fn poh_config(mut self, poh_config: PohConfig) -> Self {
        self.genesis_block.poh_config = poh_config;
        self
    }
    pub fn fee_calculator(mut self, fee_calculator: FeeCalculator) -> Self {
        self.genesis_block.fee_calculator = fee_calculator;
        self
    }
    pub fn inflation(mut self, inflation: Inflation) -> Self {
        self.genesis_block.inflation = inflation;
        self
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
        let block = Builder::new()
            .account(
                mint_keypair.pubkey(),
                Account::new(10_000, 0, &Pubkey::default()),
            )
            .accounts(&[(Pubkey::new_rand(), Account::new(1, 0, &Pubkey::default()))])
            .native_instruction_processor("hi", Pubkey::new_rand())
            .build();

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
