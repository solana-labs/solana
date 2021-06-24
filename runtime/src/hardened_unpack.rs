use solana_measure::measure::Measure;
use solana_sdk::genesis_config::{DEFAULT_GENESIS_ARCHIVE, DEFAULT_GENESIS_FILE};
use {
    bzip2::bufread::BzDecoder,
    log::*,
    rand::{thread_rng, Rng},
    solana_sdk::genesis_config::GenesisConfig,
    std::{
        collections::HashMap,
        fs::{self, File},
        io::{BufReader, Read},
        path::{
            Component::{CurDir, Normal},
            Path, PathBuf,
        },
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex, RwLock,
        },
        thread::Builder,
        time::Instant,
    },
    tar::{
        Archive,
        EntryType::{Directory, GNUSparse, Regular},
    },
    thiserror::Error,
};

#[derive(Error, Debug)]
pub enum UnpackError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Archive error: {0}")]
    Archive(String),
}

pub type Result<T> = std::result::Result<T, UnpackError>;

// 64 TiB; some safe margin to the max 128 TiB in amd64 linux userspace VmSize
// (ref: https://unix.stackexchange.com/a/386555/364236)
// note that this is directly related to the mmaped data size
// so protect against insane value
// This is the file size including holes for sparse files
const MAX_SNAPSHOT_ARCHIVE_UNPACKED_APPARENT_SIZE: u64 = 64 * 1024 * 1024 * 1024 * 1024;

// 4 TiB;
// This is the actually consumed disk usage for sparse files
const MAX_SNAPSHOT_ARCHIVE_UNPACKED_ACTUAL_SIZE: u64 = 4 * 1024 * 1024 * 1024 * 1024;

const MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT: u64 = 5_000_000;
pub const MAX_GENESIS_ARCHIVE_UNPACKED_SIZE: u64 = 10 * 1024 * 1024; // 10 MiB
const MAX_GENESIS_ARCHIVE_UNPACKED_COUNT: u64 = 100;

fn checked_total_size_sum(total_size: u64, entry_size: u64, limit_size: u64) -> Result<u64> {
    trace!(
        "checked_total_size_sum: {} + {} < {}",
        total_size,
        entry_size,
        limit_size,
    );
    let total_size = total_size.saturating_add(entry_size);
    if total_size > limit_size {
        return Err(UnpackError::Archive(format!(
            "too large archive: {} than limit: {}",
            total_size, limit_size,
        )));
    }
    Ok(total_size)
}

fn checked_total_count_increment(total_count: u64, limit_count: u64) -> Result<u64> {
    let total_count = total_count + 1;
    if total_count > limit_count {
        return Err(UnpackError::Archive(format!(
            "too many files in snapshot: {:?}",
            total_count
        )));
    }
    Ok(total_count)
}

fn check_unpack_result(unpack_result: bool, path: String) -> Result<()> {
    if !unpack_result {
        return Err(UnpackError::Archive(format!(
            "failed to unpack: {:?}",
            path
        )));
    }
    Ok(())
}
pub struct SeekableBufferingReaderInner {
    // unpacking callers read from 'data'. Data is transferred when 'data' is exhausted.
    // This minimizes lock contention since bg file reader has to have almost constant write access.
    pub data: RwLock<Vec<Vec<u8>>>,
    // bg thread reads to 'new_data'
    pub new_data: RwLock<Vec<Vec<u8>>>,
    pub len: AtomicUsize,
    pub calls: AtomicUsize,
    pub error: Mutex<io::Result<()>>,
}

use std::io;

pub struct SeekableBufferingReader {
    pub instance: Arc<SeekableBufferingReaderInner>,
    pub pos: usize,
    pub last_buffer_index: usize,
    pub next_index_within_last_buffer: usize,
}

impl Clone for SeekableBufferingReader {
    fn clone(&self) -> Self {
        Self {
            instance: Arc::clone(&self.instance),
            pos: 0,
            last_buffer_index: 0,
            next_index_within_last_buffer: 0,
        }
    }
}

impl SeekableBufferingReader {
    pub fn new<T: 'static + Read + std::marker::Send>(mut reader: T) -> Self {
        let inner = SeekableBufferingReaderInner {
            new_data: RwLock::new(vec![]),
            data: RwLock::new(vec![]),
            len: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            error: Mutex::new(Ok(())),
        };
        let result = Self {
            instance: Arc::new(inner),
            pos: 0,
            last_buffer_index: 0,
            next_index_within_last_buffer: 0,
        };

