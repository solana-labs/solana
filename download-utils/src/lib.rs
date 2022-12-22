#![allow(clippy::integer_arithmetic)]
use {
    console::Emoji,
    indicatif::{ProgressBar, ProgressStyle},
    log::*,
    rayon::prelude::*,
    reqwest::{blocking::Response, header::RANGE},
    solana_runtime::{
        snapshot_hash::SnapshotHash,
        snapshot_package::SnapshotType,
        snapshot_utils::{self, ArchiveFormat},
    },
    solana_sdk::{clock::Slot, genesis_config::DEFAULT_GENESIS_ARCHIVE},
    std::{
        fs::{self, File, OpenOptions},
        io::{self, Read, Write},
        net::SocketAddr,
        ops::Range,
        path::{Path, PathBuf},
        sync::RwLock,
        time::{Duration, Instant},
    },
};

static TRUCK: Emoji = Emoji("🚚 ", "");
static SPARKLE: Emoji = Emoji("✨ ", "");
const DOWNLOAD_CHUNK_SIZE_BYTES: u64 = 1024 * 1024 * 1024;

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

#[derive(Clone, Default)]
pub struct SnapshotAbortControl {
    minimal_snapshot_download_speed: f32,
    single_known_rpc: bool,
    abort_allowed: bool,
}

pub struct ProgressCallback {
    pub minimal_snapshot_download_speed: f32,
    pub maximum_snapshot_download_abort: u64,
    pub single_known_rpc: bool,
}

struct SnapshotResults {
    completed_chunks: Vec<(PathBuf, u64)>,
}

fn progress_callback(
    download_progress: &DownloadProgressRecord,
    snapshot_abort_control: &SnapshotAbortControl,
) -> bool {
    debug!("Download progress: {:?}", download_progress);
    if download_progress.last_throughput < snapshot_abort_control.minimal_snapshot_download_speed
        && download_progress.notification_count <= 1
        && download_progress.percentage_done <= 2_f32
        && download_progress.estimated_remaining_time > 60_f32
        && snapshot_abort_control.abort_allowed
    {
        if snapshot_abort_control.single_known_rpc {
            println!(
                "The snapshot download is too slow, throughput: {} < min speed {} \
                bytes/sec, but will NOT abort and try a different node as it is the \
                only known validator and the --only-known-rpc flag is set. \
                Progress detail: {:?}",
                download_progress.last_throughput,
                snapshot_abort_control.minimal_snapshot_download_speed,
                download_progress,
            );
            return true; // Do not abort download from the one-and-only known validator
        }
        println!(
            "The snapshot download is too slow, throughput: {} < min speed {} \
            bytes/sec, will abort and try a different node. \
            Progress detail: {:?}",
            download_progress.last_throughput,
            snapshot_abort_control.minimal_snapshot_download_speed,
            download_progress,
        );
        false
    } else {
        true
    }
}

fn verify_response_range(range: Option<Range<u64>>, response: &Response) -> Result<(), String> {
    if let Some(range) = range {
        let request_range = format!("bytes={}-{}", range.start, range.end);
        let response_range = response.headers().get(reqwest::header::CONTENT_RANGE);
        if let Some(response_range) = response_range {
            let x = response_range.to_str();
            match x {
                Ok(range) => {
                    if range == request_range {
                        return Ok(());
                    }
                }
                Err(_err) => (),
            }
        }
        Err("Didn't receive requested download range".to_string())
    } else {
        Ok(())
    }
}

