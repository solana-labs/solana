use {
    solana_download_utils::download_genesis_if_missing,
    solana_runtime::hardened_unpack::unpack_genesis_archive,
    solana_sdk::{
        genesis_config::{GenesisConfig, DEFAULT_GENESIS_ARCHIVE},
        hash::Hash,
    },
    std::net::SocketAddr,
};

fn check_genesis_hash(
    genesis_config: &GenesisConfig,
    expected_genesis_hash: Option<Hash>,
) -> Result<(), String> {
    let genesis_hash = genesis_config.hash();

    if let Some(expected_genesis_hash) = expected_genesis_hash {
        if expected_genesis_hash != genesis_hash {
            return Err(format!(
                "Genesis hash mismatch: expected {expected_genesis_hash} but downloaded genesis hash is {genesis_hash}",
            ));
        }
    }

    Ok(())
}

fn load_local_genesis(
    ledger_path: &std::path::Path,
    expected_genesis_hash: Option<Hash>,
) -> Result<GenesisConfig, String> {
    let existing_genesis = GenesisConfig::load(ledger_path)
        .map_err(|err| format!("Failed to load genesis config: {err}"))?;
    check_genesis_hash(&existing_genesis, expected_genesis_hash)?;

    Ok(existing_genesis)
}

pub fn download_then_check_genesis_hash(
    rpc_addr: &SocketAddr,
    ledger_path: &std::path::Path,
    expected_genesis_hash: Option<Hash>,
    max_genesis_archive_unpacked_size: u64,
    no_genesis_fetch: bool,
    use_progress_bar: bool,
) -> Result<GenesisConfig, String> {
    if no_genesis_fetch {
        let genesis_config = load_local_genesis(ledger_path, expected_genesis_hash)?;
        return Ok(genesis_config);
    }

    let genesis_package = ledger_path.join(DEFAULT_GENESIS_ARCHIVE);
    let genesis_config = if let Ok(tmp_genesis_package) =
        download_genesis_if_missing(rpc_addr, &genesis_package, use_progress_bar)
    {
        unpack_genesis_archive(
            &tmp_genesis_package,
            ledger_path,
            max_genesis_archive_unpacked_size,
        )
        .map_err(|err| format!("Failed to unpack downloaded genesis config: {err}"))?;

        let downloaded_genesis = GenesisConfig::load(ledger_path)
            .map_err(|err| format!("Failed to load downloaded genesis config: {err}"))?;

        check_genesis_hash(&downloaded_genesis, expected_genesis_hash)?;
        std::fs::rename(tmp_genesis_package, genesis_package)
            .map_err(|err| format!("Unable to rename: {err:?}"))?;

        downloaded_genesis
    } else {
        load_local_genesis(ledger_path, expected_genesis_hash)?
    };

    Ok(genesis_config)
}