        let result_ = result.clone();

        let handle = Builder::new()
            .name("solana-compressed_file_reader".to_string())
            .spawn(move || {
                let mut time = Measure::start("");
                const SIZE: usize = 65536 * 2;
                let mut data = [0u8; SIZE];
                loop {
                    let result = reader.read(&mut data);
                    match result {
                        Ok(size) => {
                            result_
                                .instance
                                .new_data
                                .write()
                                .unwrap()
                                .push(data[0..size].to_vec());
                            let len = result_.instance.len.fetch_add(size, Ordering::Relaxed);
                            if size == 0 {
                                break;
                            }
                        }
                        Err(err) => {
                            error!("error reading file");
                            *result_.instance.error.lock().unwrap() = Err(err);
                            break;
                        }
                    }
                }
                time.stop();
                error!(
                    "reading entire decompressed file took: {} us, bytes: {}",
                    time.as_us(),
                    result_.instance.len.load(Ordering::Relaxed)
                );
            });
        std::thread::sleep(std::time::Duration::from_millis(200)); // give time for file to be read a little bit
        result
    }
    fn transfer_data(&self) {
        let mut from_lock = self.instance.new_data.write().unwrap();
        if from_lock.is_empty() {
            return;
        }
        let mut new_data: Vec<Vec<u8>> = vec![];
        std::mem::swap(&mut *from_lock, &mut new_data);
        drop(from_lock);
        let mut to_lock = self.instance.data.write().unwrap();
        to_lock.append(&mut new_data);
    }
    pub fn calls(&self) -> usize {
        self.instance.calls.load(Ordering::Relaxed)
    }
    pub fn len(&self) -> usize {
        self.instance.len.load(Ordering::Relaxed)
    }
}

impl Read for SeekableBufferingReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let request_len = buf.len();

        let mut remaining_request = request_len;
        let mut offset_in_dest = 0;
        let mut transferred_data = false;
        while remaining_request > 0 {
            let mut lock = self.instance.data.read().unwrap();
            if self.last_buffer_index >= lock.len() {
                if !transferred_data {
                    transferred_data = true;
                    self.transfer_data();
                    continue;
                }
                break; // no more to read right now
            }
            let source = &lock[self.last_buffer_index];
            let full_len = source.len();
            let remaining_len = full_len - self.next_index_within_last_buffer;
            if remaining_len >= remaining_request {
                let bytes_to_transfer = remaining_request;
                buf[offset_in_dest..(offset_in_dest + bytes_to_transfer)].copy_from_slice(
                    &source[self.next_index_within_last_buffer
                        ..(self.next_index_within_last_buffer + bytes_to_transfer)],
                );
                self.next_index_within_last_buffer += bytes_to_transfer;
                offset_in_dest += bytes_to_transfer;
                remaining_request -= bytes_to_transfer;
            } else {
                let bytes_to_transfer = remaining_len;
                buf[offset_in_dest..(offset_in_dest + bytes_to_transfer)].copy_from_slice(
                    &source[self.next_index_within_last_buffer
                        ..(self.next_index_within_last_buffer + bytes_to_transfer)],
                );
                offset_in_dest += bytes_to_transfer;
                self.next_index_within_last_buffer = 0;
                self.last_buffer_index += 1;
                remaining_request -= bytes_to_transfer;
            }
        }

        self.instance.calls.fetch_add(1, Ordering::Relaxed);
        Ok(offset_in_dest)
    }
}

