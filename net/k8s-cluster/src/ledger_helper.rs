use {
    crate::{SOLANA_ROOT, genesis::DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE},
    log::*,
    solana_sdk::shred_version::compute_shred_version,
    solana_accounts_db::hardened_unpack::open_genesis_config,
};

pub struct LedgerHelper {}

impl LedgerHelper {
    pub fn get_shred_version() -> u16 {
        let ledger_dir = SOLANA_ROOT.join("config-k8s/bootstrap-validator");
        let genesis_config = open_genesis_config(ledger_dir.as_path(), DEFAULT_MAX_GENESIS_ARCHIVE_UNPACKED_SIZE);
        let shred_version = compute_shred_version(
            &genesis_config.hash(),
            None
        );
        info!("Shred Version: {}", shred_version);
        shred_version
    }
}