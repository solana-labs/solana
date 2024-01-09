use {
    crate::{boxed_error, genesis::DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE, LEDGER_DIR},
    log::*,
    solana_accounts_db::hardened_unpack::open_genesis_config,
    solana_sdk::{hash::Hash, shred_version::compute_shred_version},
    std::{error::Error, path::PathBuf, process::Command, str::FromStr},
};

fn ledger_directory_exists() -> Result<(), Box<dyn Error>> {
    if !LEDGER_DIR.exists() {
        return Err(boxed_error!(format!(
            "Ledger Directory does not exist, have you created genesis yet??"
        )));
    }
    Ok(())
}

pub struct LedgerHelper {}

impl LedgerHelper {
    pub fn get_shred_version() -> Result<u16, Box<dyn Error>> {
        ledger_directory_exists()?;
        let genesis_config = open_genesis_config(
            LEDGER_DIR.as_path(),
            DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
        );
        let shred_version = compute_shred_version(&genesis_config.hash(), None);
        info!("Shred Version: {}", shred_version);
        Ok(shred_version)
    }

    pub fn create_snapshot(_warp_slot: u64, build_path: PathBuf) -> Result<(), Box<dyn Error>> {
        ledger_directory_exists()?;
        let config_dir = LEDGER_DIR.join("accounts_hash_cache");
        if config_dir.exists() {
            std::fs::remove_dir_all(&config_dir).unwrap();
        }
        std::fs::create_dir_all(&config_dir).unwrap();
        let executable_path = build_path.join("solana-ledger-tool");
        let output = Command::new(executable_path)
            .arg("-l")
            .arg(LEDGER_DIR.as_os_str())
            .arg("create-snapshot")
            .arg("0")
            .arg(LEDGER_DIR.as_os_str())
            // .arg("--warp-slot")
            // .arg(warp_slot.to_string())
            .output()
            .expect("Failed to execute create-snapshot command");

        if !output.status.success() {
            return Err(boxed_error!(format!(
                "Error in solana-ledger-tool create-snapshot command. err: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(())
    }

    pub fn create_bank_hash(build_path: PathBuf) -> Result<Hash, Box<dyn Error>> {
        ledger_directory_exists()?;
        let executable_path = build_path.join("solana-ledger-tool");
        let output = Command::new(executable_path)
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
                Ok(bank_hash) => Ok(bank_hash),
                Err(err) => Err(boxed_error!(format!(
                    "Failed to convert string to hash: {}",
                    err
                ))),
            }
        } else {
            Err(boxed_error!(
                "Error in solana-ledger-tool bank-hash command"
            ))
        }
    }
}