fn unpack_archive<'a, A: Read, C, D>(
    buf: Option<&'a mut SeekableBufferingReader>,
    archive: &'a mut Archive<A>,
    apparent_limit_size: u64,
    actual_limit_size: u64,
    limit_count: u64,
    mut entry_checker: C,
    mut file_notifier: D,
) -> Result<()>
where
    C: FnMut(&[&str], tar::EntryType) -> Option<(&'a Path, bool)>,
    D: FnMut(PathBuf, bool),
{
    let mut apparent_total_size: u64 = 0;
    let mut actual_total_size: u64 = 0;
    let mut total_count: u64 = 0;

    let mut total_entries = 0;
    let mut last_log_update = Instant::now();
    error!("unpack_archive");
    let entries = archive.entries();
    if entries.is_err() {}
    for entry in entries? {
        let mut entry = entry?;
        let path = entry.path();
        if path.is_err() {}
        let path = path?;
        let path_str = path.display().to_string();

        // Although the `tar` crate safely skips at the actual unpacking, fail
        // first by ourselves when there are odd paths like including `..` or /
        // for our clearer pattern matching reasoning:
        //   https://docs.rs/tar/0.4.26/src/tar/entry.rs.html#371
        let parts = path.components().map(|p| match p {
            CurDir => Some("."),
            Normal(c) => c.to_str(),
            _ => None, // Prefix (for Windows) and RootDir are forbidden
        });

        // Reject old-style BSD directory entries that aren't explicitly tagged as directories
        let legacy_dir_entry =
            entry.header().as_ustar().is_none() && entry.path_bytes().ends_with(b"/");
        let kind = entry.header().entry_type();
        let reject_legacy_dir_entry = legacy_dir_entry && (kind != Directory);

        if parts.clone().any(|p| p.is_none()) || reject_legacy_dir_entry {
            return Err(UnpackError::Archive(format!(
                "invalid path found: {:?}",
                path_str
            )));
        }

        let parts: Vec<_> = parts.map(|p| p.unwrap()).collect::<Vec<_>>().clone();
        let mut result_bool = false;
        let res = entry_checker(parts.as_slice(), kind);
        if res.is_none() {
            continue;
        }
        let unpack_dir = match res {
            None => {
                return Err(UnpackError::Archive(format!(
                    "extra entry found: {:?} {:?}",
                    path_str,
                    entry.header().entry_type(),
                )));
            }
            Some((unpack_dir, result_bool_in)) => {
                result_bool = result_bool_in;
                unpack_dir
            }
        };

        apparent_total_size = checked_total_size_sum(
            apparent_total_size,
            entry.header().size()?,
            apparent_limit_size,
        )?;
        actual_total_size = checked_total_size_sum(
            actual_total_size,
            entry.header().entry_size()?,
            actual_limit_size,
        )?;
        total_count = checked_total_count_increment(total_count, limit_count)?;

        let pb = unpack_dir.join(entry.path()?).to_path_buf();

        // unpack_in does its own sanitization
        // ref: https://docs.rs/tar/*/tar/struct.Entry.html#method.unpack_in
        check_unpack_result(entry.unpack_in(unpack_dir)?, path_str)?;
        let start = entry.raw_file_position();
        let len = entry.size();
        // Sanitize permissions.
        let mode = match entry.header().entry_type() {
            GNUSparse | Regular => 0o644,
            _ => 0o755,
        };
        set_perms(&unpack_dir.join(entry.path()?), mode)?;
        file_notifier(pb, result_bool);

        total_entries += 1;
        let now = Instant::now();
        if now.duration_since(last_log_update).as_secs() >= 10 {
            info!("unpacked {} entries so far...", total_entries);
            last_log_update = now;
        }
    }
    if let Some(buf) = buf {
        let len = buf.len();
        let calls = std::cmp::max(1, buf.calls());
        info!(
            "unpacked {} entries total, raw bytes: {}, calls: {}, bytes/call: {}",
            total_entries,
            len,
            calls,
            len / calls
        );
    }

    return Ok(());

    #[cfg(unix)]
    fn set_perms(dst: &Path, mode: u32) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let perm = fs::Permissions::from_mode(mode as _);
        fs::set_permissions(dst, perm)
    }

    #[cfg(windows)]
    fn set_perms(dst: &Path, _mode: u32) -> std::io::Result<()> {
        let mut perm = fs::metadata(dst)?.permissions();
        perm.set_readonly(false);
        fs::set_permissions(dst, perm)
    }
}

/// Map from AppendVec file name to unpacked file system location
pub type UnpackedAppendVecMap = HashMap<String, PathBuf>;

use crossbeam_channel::Sender;

