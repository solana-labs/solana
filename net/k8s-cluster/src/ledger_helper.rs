use {
    crate::{boxed_error, genesis::DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE, LEDGER_DIR},
    log::*,
    solana_sdk::{
        shred_version::compute_shred_version,
        hash::Hash,
    },
    solana_accounts_db::{
        hardened_unpack::open_genesis_config,
        accounts_db::AccountsDbConfig,
    },
    solana_ledger::{
        blockstore::Blockstore,
        blockstore_processor::ProcessOptions,
        blockstore_options::BlockstoreOptions,
    },
    solana_runtime::snapshot_config::SnapshotConfig,
    std::{
        error::Error,
        process::Command,
        str::FromStr,
    },
};

pub struct LedgerHelper {}

impl LedgerHelper {

    pub fn get_shred_version() -> u16 {
        let genesis_config = open_genesis_config(LEDGER_DIR.as_path(), DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE);
        let shred_version = compute_shred_version(
            &genesis_config.hash(),
            None
        );
        info!("Shred Version: {}", shred_version);
        shred_version
    }

    pub fn create_snapshot(warp_slot: u64) -> Result<(), Box<dyn Error>> {
        let output = Command::new("solana-ledger-tool")
            .arg("-l")
            .arg(LEDGER_DIR.as_os_str())
            .arg("create-snapshot")
            .arg("0")
            .arg(LEDGER_DIR.as_os_str())
            .arg("--warp-slot")
            .arg(warp_slot.to_string())
            .output()
            .expect("Failed to execute create-snapshot command");
    
        if !output.status.success() {
            return Err(boxed_error!("Failed to execute create-snapshot! Bad news"));
        }
        Ok(())
    }

    pub fn create_bank_hash() -> Result<Hash, Box<dyn Error>> {
        let output = Command::new("solana-ledger-tool")
            .arg("-l")
            .arg(LEDGER_DIR.as_os_str())
            .arg("bank-hash")
            .arg("--halt-at-slot")
            .arg(0.to_string())
            .output()
            .expect("Failed to execute bank-hash command");

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let bank_hash_string = stdout.trim();
            match Hash::from_str(bank_hash_string) {
                Ok(bank_hash) => return Ok(bank_hash),
                Err(err) => return Err(boxed_error!(format!("Failed to convert string to hash: {}", err))),
            }

        } else {
            return Err(boxed_error!("Failed to execute b Bad news"));
        }
    }
}

