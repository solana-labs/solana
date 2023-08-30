use {
    log::*,
    solana_accounts_db::hardened_unpack::unpack_genesis_archive,
    solana_download_utils::download_genesis_if_missing,
    solana_rpc_client::rpc_client::RpcClient,
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

fn get_genesis_config(
    rpc_addr: &SocketAddr,
    ledger_path: &std::path::Path,
    expected_genesis_hash: Option<Hash>,
    max_genesis_archive_unpacked_size: u64,
    no_genesis_fetch: bool,
    use_progress_bar: bool,
) -> Result<GenesisConfig, String> {
    if no_genesis_fetch {
        return load_local_genesis(ledger_path, expected_genesis_hash);
    }

    let genesis_package = ledger_path.join(DEFAULT_GENESIS_ARCHIVE);
    if let Ok(tmp_genesis_package) =
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

        Ok(downloaded_genesis)
    } else {
        load_local_genesis(ledger_path, expected_genesis_hash)
    }
}

fn set_and_verify_expected_genesis_hash(
    genesis_config: GenesisConfig,
    expected_genesis_hash: &mut Option<Hash>,
    rpc_client: &RpcClient,
) -> Result<(), String> {
    let genesis_hash = genesis_config.hash();
    if expected_genesis_hash.is_none() {
        info!("Expected genesis hash set to {}", genesis_hash);
        *expected_genesis_hash = Some(genesis_hash);
    }
    let expected_genesis_hash = expected_genesis_hash.unwrap();

    // Sanity check that the RPC node is using the expected genesis hash before
    // downloading a snapshot from it
    let rpc_genesis_hash = rpc_client
        .get_genesis_hash()
        .map_err(|err| format!("Failed to get genesis hash: {err}"))?;

    if expected_genesis_hash != rpc_genesis_hash {
        return Err(format!(
            "Genesis hash mismatch: expected {expected_genesis_hash} but RPC node genesis hash is {rpc_genesis_hash}"
        ));
    }

    Ok(())
}

pub fn download_then_check_genesis_hash(
    rpc_addr: &SocketAddr,
    ledger_path: &std::path::Path,
    expected_genesis_hash: &mut Option<Hash>,
    max_genesis_archive_unpacked_size: u64,
    no_genesis_fetch: bool,
    use_progress_bar: bool,
    rpc_client: &RpcClient,
) -> Result<(), String> {
    let genesis_config = get_genesis_config(
        rpc_addr,
        ledger_path,
        *expected_genesis_hash,
        max_genesis_archive_unpacked_size,
        no_genesis_fetch,
        use_progress_bar,
    )?;

    set_and_verify_expected_genesis_hash(genesis_config, expected_genesis_hash, rpc_client)
}