pub fn unpack_snapshot<A: Read>(
    buf: Option<&mut SeekableBufferingReader>,
    archive: &mut Archive<A>,
    ledger_dir: &Path,
    account_paths: &[PathBuf], // put a channel here...
    account_path_sender: Option<&Sender<PathBuf>>,
    accounts: bool,
    disable: bool,
    amod: Option<(usize, usize)>,
) -> Result<UnpackedAppendVecMap> {
    assert!(!account_paths.is_empty());
    let mut unpacked_append_vec_map = UnpackedAppendVecMap::new();
    let mut i = 0;

    unpack_archive(
        buf,
        archive,
        MAX_SNAPSHOT_ARCHIVE_UNPACKED_APPARENT_SIZE,
        MAX_SNAPSHOT_ARCHIVE_UNPACKED_ACTUAL_SIZE,
        MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT,
        |parts, kind| {
            if disable {
                None
            } else {
                if is_valid_snapshot_archive_entry(parts, kind) {
                    if let ["accounts", file] = parts {
                        if accounts {
                            i += 1;
                            match &amod {
                                Some((idx, total)) => {
                                    if (i - 1) % total != *idx {
                                        return None;
                                    }
                                }
                                None => {}
                            };
                            // Randomly distribute the accounts files about the available `account_paths`,
                            let path_index = thread_rng().gen_range(0, account_paths.len());
                            account_paths
                                .get(path_index)
                                .map(|path_buf| {
                                    unpacked_append_vec_map.insert(
                                        file.to_string(),
                                        path_buf.join("accounts").join(file),
                                    );
                                    //error!("accounts: {:?}", ledger_dir);
                                    path_buf.as_path()
                                })
                                .map(|i| (i, true))
                        } else {
                            None
                        }
                    } else {
                        if accounts {
                            None
                        } else {
                            error!("path: {:?}, {:?}", ledger_dir, kind);
                            Some((ledger_dir, false))
                        }
                    }
                } else {
                    None
                }
            }
        },
        |path, account| {
            match &account_path_sender {
                Some(sender) => {
                    let _ = sender.send(path);
                }
                None => {
                    if account {
                        //error!("path: {:?}", path);
                    } else {
                        let metadata = fs::metadata(path.clone()).unwrap();
                        let filename = path.file_name().unwrap();
                        let subdir = path.parent().unwrap().file_name().unwrap();
                        error!(
                            "untar'd: path: {:?}, len: {}, {:?}, {:?}, {:?}",
                            path,
                            metadata.len(),
                            filename,
                            subdir,
                            filename == subdir
                        );
                    }
                }
            }
        },
    )
    .map(|_| unpacked_append_vec_map)
}

fn all_digits(v: &str) -> bool {
    if v.is_empty() {
        return false;
    }
    for x in v.chars() {
        if !x.is_numeric() {
            return false;
        }
    }
    true
}

fn like_storage(v: &str) -> bool {
    let mut periods = 0;
    let mut saw_numbers = false;
    for x in v.chars() {
        if !x.is_numeric() {
            if x == '.' {
                if periods > 0 || !saw_numbers {
                    return false;
                }
                saw_numbers = false;
                periods += 1;
            } else {
                return false;
            }
        } else {
            saw_numbers = true;
        }
    }
    saw_numbers && periods == 1
}

fn is_valid_snapshot_archive_entry(parts: &[&str], kind: tar::EntryType) -> bool {
    match (parts, kind) {
        (["version"], Regular) => true,
        (["accounts"], Directory) => true,
        (["accounts", file], GNUSparse) if like_storage(file) => true,
        (["accounts", file], Regular) if like_storage(file) => true,
        (["snapshots"], Directory) => true,
        (["snapshots", "status_cache"], GNUSparse) => true,
        (["snapshots", "status_cache"], Regular) => true,
        (["snapshots", dir, file], GNUSparse) if all_digits(dir) && all_digits(file) => true,
        (["snapshots", dir, file], Regular) if all_digits(dir) && all_digits(file) => true,
        (["snapshots", dir], Directory) if all_digits(dir) => true,
        _ => false,
    }
}

pub fn open_genesis_config(
    ledger_path: &Path,
    max_genesis_archive_unpacked_size: u64,
) -> GenesisConfig {
    GenesisConfig::load(ledger_path).unwrap_or_else(|load_err| {
        let genesis_package = ledger_path.join(DEFAULT_GENESIS_ARCHIVE);
        unpack_genesis_archive(
            &genesis_package,
            ledger_path,
            max_genesis_archive_unpacked_size,
        )
        .unwrap_or_else(|unpack_err| {
            warn!(
                "Failed to open ledger genesis_config at {:?}: {}, {}",
                ledger_path, load_err, unpack_err,
            );
            std::process::exit(1);
        });

        // loading must succeed at this moment
        GenesisConfig::load(ledger_path).unwrap()
    })
}