/// This callback allows the caller to get notified of the download progress modelled by DownloadProgressRecord
/// Return "true" to continue the download
/// Return "false" to abort the download
pub fn download_file(
    url: &str,
    destination_file: &Path,
    use_progress_bar: bool,
    range: Option<Range<u64>>,
    snapshot_abort_control: Option<SnapshotAbortControl>,
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

    let mut request = reqwest::blocking::Client::new().get(url);
    if let Some(range) = range.clone() {
        let header_value = format!("bytes={}-{}", range.start, range.end);
        request = request.header(RANGE, header_value);
    }
    let response = request
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| {
            progress_bar.finish_and_clear();
            err.to_string()
        })?;
    verify_response_range(range, &response)?;

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

    struct DownloadProgress<R> {
        progress_bar: ProgressBar,
        response: R,
        last_print: Instant,
        current_bytes: usize,
        last_print_bytes: usize,
        download_size: f32,
        use_progress_bar: bool,
        start_time: Instant,
        notification_count: u64,
        snapshot_abort_control: Option<SnapshotAbortControl>,
    }

    impl<R: Read> Read for DownloadProgress<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut n = self.response.read(buf)?;
            n = n.min(self.download_size as usize - self.current_bytes);

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

            if let Some(snapshot_abort_control) = &self.snapshot_abort_control {
                if to_update_progress
                    && !progress_callback(&progress_record, snapshot_abort_control)
                {
                    info!("Download is aborted by the caller!!");
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "Download is aborted by the caller",
                    ));
                }
            }

            Ok(n)
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
        start_time: Instant::now(),
        notification_count: 0,
        snapshot_abort_control,
    };

    File::create(&temp_destination_file)
        .and_then(|mut file| std::io::copy(&mut source, &mut file))
        .map_err(|err| format!("Unable to write {temp_destination_file:?}: {err:?}"))?;

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
            None,
            None,
        )?;

        Ok(tmp_genesis_package)
    } else {
        Err("genesis already exists".to_string())
    }
}

fn get_snapshot_size(rpc_addrs: Vec<SocketAddr>, src_path: PathBuf) -> Option<u64> {
    let url = &format!(
        "http://{}/{}",
        rpc_addrs[0],
        src_path.file_name().unwrap().to_str().unwrap()
    );
    let response = reqwest::blocking::Client::new()
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|err| {
            println!("ERROR response {}", err);
            err.to_string()
        });
    if let Ok(response) = response {
        let snapshot_size = {
            response
                .headers()
                .get(reqwest::header::CONTENT_LENGTH)
                .and_then(|content_length| content_length.to_str().ok())
                .and_then(|content_length| content_length.parse().ok())
                .unwrap_or(0)
        };
        Some(snapshot_size as u64)
    } else {
        None
    }
}

fn coalesce_temp_files(mut temp_dest_paths: Vec<(u64, PathBuf)>, dest_path: PathBuf) {
    let mut file_dest = OpenOptions::new()
        .append(true)
        .create(true)
        .open(dest_path.clone())
        .expect("Unable to open destination file");

    if temp_dest_paths.len() > 1 {
        temp_dest_paths.sort();
        for (_, temp_dest_path) in temp_dest_paths {
            let mut file_dest_temp = OpenOptions::new()
                .read(true)
                .open(temp_dest_path.clone())
                .unwrap_or_else(|_| {
                    panic!(
                        "Unable to open file {} for reading",
                        temp_dest_path.display()
                    )
                });
            let mut buf = vec![];
            file_dest_temp.read_to_end(&mut buf).unwrap_or_else(|_| {
                panic!(
                    "Unable to read all contents from file {}",
                    temp_dest_path.display()
                )
            });
            file_dest.write_all(&buf).unwrap_or_else(|_| {
                panic!(
                    "Unable to write all buffer contents to file {}",
                    dest_path.display()
                )
            });
            fs::remove_file(temp_dest_path.clone()).unwrap_or_else(|_| {
                panic!("failed to remove temp file {}", temp_dest_path.display())
            });
        }
    } else {
        let from = &temp_dest_paths[0].1;
        // special case optimization when we only have 1 file
        fs::rename(from, &dest_path).unwrap_or_else(|_| {
            panic!(
                "Unable to rename file {} to {}",
                from.display(),
                dest_path.display()
            )
        });
    }
}

fn populate_snapshot_download_start_offsets(
    download_start_offsets: &RwLock<(i32, Vec<u64>)>,
    snapshot_size: u64,
) {
    let mut download_start_offsets_l = download_start_offsets.write().unwrap();
    let mut start_offset = 0;
    loop {
        if start_offset >= snapshot_size {
            break;
        }
        download_start_offsets_l.0 += 1;
        download_start_offsets_l.1.push(start_offset);
        start_offset += DOWNLOAD_CHUNK_SIZE_BYTES;
    }
}

