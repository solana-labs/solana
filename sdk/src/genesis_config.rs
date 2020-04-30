//! The `genesis_config` module is a library for generating the chain's genesis config.

use crate::{
    account::Account,
    clock::{UnixTimestamp, DEFAULT_SLOTS_PER_SEGMENT, DEFAULT_TICKS_PER_SLOT},
    epoch_schedule::EpochSchedule,
    fee_calculator::FeeRateGovernor,
    hash::{hash, Hash},
    inflation::Inflation,
    native_token::lamports_to_sol,
    poh_config::PohConfig,
    pubkey::Pubkey,
    rent::Rent,
    shred_version::compute_shred_version,
    signature::{Keypair, Signer},
    system_program,
};
use bincode::{deserialize, serialize};
use chrono::{TimeZone, Utc};
use memmap::Mmap;
use std::{
    collections::BTreeMap,
    fmt,
    fs::{File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub enum OperatingMode {
    Preview,     // Next set of cluster features to be promoted to Stable
    Stable,      // Stable cluster features
    Development, // All features (including experimental features)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenesisConfig {
    /// when the network (bootstrap validator) was started relative to the UNIX Epoch
    pub creation_time: UnixTimestamp,
    /// initial accounts
    pub accounts: BTreeMap<Pubkey, Account>,
    /// built-in programs
    pub native_instruction_processors: Vec<(String, Pubkey)>,
    /// accounts for network rewards, these do not count towards capitalization
    pub rewards_pools: BTreeMap<Pubkey, Account>,
    pub ticks_per_slot: u64,
    pub slots_per_segment: u64,
    /// network speed configuration
    pub poh_config: PohConfig,
    /// this field exists only to ensure that the binary layout of GenesisConfig remains compatible
    /// with the Solana v0.23 release line
    pub __backwards_compat_with_v0_23: u64,
    /// transaction fee config
    pub fee_rate_governor: FeeRateGovernor,
    /// rent config
    pub rent: Rent,
    /// inflation config
    pub inflation: Inflation,
    /// how slots map to epochs
    pub epoch_schedule: EpochSchedule,
    /// network runlevel
    pub operating_mode: OperatingMode,
}

// useful for basic tests
pub fn create_genesis_config(lamports: u64) -> (GenesisConfig, Keypair) {
    let faucet_keypair = Keypair::new();
    (
        GenesisConfig::new(
            &[(
                faucet_keypair.pubkey(),
                Account::new(lamports, 0, &system_program::id()),
            )],
            &[],
        ),
        faucet_keypair,
    )
}

impl Default for GenesisConfig {
    fn default() -> Self {
        Self {
            creation_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as UnixTimestamp,
            accounts: BTreeMap::default(),
            native_instruction_processors: Vec::default(),
            rewards_pools: BTreeMap::default(),
            ticks_per_slot: DEFAULT_TICKS_PER_SLOT,
            slots_per_segment: DEFAULT_SLOTS_PER_SEGMENT,
            poh_config: PohConfig::default(),
            inflation: Inflation::default(),
            __backwards_compat_with_v0_23: 0,
            fee_rate_governor: FeeRateGovernor::default(),
            rent: Rent::default(),
            epoch_schedule: EpochSchedule::default(),
            operating_mode: OperatingMode::Development,
        }
    }
}

impl GenesisConfig {
    pub fn new(
        accounts: &[(Pubkey, Account)],
        native_instruction_processors: &[(String, Pubkey)],
    ) -> Self {
        Self {
            accounts: accounts
                .iter()
                .cloned()
                .collect::<BTreeMap<Pubkey, Account>>(),
            native_instruction_processors: native_instruction_processors.to_vec(),
            ..GenesisConfig::default()
        }
    }

    pub fn hash(&self) -> Hash {
        let serialized = serialize(&self).unwrap();
        hash(&serialized)
    }

    fn genesis_filename(ledger_path: &Path) -> PathBuf {
        Path::new(ledger_path).join("genesis.bin")
    }

    pub fn load(ledger_path: &Path) -> Result<Self, std::io::Error> {
        let filename = Self::genesis_filename(&ledger_path);
        let file = OpenOptions::new()
            .read(true)
            .open(&filename)
            .map_err(|err| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Unable to open {:?}: {:?}", filename, err),
                )
            })?;

        //UNSAFE: Required to create a Mmap
        let mem = unsafe { Mmap::map(&file) }.map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Unable to map {:?}: {:?}", filename, err),
            )
        })?;

        let genesis_config = deserialize(&mem).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Unable to deserialize {:?}: {:?}", filename, err),
            )
        })?;
        Ok(genesis_config)
    }

    pub fn write(&self, ledger_path: &Path) -> Result<(), std::io::Error> {
        let serialized = serialize(&self).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Unable to serialize: {:?}", err),
            )
        })?;

        std::fs::create_dir_all(&ledger_path)?;

        let mut file = File::create(Self::genesis_filename(&ledger_path))?;
        file.write_all(&serialized)
    }

    pub fn add_account(&mut self, pubkey: Pubkey, account: Account) {
        self.accounts.insert(pubkey, account);
    }

    pub fn add_native_instruction_processor(&mut self, name: String, program_id: Pubkey) {
        self.native_instruction_processors.push((name, program_id));
    }

    pub fn add_rewards_pool(&mut self, pubkey: Pubkey, account: Account) {
        self.rewards_pools.insert(pubkey, account);
    }
}

impl fmt::Display for GenesisConfig {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "\
             Creation time: {}\n\
             Operating mode: {:?}\n\
             Genesis hash: {}\n\
             Shred version: {}\n\
             Hashes per tick: {:?}\n\
             Slots per epoch: {}\n\
             Warmup epochs: {}abled\n\
             {:?}\n\
             {:?}\n\
             Capitalization: {} SOL in {} accounts\n\
             ",
            Utc.timestamp(self.creation_time, 0).to_rfc3339(),
            self.operating_mode,
            self.hash(),
            compute_shred_version(&self.hash(), None),
            self.poh_config.hashes_per_tick,
            self.epoch_schedule.slots_per_epoch,
            if self.epoch_schedule.warmup {
                "en"
            } else {
                "dis"
            },
            self.rent,
            self.fee_rate_governor,
            lamports_to_sol(
                self.accounts
                    .iter()
                    .map(|(pubkey, account)| {
                        if account.lamports == 0 {
                            panic!("{:?}", (pubkey, account));
                        }
                        account.lamports
                    })
                    .sum::<u64>()
            ),
            self.accounts.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signature::{Keypair, Signer};
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
    fn test_genesis_config() {
        let faucet_keypair = Keypair::new();
        let mut config = GenesisConfig::default();
        config.add_account(
            faucet_keypair.pubkey(),
            Account::new(10_000, 0, &Pubkey::default()),
        );
        config.add_account(Pubkey::new_rand(), Account::new(1, 0, &Pubkey::default()));
        config.add_native_instruction_processor("hi".to_string(), Pubkey::new_rand());

        assert_eq!(config.accounts.len(), 2);
        assert!(config
            .accounts
            .iter()
            .any(|(pubkey, account)| *pubkey == faucet_keypair.pubkey()
                && account.lamports == 10_000));

        let path = &make_tmp_path("genesis_config");
        config.write(&path).expect("write");
        let loaded_config = GenesisConfig::load(&path).expect("load");
        assert_eq!(config.hash(), loaded_config.hash());
        let _ignored = std::fs::remove_file(&path);
    }
}