pub fn unpack_genesis_archive(
    archive_filename: &Path,
    destination_dir: &Path,
    max_genesis_archive_unpacked_size: u64,
) -> std::result::Result<(), UnpackError> {
    info!("Extracting {:?}...", archive_filename);
    let extract_start = Instant::now();

    fs::create_dir_all(destination_dir)?;
    let tar_bz2 = File::open(&archive_filename)?;
    let tar = BzDecoder::new(BufReader::new(tar_bz2));
    let mut archive = Archive::new(tar);
    unpack_genesis(
        &mut archive,
        destination_dir,
        max_genesis_archive_unpacked_size,
    )?;
    info!(
        "Extracted {:?} in {:?}",
        archive_filename,
        Instant::now().duration_since(extract_start)
    );
    Ok(())
}

fn unpack_genesis<A: Read>(
    archive: &mut Archive<A>,
    unpack_dir: &Path,
    max_genesis_archive_unpacked_size: u64,
) -> Result<()> {
    unpack_archive(
        None::<&mut SeekableBufferingReader>,
        archive,
        max_genesis_archive_unpacked_size,
        max_genesis_archive_unpacked_size,
        MAX_GENESIS_ARCHIVE_UNPACKED_COUNT,
        |p, k| {
            if is_valid_genesis_archive_entry(p, k) {
                Some((unpack_dir, false))
            } else {
                None
            }
        },
        |_, _| {},
    )
}