fn remove_temp_snapshot_files(temp_dest_paths: &Vec<(u64, PathBuf)>) {
    for (_, temp_file) in temp_dest_paths {
        if temp_file.is_file() {
            fs::remove_file(temp_file).expect("Failed to remove file");
        }
    }
}

fn parse_snapshot_download_results(
    results: &[Result<SnapshotResults, (SocketAddr, SnapshotResults)>],
    remaining_good_rpc_addrs: &mut Vec<SocketAddr>,
    download_abort_count: &mut u64,
    temp_dest_paths: &mut Vec<(u64, PathBuf)>,
) {
    for result in results {
        let snapshot_results = match result {
            Err((rpc_addr, snapshot_results)) => {
                let idx = remaining_good_rpc_addrs
                    .iter()
                    .position(|x| *x == *rpc_addr)
                    .unwrap();
                remaining_good_rpc_addrs.remove(idx);
                *download_abort_count += 1;
                snapshot_results
            }
            Ok(snapshot_results) => snapshot_results,
        };

        for (dest_path, start) in &snapshot_results.completed_chunks {
            temp_dest_paths.push((*start, dest_path.to_path_buf()));
        }
    }
}

fn get_snapshot_download_start_offset(
    download_start_offsets: &RwLock<(i32, Vec<u64>)>,
) -> Result<u64, bool> {
    if let Ok(mut download_start_offsets_l) = download_start_offsets.write() {
        if let Some(start) = download_start_offsets_l.1.pop() {
            Ok(start)
        } else if download_start_offsets_l.0 > 0 {
            Err(false)
        } else {
            // Nothing more to do!
            Err(true)
        }
    } else {
        panic!("Couldn't get write lock");
    }
}

