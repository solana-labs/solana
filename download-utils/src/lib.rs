#![allow(clippy::integer_arithmetic)]
use {
    console::Emoji,
    indicatif::{ProgressBar, ProgressStyle},
    log::*,
    solana_runtime::{
        snapshot_hash::SnapshotHash,
        snapshot_package::SnapshotType,
        snapshot_utils::{self, ArchiveFormat},
    },
    solana_sdk::{clock::Slot, genesis_config::DEFAULT_GENESIS_ARCHIVE},
    std::{
        fs::{self, File},
        io::{self, Read},
        net::SocketAddr,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    },
};

static TRUCK: Emoji = Emoji("🚚 ", "");
static SPARKLE: Emoji = Emoji("✨ ", "");

/// Creates a new process bar for processing that will take an unknown amount of time
fn new_spinner_progress_bar() -> ProgressBar {
    let progress_bar = ProgressBar::new(42);
    progress_bar.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {wide_msg}")
            .expect("ProgresStyle::template direct input to be correct"),
    );
    progress_bar.enable_steady_tick(Duration::from_millis(100));
    progress_bar
}

/// Structure modeling information about download progress
#[derive(Debug)]
pub struct DownloadProgressRecord {
    // Duration since the beginning of the download
    pub elapsed_time: Duration,
    // Duration since the the last notification
    pub last_elapsed_time: Duration,
    // the bytes/sec speed measured for the last notification period
    pub last_throughput: f32,
    // the bytes/sec speed measured from the beginning
    pub total_throughput: f32,
    // total bytes of the download
    pub total_bytes: usize,
    // bytes downloaded so far
    pub current_bytes: usize,
    // percentage downloaded
    pub percentage_done: f32,
    // Estimated remaining time (in seconds) to finish the download if it keeps at the the last download speed
    pub estimated_remaining_time: f32,
    // The times of the progress is being notified, it starts from 1 and increments by 1 each time
    pub notification_count: u64,
}

type DownloadProgressCallback<'a> = Box<dyn FnMut(&DownloadProgressRecord) -> bool + 'a>;
type DownloadProgressCallbackOption<'a> = Option<DownloadProgressCallback<'a>>;

/// This callback allows the caller to get notified of the download progress modelled by DownloadProgressRecord
/// Return "true" to continue the download
/// Return "false" to abort the download
pub fn download_file<'a, 'b>(
    url: &str,
    destination_file: &Path,
    use_progress_bar: bool,
    progress_notify_callback: &'a mut DownloadProgressCallbackOption<'b>,
) -> Result<(), String> {
    if destination_file.is_file() {
        return Err(format!("{destination_file:?} already exists"));
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
        progress_bar.set_message(format!("{TRUCK}Downloading {url}..."));
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
                .template(
                    "{spinner:.green}{msg_wide}[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})",
                )
                .expect("ProgresStyle::template direct input to be correct")
                .progress_chars("=> "),
        );
        progress_bar.set_message(format!("{TRUCK}Downloading~ {url}"));
    } else {
        info!("Downloading {} bytes from {}", download_size, url);
    }

    struct DownloadProgress<'e, 'f, R> {
        progress_bar: ProgressBar,
        response: R,
        last_print: Instant,
        current_bytes: usize,
        last_print_bytes: usize,
        download_size: f32,
        use_progress_bar: bool,
        start_time: Instant,
        callback: &'f mut DownloadProgressCallbackOption<'e>,
        notification_count: u64,
    }

    impl<'e, 'f, R: Read> Read for DownloadProgress<'e, 'f, R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.response.read(buf)?;

            self.current_bytes += n;
            let total_bytes_f32 = self.current_bytes as f32;
            let diff_bytes_f32 = (self.current_bytes - self.last_print_bytes) as f32;
            let last_throughput = diff_bytes_f32 / self.last_print.elapsed().as_secs_f32();
            let estimated_remaining_time = if last_throughput > 0_f32 {
                (self.download_size - self.current_bytes as f32) / last_throughput
            } else {
                f32::MAX
            };

            let mut progress_record = DownloadProgressRecord {
                elapsed_time: self.start_time.elapsed(),
                last_elapsed_time: self.last_print.elapsed(),
                last_throughput,
                total_throughput: self.current_bytes as f32
                    / self.start_time.elapsed().as_secs_f32(),
                total_bytes: self.download_size as usize,
                current_bytes: self.current_bytes,
                percentage_done: 100f32 * (total_bytes_f32 / self.download_size),
                estimated_remaining_time,
                notification_count: self.notification_count,
            };
            let mut to_update_progress = false;
            if progress_record.last_elapsed_time.as_secs() > 5 {
                self.last_print = Instant::now();
                self.last_print_bytes = self.current_bytes;
                to_update_progress = true;
                self.notification_count += 1;
                progress_record.notification_count = self.notification_count
            }

            if self.use_progress_bar {
                self.progress_bar.inc(n as u64);
            } else if to_update_progress {
                info!(
                    "downloaded {} bytes {:.1}% {:.1} bytes/s",
                    self.current_bytes,
                    progress_record.percentage_done,
                    progress_record.last_throughput,
                );
            }

            if let Some(callback) = self.callback {
                if to_update_progress && !callback(&progress_record) {
                    info!("Download is aborted by the caller");
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "Download is aborted by the caller",
                    ));
                }
            }

            Ok(n)
        }
    }

    let mut source = DownloadProgress::<'b, 'a> {
        progress_bar,
        response,
        last_print: Instant::now(),
        current_bytes: 0,
        last_print_bytes: 0,
        download_size: (download_size as f32).max(1f32),
        use_progress_bar,
        start_time: Instant::now(),
        callback: progress_notify_callback,
        notification_count: 0,
    };

    File::create(&temp_destination_file)
        .and_then(|mut file| std::io::copy(&mut source, &mut file))
        .map_err(|err| format!("Unable to write {temp_destination_file:?}: {err:?}"))?;

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
        .map_err(|err| format!("Unable to rename: {err:?}"))?;

    Ok(())
}

