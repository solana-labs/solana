#![allow(clippy::integer_arithmetic)]
use console::Emoji;
use indicatif::{ProgressBar, ProgressStyle};
use log::*;
use solana_runtime::{bank_forks::ArchiveFormat, snapshot_utils};
use solana_sdk::clock::Slot;
use solana_sdk::hash::Hash;
use std::fs::{self, File};
use std::io;
use std::io::Read;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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

pub fn download_file(
    url: &str,
    destination_file: &Path,
    use_progress_bar: bool,
) -> Result<(), String> {
    if destination_file.is_file() {
        return Err(format!("{:?} already exists", destination_file));
    }
    let download_start = Instant::now();

    fs::create_dir_all(destination_file.parent().expect("parent"))
        .map_err(|err| err.to_string())?;

    let mut temp_destination_file = destination_file.to_path_buf();
    temp_destination_file.set_file_name(format!(
        "tmp-{}",
        destination_file
            .file_name()
            .expect("file_name")
            .to_str()
            .expect("to_str")
    ));

    let progress_bar = new_spinner_progress_bar();
    if use_progress_bar {
        progress_bar.set_message(&format!("{}Downloading {}...", TRUCK, url));
    }

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

    if use_progress_bar {
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
    } else {
        info!("Downloading {} bytes from {}", download_size, url);
    }

    struct DownloadProgress<R> {
        progress_bar: ProgressBar,
        response: R,
        last_print: Instant,
        current_bytes: usize,
        last_print_bytes: usize,
        download_size: f32,
        use_progress_bar: bool,
    }

    impl<R: Read> Read for DownloadProgress<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.response.read(buf).map(|n| {
                if self.use_progress_bar {
                    self.progress_bar.inc(n as u64);
                } else {
                    self.current_bytes += n;
                    if self.last_print.elapsed().as_secs() > 5 {
                        let total_bytes_f32 = self.current_bytes as f32;
                        let diff_bytes_f32 = (self.current_bytes - self.last_print_bytes) as f32;
                        info!(
                            "downloaded {} bytes {:.1}% {:.1} bytes/s",
                            self.current_bytes,
                            100f32 * (total_bytes_f32 / self.download_size),
                            diff_bytes_f32 / self.last_print.elapsed().as_secs_f32(),
                        );
                        self.last_print = Instant::now();
                        self.last_print_bytes = self.current_bytes;
                    }
                }
                n
            })
        }
    }

    let mut source = DownloadProgress {
        progress_bar,
        response,
        last_print: Instant::now(),
        current_bytes: 0,
        last_print_bytes: 0,
        download_size: (download_size as f32).max(1f32),
        use_progress_bar,
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

pub fn download_genesis_if_missing(
    rpc_addr: &SocketAddr,
    genesis_package: &Path,
    use_progress_bar: bool,
) -> Result<PathBuf, String> {
    if !genesis_package.exists() {
        let tmp_genesis_path = genesis_package.parent().unwrap().join("tmp-genesis");
        let tmp_genesis_package = tmp_genesis_path.join("genesis.tar.bz2");

        let _ignored = fs::remove_dir_all(&tmp_genesis_path);
        download_file(
            &format!("http://{}/{}", rpc_addr, "genesis.tar.bz2"),
            &tmp_genesis_package,
            use_progress_bar,
        )?;

        Ok(tmp_genesis_package)
    } else {
        Err("genesis already exists".to_string())
    }
}

pub fn download_snapshot(
    rpc_addr: &SocketAddr,
    ledger_path: &Path,
    desired_snapshot_hash: (Slot, Hash),
    use_progress_bar: bool,
) -> Result<(), String> {
    snapshot_utils::purge_old_snapshot_archives(ledger_path);

    for compression in &[
        ArchiveFormat::TarZstd,
        ArchiveFormat::TarGzip,
        ArchiveFormat::TarBzip2,
    ] {
        let desired_snapshot_package = snapshot_utils::get_snapshot_archive_path(
            ledger_path.to_path_buf(),
            &desired_snapshot_hash,
            *compression,
        );

        if desired_snapshot_package.is_file() {
            return Ok(());
        }

        if download_file(
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
            use_progress_bar,
        )
        .is_ok()
        {
            return Ok(());
        }
    }
    Err("Snapshot couldn't be downloaded".to_string())
}