/// Download a snapshot archive from `rpc_addr`.  Use `snapshot_type` to specify downloading either
/// a full snapshot or an incremental snapshot.
#[allow(clippy::too_many_arguments)]
pub fn download_snapshot_archive(
    rpc_addrs: &[SocketAddr],
    full_snapshot_archives_dir: &Path,
    incremental_snapshot_archives_dir: &Path,
    desired_snapshot_hash: (Slot, SnapshotHash),
    snapshot_type: SnapshotType,
    maximum_full_snapshot_archives_to_retain: usize,
    maximum_incremental_snapshot_archives_to_retain: usize,
    use_progress_bar: bool,
    progress_callback: Option<ProgressCallback>,
    download_abort_count: &mut u64,
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

    let snapshot_abort_control =
        progress_callback
            .as_ref()
            .map(|progress_callback| SnapshotAbortControl {
                minimal_snapshot_download_speed: progress_callback.minimal_snapshot_download_speed,
                single_known_rpc: progress_callback.single_known_rpc,
                abort_allowed: *download_abort_count
                    < progress_callback.maximum_snapshot_download_abort,
            });

    for archive_format in [
        ArchiveFormat::TarZstd,
        ArchiveFormat::TarGzip,
        ArchiveFormat::TarBzip2,
        ArchiveFormat::TarLz4,
        ArchiveFormat::Tar, // `solana-test-validator` creates uncompressed snapshots
    ] {
        let path = snapshot_utils::build_snapshot_archive_path(
            snapshot_type,
            &snapshot_archives_remote_dir,
            desired_snapshot_hash.0,
            &desired_snapshot_hash.1,
            archive_format,
            None,
        );

        let snapshot_size = match get_snapshot_size(rpc_addrs.to_owned(), path.clone()) {
            Some(snapshot_size) => snapshot_size,
            None => continue,
        };
        let download_start_offsets = RwLock::new((0, Vec::<u64>::new()));
        populate_snapshot_download_start_offsets(&download_start_offsets, snapshot_size);

        let mut remaining_good_rpc_addrs = rpc_addrs.to_owned();
        let results: Vec<Result<SnapshotResults, (SocketAddr, SnapshotResults)>> =
            remaining_good_rpc_addrs
                .clone()
                .into_par_iter()
                .map(|rpc_addr| {
                    let mut completed_chunks = vec![];
                    loop {
                        let start =
                            match get_snapshot_download_start_offset(&download_start_offsets) {
                                Ok(start) => start,
                                Err(done) => {
                                    if done {
                                        // Nothing more to do!
                                        return Ok(SnapshotResults { completed_chunks });
                                    } else {
                                        // Wait to see if we need to pickup work from failed RPCs
                                        std::thread::sleep(Duration::from_millis(1_000));
                                        continue;
                                    }
                                }
                            };

                        let end = snapshot_size.min(start + DOWNLOAD_CHUNK_SIZE_BYTES) - 1;
                        let destination_path = snapshot_utils::build_snapshot_archive_path(
                            snapshot_type,
                            snapshot_archives_remote_dir.clone(),
                            desired_snapshot_hash.0,
                            &desired_snapshot_hash.1,
                            archive_format,
                            Some(start),
                        );

                        if destination_path.is_file() {
                            // Already exists! No need to download again
                            download_start_offsets.write().unwrap().0 -= 1;
                            completed_chunks.push((destination_path, start));
                            continue;
                        }

                        match download_file(
                            &format!(
                                "http://{}/{}",
                                rpc_addr,
                                path.file_name().unwrap().to_str().unwrap()
                            ),
                            &destination_path,
                            use_progress_bar,
                            Some(Range { start, end }),
                            snapshot_abort_control.clone(),
                        ) {
                            Ok(()) => {
                                download_start_offsets.write().unwrap().0 -= 1;
                                completed_chunks.push((destination_path, start));
                            }
                            Err(_err) => {
                                if destination_path.is_file() {
                                    fs::remove_file(destination_path)
                                        .expect("Failed to remove file");
                                }
                                // Push this range back onto the queue so another validator can pick it up.
                                download_start_offsets.write().unwrap().1.push(start);
                                return Err((rpc_addr, SnapshotResults { completed_chunks }));
                            }
                        }
                    }
                })
                .collect();

        let mut temp_dest_paths = vec![];
        parse_snapshot_download_results(
            &results,
            &mut remaining_good_rpc_addrs,
            download_abort_count,
            &mut temp_dest_paths,
        );

        if download_start_offsets.read().unwrap().0 > 0 {
            // Didn't download all the chunks successfully.
            remove_temp_snapshot_files(&temp_dest_paths);
            continue;
        }

        coalesce_temp_files(temp_dest_paths, path);
        return Ok(());
    }

    Err(format!(
        "Failed to download a snapshot archive for slot {}",
        desired_snapshot_hash.0,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_file_coalesce() {
        // Create 1MB buffer for rapidly filling temporary files.
        let buffer = vec![0_u8; 1024 * 1024];

        // Create large number of temporary files.
        let temp_file_paths = vec![
            "./temp1", "./temp2", "./temp3", "./temp4", "./temp5", "./temp6", "./temp7", "./temp8",
            "./temp9", "./temp10", "./temp11", "./temp12", "./temp13", "./temp14", "./temp15",
            "./temp16", "./temp17", "./temp18", "./temp19", "./temp20", "./temp21", "./temp22",
            "./temp23", "./temp24", "./temp25", "./temp26", "./temp27", "./temp28", "./temp29",
            "./temp30", "./temp31", "./temp32",
        ];
        for file_path in &temp_file_paths {
            let mut file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(*file_path)
                .expect("Unable to open file for writing");
            while file.metadata().unwrap().len() < 1024 * 1024 * 1024 {
                file.write_all(&buffer)
                    .expect("Failed to write buffer to file");
            }
        }

        // Coalesce temporary files into single, large output file.
        let mut temp_dest_paths = vec![];
        temp_file_paths
            .iter()
            .enumerate()
            .for_each(|(i, file_path)| {
                temp_dest_paths.push((i as u64, PathBuf::from(*file_path)));
            });
        const OUTPUT_FILE: &str = "./output_file";
        coalesce_temp_files(temp_dest_paths, PathBuf::from(OUTPUT_FILE));

        // Cleanup output file.
        fs::remove_file(OUTPUT_FILE)
            .unwrap_or_else(|_| panic!("failed to remove file {}", OUTPUT_FILE));
    }
}
