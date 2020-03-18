use bzip2::bufread::BzDecoder;
use console::Emoji;
use indicatif::{ProgressBar, ProgressStyle};
use log::*;
use solana_sdk::clock::Slot;
use solana_sdk::genesis_config::GenesisConfig;
use solana_sdk::hash::Hash;
use std::fs::{self, File};
use std::io;
use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

static TRUCK: Emoji = Emoji("🚚 ", "");
static SPARKLE: Emoji = Emoji("✨ ", "");

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar
        .set_style(ProgressStyle::default_spinner().template("{spinner:.green} {wide_msg}"));
    progress_bar.enable_steady_tick(100);
    progress_bar
}

pub fn download_file(url: &str, destination_file: &Path) -> Result<(), String> {
    if destination_file.is_file() {
        return Err(format!("{:?} already exists", destination_file));
    }
    let download_start = Instant::now();

    fs::create_dir_all(destination_file.parent().unwrap()).map_err(|err| err.to_string())?;

    let temp_destination_file = destination_file.with_extension(".tmp");

    let progress_bar = new_spinner_progress_bar();
    progress_bar.set_message(&format!("{}Downloading {}...", TRUCK, url));

    let response = reqwest::blocking::Client::new()
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| {
            progress_bar.finish_and_clear();
            err.to_string()
        })?;

    let download_size = {
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|content_length| content_length.to_str().ok())
            .and_then(|content_length| content_length.parse().ok())
            .unwrap_or(0)
    };
    progress_bar.set_length(download_size);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template(&format!(
                "{}{}Downloading {} {}",
                "{spinner:.green} ",
                TRUCK,
                url,
                "[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})"
            ))
            .progress_chars("=> "),
    );

    struct DownloadProgress<R> {
        progress_bar: ProgressBar,
        response: R,
    }

    impl<R: Read> Read for DownloadProgress<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.response.read(buf).map(|n| {
                self.progress_bar.inc(n as u64);
                n
            })
        }
    }

    let mut source = DownloadProgress {
        progress_bar,
        response,
    };

    File::create(&temp_destination_file)
        .and_then(|mut file| std::io::copy(&mut source, &mut file))
        .map_err(|err| format!("Unable to write {:?}: {:?}", temp_destination_file, err))?;

    source.progress_bar.finish_and_clear();
    info!(
        "  {}{}",
        SPARKLE,
        format!(
            "Downloaded {} ({} bytes) in {:?}",
            url,
            download_size,
            Instant::now().duration_since(download_start),
        )
    );

    std::fs::rename(temp_destination_file, destination_file)
        .map_err(|err| format!("Unable to rename: {:?}", err))?;

    Ok(())
}

fn extract_archive(archive_filename: &Path, destination_dir: &Path) -> Result<(), String> {
    info!("Extracting {:?}...", archive_filename);
    let extract_start = Instant::now();

    fs::create_dir_all(destination_dir).map_err(|err| err.to_string())?;
    let tar_bz2 = File::open(&archive_filename)
        .map_err(|err| format!("Unable to open {:?}: {:?}", archive_filename, err))?;
    let tar = BzDecoder::new(std::io::BufReader::new(tar_bz2));
    let mut archive = tar::Archive::new(tar);
    archive
        .unpack(destination_dir)
        .map_err(|err| format!("Unable to unpack {:?}: {:?}", archive_filename, err))?;
    info!(
        "Extracted {:?} in {:?}",
        archive_filename,
        Instant::now().duration_since(extract_start)
    );
    Ok(())
}

pub fn download_genesis(
    rpc_addr: &SocketAddr,
    ledger_path: &Path,
    expected_genesis_hash: Option<Hash>,
) -> Result<Hash, String> {
    let genesis_package = ledger_path.join("genesis.tar.bz2");

    let genesis_config = if !genesis_package.exists() {
        let tmp_genesis_path = ledger_path.join("tmp-genesis");
        let tmp_genesis_package = tmp_genesis_path.join("genesis.tar.bz2");

        let _ignored = fs::remove_dir_all(&tmp_genesis_path);
        download_file(
            &format!("http://{}/{}", rpc_addr, "genesis.tar.bz2"),
            &tmp_genesis_package,
        )?;
        extract_archive(&tmp_genesis_package, &ledger_path)?;

        let tmp_genesis_config = GenesisConfig::load(&ledger_path)
            .map_err(|err| format!("Failed to load downloaded genesis config: {}", err))?;

        if let Some(expected_genesis_hash) = expected_genesis_hash {
            if expected_genesis_hash != tmp_genesis_config.hash() {
                return Err(format!(
                    "Genesis hash mismatch: expected {} but downloaded genesis hash is {}",
                    expected_genesis_hash,
                    tmp_genesis_config.hash(),
                ));
            }
        }

        std::fs::rename(tmp_genesis_package, genesis_package)
            .map_err(|err| format!("Unable to rename: {:?}", err))?;
        tmp_genesis_config
    } else {
        GenesisConfig::load(&ledger_path)
            .map_err(|err| format!("Failed to load genesis config: {}", err))?
    };

    Ok(genesis_config.hash())
}

pub fn download_snapshot(
    rpc_addr: &SocketAddr,
    ledger_path: &Path,
    desired_snapshot_hash: (Slot, Hash),
) -> Result<(), String> {
    // Remove all snapshot not matching the desired hash
    let snapshot_packages = solana_ledger::snapshot_utils::get_snapshot_archives(ledger_path);
    for (snapshot_package, snapshot_hash) in snapshot_packages.iter() {
        if *snapshot_hash != desired_snapshot_hash {
            info!("Removing old snapshot: {:?}", snapshot_package);
            fs::remove_file(snapshot_package)
                .unwrap_or_else(|err| info!("Failed to remove old snapshot: {:}", err));
        }
    }

    let desired_snapshot_package = solana_ledger::snapshot_utils::get_snapshot_archive_path(
        ledger_path,
        &desired_snapshot_hash,
    );
    if desired_snapshot_package.exists() {
        Ok(())
    } else {
        download_file(
            &format!(
                "http://{}/{}",
                rpc_addr,
                desired_snapshot_package
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
            ),
            &desired_snapshot_package,
        )
    }
}