fn is_valid_genesis_archive_entry(parts: &[&str], kind: tar::EntryType) -> bool {
    trace!("validating: {:?} {:?}", parts, kind);
    #[allow(clippy::match_like_matches_macro)]
    match (parts, kind) {
        ([DEFAULT_GENESIS_FILE], GNUSparse) => true,
        ([DEFAULT_GENESIS_FILE], Regular) => true,
        (["rocksdb"], Directory) => true,
        (["rocksdb", _], GNUSparse) => true,
        (["rocksdb", _], Regular) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use tar::{Builder, Header};

    #[test]
    fn test_archive_is_valid_entry() {
        assert!(is_valid_snapshot_archive_entry(
            &["snapshots"],
            tar::EntryType::Directory
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["snapshots", ""],
            tar::EntryType::Directory
        ));
        assert!(is_valid_snapshot_archive_entry(
            &["snapshots", "3"],
            tar::EntryType::Directory
        ));
        assert!(is_valid_snapshot_archive_entry(
            &["snapshots", "3", "3"],
            tar::EntryType::Regular
        ));
        assert!(is_valid_snapshot_archive_entry(
            &["version"],
            tar::EntryType::Regular
        ));
        assert!(is_valid_snapshot_archive_entry(
            &["accounts"],
            tar::EntryType::Directory
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", ""],
            tar::EntryType::Regular
        ));

        assert!(!is_valid_snapshot_archive_entry(
            &["snapshots"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["snapshots", "x0"],
            tar::EntryType::Directory
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["snapshots", "0x"],
            tar::EntryType::Directory
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["snapshots", "0", "aa"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["aaaa"],
            tar::EntryType::Regular
        ));
    }

    #[test]
    fn test_valid_snapshot_accounts() {
        solana_logger::setup();
        assert!(is_valid_snapshot_archive_entry(
            &["accounts", "0.0"],
            tar::EntryType::Regular
        ));
        assert!(is_valid_snapshot_archive_entry(
            &["accounts", "01829.077"],
            tar::EntryType::Regular
        ));

        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", "1.2.34"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", "12."],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", ".12"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", "0x0"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", "abc"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_snapshot_archive_entry(
            &["accounts", "232323"],
            tar::EntryType::Regular
        ));
    }

    #[test]
    fn test_archive_is_valid_archive_entry() {
        assert!(is_valid_genesis_archive_entry(
            &["genesis.bin"],
            tar::EntryType::Regular
        ));
        assert!(is_valid_genesis_archive_entry(
            &["genesis.bin"],
            tar::EntryType::GNUSparse,
        ));
        assert!(is_valid_genesis_archive_entry(
            &["rocksdb"],
            tar::EntryType::Directory
        ));
        assert!(is_valid_genesis_archive_entry(
            &["rocksdb", "foo"],
            tar::EntryType::Regular
        ));
        assert!(is_valid_genesis_archive_entry(
            &["rocksdb", "foo"],
            tar::EntryType::GNUSparse,
        ));

        assert!(!is_valid_genesis_archive_entry(
            &["aaaa"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["aaaa"],
            tar::EntryType::GNUSparse,
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["rocksdb"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["rocksdb"],
            tar::EntryType::GNUSparse,
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["rocksdb", "foo"],
            tar::EntryType::Directory,
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["rocksdb", "foo", "bar"],
            tar::EntryType::Directory,
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["rocksdb", "foo", "bar"],
            tar::EntryType::Regular
        ));
        assert!(!is_valid_genesis_archive_entry(
            &["rocksdb", "foo", "bar"],
            tar::EntryType::GNUSparse
        ));
    }

    fn with_finalize_and_unpack<C>(archive: tar::Builder<Vec<u8>>, checker: C) -> Result<()>
    where
        C: Fn(&mut Archive<BufReader<&[u8]>>, &Path) -> Result<()>,
    {
        let data = archive.into_inner().unwrap();
        let reader = BufReader::new(&data[..]);
        let mut archive: Archive<std::io::BufReader<&[u8]>> = Archive::new(reader);
        let temp_dir = tempfile::TempDir::new().unwrap();

        checker(&mut archive, temp_dir.path())?;
        // Check that there is no bad permissions preventing deletion.
        let result = temp_dir.close();
        assert_matches!(result, Ok(()));
        Ok(())
    }

    fn finalize_and_unpack_snapshot(archive: tar::Builder<Vec<u8>>) -> Result<()> {
        with_finalize_and_unpack(archive, |a, b| {
            unpack_snapshot(a, b, &[PathBuf::new()]).map(|_| ())
        })
    }

    fn finalize_and_unpack_genesis(archive: tar::Builder<Vec<u8>>) -> Result<()> {
        with_finalize_and_unpack(archive, |a, b| {
            unpack_genesis(a, b, MAX_GENESIS_ARCHIVE_UNPACKED_SIZE)
        })
    }

    #[test]
    fn test_archive_unpack_snapshot_ok() {
        let mut header = Header::new_gnu();
        header.set_path("version").unwrap();
        header.set_size(4);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();

        let result = finalize_and_unpack_snapshot(archive);
        assert_matches!(result, Ok(()));
    }

    #[test]
    fn test_archive_unpack_genesis_ok() {
        let mut header = Header::new_gnu();
        header.set_path("genesis.bin").unwrap();
        header.set_size(4);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();

        let result = finalize_and_unpack_genesis(archive);
        assert_matches!(result, Ok(()));
    }

    #[test]
    fn test_archive_unpack_genesis_bad_perms() {
        let mut archive = Builder::new(Vec::new());

        let mut header = Header::new_gnu();
        header.set_path("rocksdb").unwrap();
        header.set_entry_type(Directory);
        header.set_size(0);
        header.set_cksum();
        let data: &[u8] = &[];
        archive.append(&header, data).unwrap();

        let mut header = Header::new_gnu();
        header.set_path("rocksdb/test").unwrap();
        header.set_size(4);
        header.set_cksum();
        let data: &[u8] = &[1, 2, 3, 4];
        archive.append(&header, data).unwrap();

        // Removing all permissions makes it harder to delete this directory
        // or work with files inside it.
        let mut header = Header::new_gnu();
        header.set_path("rocksdb").unwrap();
        header.set_entry_type(Directory);
        header.set_mode(0o000);
        header.set_size(0);
        header.set_cksum();
        let data: &[u8] = &[];
        archive.append(&header, data).unwrap();

        let result = finalize_and_unpack_genesis(archive);
        assert_matches!(result, Ok(()));
    }

    #[test]
    fn test_archive_unpack_genesis_bad_rocksdb_subdir() {
        let mut archive = Builder::new(Vec::new());

        let mut header = Header::new_gnu();
        header.set_path("rocksdb").unwrap();
        header.set_entry_type(Directory);
        header.set_size(0);
        header.set_cksum();
        let data: &[u8] = &[];
        archive.append(&header, data).unwrap();

        // tar-rs treats following entry as a Directory to support old tar formats.
        let mut header = Header::new_gnu();
        header.set_path("rocksdb/test/").unwrap();
        header.set_entry_type(Regular);
        header.set_size(0);
        header.set_cksum();
        let data: &[u8] = &[];
        archive.append(&header, data).unwrap();

        let result = finalize_and_unpack_genesis(archive);
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message == "invalid path found: \"rocksdb/test/\"");
    }

    #[test]
    fn test_archive_unpack_snapshot_invalid_path() {
        let mut header = Header::new_gnu();
        // bypass the sanitization of the .set_path()
        for (p, c) in header
            .as_old_mut()
            .name
            .iter_mut()
            .zip(b"foo/../../../dangerous".iter().chain(Some(&0)))
        {
            *p = *c;
        }
        header.set_size(4);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();
        let result = finalize_and_unpack_snapshot(archive);
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message == "invalid path found: \"foo/../../../dangerous\"");
    }

    fn with_archive_unpack_snapshot_invalid_path(path: &str) -> Result<()> {
        let mut header = Header::new_gnu();
        // bypass the sanitization of the .set_path()
        for (p, c) in header
            .as_old_mut()
            .name
            .iter_mut()
            .zip(path.as_bytes().iter().chain(Some(&0)))
        {
            *p = *c;
        }
        header.set_size(4);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();
        with_finalize_and_unpack(archive, |unpacking_archive, path| {
            for entry in unpacking_archive.entries()? {
                if !entry?.unpack_in(path)? {
                    return Err(UnpackError::Archive("failed!".to_string()));
                } else if !path.join(path).exists() {
                    return Err(UnpackError::Archive("not existing!".to_string()));
                }
            }
            Ok(())
        })
    }

    #[test]
    fn test_archive_unpack_itself() {
        assert_matches!(
            with_archive_unpack_snapshot_invalid_path("ryoqun/work"),
            Ok(())
        );
        // Absolute paths are neutralized as relative
        assert_matches!(
            with_archive_unpack_snapshot_invalid_path("/etc/passwd"),
            Ok(())
        );
        assert_matches!(with_archive_unpack_snapshot_invalid_path("../../../dangerous"), Err(UnpackError::Archive(ref message)) if message == "failed!");
    }

    #[test]
    fn test_archive_unpack_snapshot_invalid_entry() {
        let mut header = Header::new_gnu();
        header.set_path("foo").unwrap();
        header.set_size(4);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();
        let result = finalize_and_unpack_snapshot(archive);
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message == "extra entry found: \"foo\" Regular");
    }

    #[test]
    fn test_archive_unpack_snapshot_too_large() {
        let mut header = Header::new_gnu();
        header.set_path("version").unwrap();
        header.set_size(1024 * 1024 * 1024 * 1024 * 1024);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();
        let result = finalize_and_unpack_snapshot(archive);
        assert_matches!(
            result,
            Err(UnpackError::Archive(ref message))
                if message == &format!(
                    "too large archive: 1125899906842624 than limit: {}", MAX_SNAPSHOT_ARCHIVE_UNPACKED_APPARENT_SIZE
                )
        );
    }

    #[test]
    fn test_archive_unpack_snapshot_bad_unpack() {
        let result = check_unpack_result(false, "abc".to_string());
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message == "failed to unpack: \"abc\"");
    }

    #[test]
    fn test_archive_checked_total_size_sum() {
        let result = checked_total_size_sum(500, 500, MAX_SNAPSHOT_ARCHIVE_UNPACKED_ACTUAL_SIZE);
        assert_matches!(result, Ok(1000));

        let result = checked_total_size_sum(
            u64::max_value() - 2,
            2,
            MAX_SNAPSHOT_ARCHIVE_UNPACKED_ACTUAL_SIZE,
        );
        assert_matches!(
            result,
            Err(UnpackError::Archive(ref message))
                if message == &format!(
                    "too large archive: 18446744073709551615 than limit: {}", MAX_SNAPSHOT_ARCHIVE_UNPACKED_ACTUAL_SIZE
                )
        );
    }

    #[test]
    fn test_archive_checked_total_size_count() {
        let result = checked_total_count_increment(101, MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT);
        assert_matches!(result, Ok(102));

        let result =
            checked_total_count_increment(999_999_999_999, MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT);
        assert_matches!(
            result,
            Err(UnpackError::Archive(ref message))
                if message == "too many files in snapshot: 1000000000000"
        );
    }
}
