use crate::notifier::Notifier;
use bzip2::bufread::BzDecoder;
use log::*;
use solana_client::rpc_client::RpcClient;
use solana_net_utils::parse_host;
use solana_sdk::{
    clock::{Epoch, Slot},
    genesis_config::GenesisConfig,
    timing::duration_as_ms,
};
use std::{
    fs::File,
    io,
    net::SocketAddr,
    path::Path,
    thread::sleep,
    time::{Duration, Instant},
};
use tar::Archive;

const GENESIS_ARCHIVE_NAME: &str = "genesis.tar.bz2";

/// Inspired by solana_local_cluster::cluster_tests
fn slots_to_secs(num_slots: u64, genesis_config: &GenesisConfig) -> u64 {
    let poh_config = &genesis_config.poh_config;
    let ticks_per_slot = genesis_config.ticks_per_slot;
    let num_ticks_to_sleep = num_slots as f64 * ticks_per_slot as f64;
    let num_ticks_per_second = (1000 / duration_as_ms(&poh_config.target_tick_duration)) as f64;
    ((num_ticks_to_sleep + num_ticks_per_second - 1.0) / num_ticks_per_second) as u64
}

fn sleep_n_slots(num_slots: u64, genesis_config: &GenesisConfig) {
    let secs = slots_to_secs(num_slots, genesis_config);
    let mins = secs / 60;
    let hours = mins / 60;
    if hours >= 5 {
        debug!("Sleeping for {} slots ({} hours)", num_slots, hours);
    } else if mins >= 5 {
        debug!("Sleeping for {} slots ({} minutes)", num_slots, mins);
    } else if secs > 0 {
        debug!("Sleeping for {} slots ({} seconds)", num_slots, secs);
    }
    sleep(Duration::from_secs(secs));
}

/// Sleep until the target epoch has started or bail if cluster is stuck
pub fn sleep_until_epoch(
    rpc_client: &RpcClient,
    notifier: &Notifier,
    genesis_config: &GenesisConfig,
    mut current_slot: Slot,
    target_epoch: Epoch,
) {
    let target_slot = genesis_config
        .epoch_schedule
        .get_first_slot_in_epoch(target_epoch);
    info!(
        "sleep_until_epoch() target_epoch: {}, target_slot: {}",
        target_epoch, target_slot
    );

    loop {
        let sleep_slots = target_slot.saturating_sub(current_slot);
        if sleep_slots == 0 {
            break;
        }

        sleep_n_slots(sleep_slots.max(50), genesis_config);
        let latest_slot = rpc_client.get_slot().unwrap_or_else(|err| {
            bail(
                notifier,
                &format!("Error: Could not fetch current slot: {}", err),
            );
        });

        if current_slot == latest_slot {
            bail(
                notifier,
                &format!("Error: Slot did not advance from {}", current_slot),
            );
        } else {
            current_slot = latest_slot;
        }
    }
}

pub fn is_host(string: String) -> Result<(), String> {
    parse_host(&string)?;
    Ok(())
}

pub fn bail(notifier: &crate::notifier::Notifier, msg: &str) -> ! {
    notifier.notify(msg);
    sleep(Duration::from_secs(30)); // Wait for notifications to send
    std::process::exit(1);
}

/// Inspired by solana_validator::download_tar_bz2
pub fn download_genesis(rpc_addr: &SocketAddr, download_path: &Path) -> Result<(), String> {
    let archive_name = GENESIS_ARCHIVE_NAME;
    let archive_path = download_path.join(archive_name);
    let url = format!("http://{}/{}", rpc_addr, archive_name);
    let download_start = Instant::now();
    debug!("Downloading genesis ({})...", url);

    let client = reqwest::blocking::Client::new();
    let mut response = client
        .get(url.as_str())
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| format!("Unable to get: {:?}", err))?;
    let download_size = {
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|content_length| content_length.to_str().ok())
            .and_then(|content_length| content_length.parse().ok())
            .unwrap_or(0)
    };

    let mut file = File::create(&archive_path)
        .map_err(|err| format!("Unable to create {:?}: {:?}", archive_path, err))?;
    io::copy(&mut response, &mut file)
        .map_err(|err| format!("Unable to write {:?}: {:?}", archive_path, err))?;

    debug!(
        "Downloaded genesis ({} bytes) in {:?}",
        download_size,
        Instant::now().duration_since(download_start),
    );

    debug!("Extracting genesis ({})...", archive_name);
    let extract_start = Instant::now();
    let tar_bz2 = File::open(&archive_path)
        .map_err(|err| format!("Unable to open {}: {:?}", archive_name, err))?;
    let tar = BzDecoder::new(io::BufReader::new(tar_bz2));
    let mut archive = Archive::new(tar);
    archive
        .unpack(download_path)
        .map_err(|err| format!("Unable to unpack {}: {:?}", archive_name, err))?;
    debug!(
        "Extracted {} in {:?}",
        archive_name,
        Instant::now().duration_since(extract_start)
    );

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_slots_to_secs() {
        let mut genesis_config = GenesisConfig::default();
        genesis_config.poh_config.target_tick_duration = Duration::from_millis(500);

        genesis_config.ticks_per_slot = 10;
        assert_eq!(slots_to_secs(2, &genesis_config), 10);

        genesis_config.ticks_per_slot = 1;
        assert_eq!(slots_to_secs(1, &genesis_config), 1);

        genesis_config.ticks_per_slot = 0;
        assert_eq!(slots_to_secs(10, &genesis_config), 0);
    }
}