pub fn download_genesis_if_missing(
    rpc_addr: &SocketAddr,
    genesis_package: &Path,
    use_progress_bar: bool,
) -> Result<PathBuf, String> {
    if !genesis_package.exists() {
        let tmp_genesis_path = genesis_package.parent().unwrap().join("tmp-genesis");
        let tmp_genesis_package = tmp_genesis_path.join(DEFAULT_GENESIS_ARCHIVE);

        let _ignored = fs::remove_dir_all(&tmp_genesis_path);
        download_file(
            &format!("http://{rpc_addr}/{DEFAULT_GENESIS_ARCHIVE}"),
            &tmp_genesis_package,
            use_progress_bar,
            &mut None,
        )?;

        Ok(tmp_genesis_package)
    } else {
        Err("genesis already exists".to_string())
    }
}

/// Download a snapshot archive from `rpc_addr`.  Use `snapshot_type` to specify downloading either
/// a full snapshot or an incremental snapshot.
pub fn download_snapshot_archive(
    rpc_addr: &SocketAddr,
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    desired_snapshot_hash: (Slot, SnapshotHash),
    snapshot_type: SnapshotType,
    maximum_full_snapshot_archives_to_retain: usize,
    maximum_incremental_snapshot_archives_to_retain: usize,
    use_progress_bar: bool,
    progress_notify_callback: &mut DownloadProgressCallbackOption<'_>,
) -> Result<(), String> {
    snapshot_utils::purge_old_snapshot_archives(
        full_snapshot_archives_dir,
        incremental_snapshot_archives_dir,
        maximum_full_snapshot_archives_to_retain,
        maximum_incremental_snapshot_archives_to_retain,
    );

    let snapshot_archives_remote_dir =
        snapshot_utils::build_snapshot_archives_remote_dir(match snapshot_type {
            SnapshotType::FullSnapshot => full_snapshot_archives_dir,
            SnapshotType::IncrementalSnapshot(_) => incremental_snapshot_archives_dir,
        });
    fs::create_dir_all(&snapshot_archives_remote_dir).unwrap();

    for archive_format in [
        ArchiveFormat::TarZstd,
        ArchiveFormat::TarGzip,
        ArchiveFormat::TarBzip2,
        ArchiveFormat::TarLz4,
        ArchiveFormat::Tar, // `solana-test-validator` creates uncompressed snapshots
    ] {
        let destination_path = match snapshot_type {
            SnapshotType::FullSnapshot => snapshot_utils::build_full_snapshot_archive_path(
                &snapshot_archives_remote_dir,
                desired_snapshot_hash.0,
                &desired_snapshot_hash.1,
                archive_format,
            ),
            SnapshotType::IncrementalSnapshot(base_slot) => {
                snapshot_utils::build_incremental_snapshot_archive_path(
                    &snapshot_archives_remote_dir,
                    base_slot,
                    desired_snapshot_hash.0,
                    &desired_snapshot_hash.1,
                    archive_format,
                )
            }
        };

        if destination_path.is_file() {
            return Ok(());
        }

        match download_file(
            &format!(
                "http://{}/{}",
                rpc_addr,
                destination_path.file_name().unwrap().to_str().unwrap()
            ),
            &destination_path,
            use_progress_bar,
            progress_notify_callback,
        ) {
            Ok(()) => return Ok(()),
            Err(err) => info!("{}", err),
        }
    }
    Err(format!(
        "Failed to download a snapshot archive for slot {} from {}",
        desired_snapshot_hash.0, rpc_addr
    ))
}
