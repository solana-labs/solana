use bzip2::bufread::BzDecoder;
use log::*;
use regex::Regex;
use solana_sdk::genesis_config::GenesisConfig;
use std::{
    fs::{self, File},
    io::{BufReader, Read},
    path::{
        Component::{CurDir, Normal},
        Path,
    },
    time::Instant,
};
use tar::{
    Archive,
    EntryType::{Directory, GNUSparse, Regular},
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum UnpackError {
    #[error("IO error")]
    IO(#[from] std::io::Error),
    #[error("Archive error")]
    Archive(String),
}

pub type Result<T> = std::result::Result<T, UnpackError>;

const MAX_SNAPSHOT_ARCHIVE_UNPACKED_SIZE: u64 = 500 * 1024 * 1024 * 1024; // 500 GiB
const MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT: u64 = 500_000;
const MAX_GENESIS_ARCHIVE_UNPACKED_SIZE: u64 = 1024 * 1024 * 1024; // 1024 MiB
const MAX_GENESIS_ARCHIVE_UNPACKED_COUNT: u64 = 100;

fn checked_total_size_sum(total_size: u64, entry_size: u64, limit_size: u64) -> Result<u64> {
    let total_size = total_size.saturating_add(entry_size);
    if total_size > limit_size {
        return Err(UnpackError::Archive(format!(
            "too large snapshot: {:?}",
            total_size
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

fn unpack_archive<A: Read, P: AsRef<Path>, C>(
    archive: &mut Archive<A>,
    unpack_dir: P,
    limit_size: u64,
    limit_count: u64,
    entry_checker: C,
) -> Result<()>
where
    C: Fn(&[&str], tar::EntryType) -> bool,
{
    let mut total_size: u64 = 0;
    let mut total_count: u64 = 0;

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
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
        if parts.clone().any(|p| p.is_none()) {
            return Err(UnpackError::Archive(format!(
                "invalid path found: {:?}",
                path_str
            )));
        }

        let parts: Vec<_> = parts.map(|p| p.unwrap()).collect();
        if !entry_checker(parts.as_slice(), entry.header().entry_type()) {
            return Err(UnpackError::Archive(format!(
                "extra entry found: {:?}",
                path_str
            )));
        }
        total_size = checked_total_size_sum(total_size, entry.header().size()?, limit_size)?;
        total_count = checked_total_count_increment(total_count, limit_count)?;

        // unpack_in does its own sanitization
        // ref: https://docs.rs/tar/*/tar/struct.Entry.html#method.unpack_in
        check_unpack_result(entry.unpack_in(&unpack_dir)?, path_str)?
    }

    Ok(())
}

pub fn unpack_snapshot<A: Read, P: AsRef<Path>>(
    archive: &mut Archive<A>,
    unpack_dir: P,
) -> Result<()> {
    unpack_archive(
        archive,
        unpack_dir,
        MAX_SNAPSHOT_ARCHIVE_UNPACKED_SIZE,
        MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT,
        is_valid_snapshot_archive_entry,
    )
}

fn is_valid_snapshot_archive_entry(parts: &[&str], kind: tar::EntryType) -> bool {
    let like_storage = Regex::new(r"^\d+\.\d+$").unwrap();
    let like_slot = Regex::new(r"^\d+$").unwrap();

    trace!("validating: {:?} {:?}", parts, kind);
    match (parts, kind) {
        (["version"], Regular) => true,
        (["accounts"], Directory) => true,
        (["accounts", file], GNUSparse) if like_storage.is_match(file) => true,
        (["accounts", file], Regular) if like_storage.is_match(file) => true,
        (["snapshots"], Directory) => true,
        (["snapshots", "status_cache"], Regular) => true,
        (["snapshots", dir, file], Regular)
            if like_slot.is_match(dir) && like_slot.is_match(file) =>
        {
            true
        }
        (["snapshots", dir], Directory) if like_slot.is_match(dir) => true,
        _ => false,
    }
}

pub fn open_genesis_config(ledger_path: &Path) -> GenesisConfig {
    GenesisConfig::load(&ledger_path).unwrap_or_else(|load_err| {
        let genesis_package = ledger_path.join("genesis.tar.bz2");
        unpack_genesis_archive(&genesis_package, ledger_path).unwrap_or_else(|unpack_err| {
            warn!(
                "Failed to open ledger genesis_config at {:?}: {}, {}",
                ledger_path, load_err, unpack_err,
            );
            std::process::exit(1);
        });

        // loading must succeed at this moment
        GenesisConfig::load(&ledger_path).unwrap()
    })
}

pub fn unpack_genesis_archive(
    archive_filename: &Path,
    destination_dir: &Path,
) -> std::result::Result<(), String> {
    info!("Extracting {:?}...", archive_filename);
    let extract_start = Instant::now();

    fs::create_dir_all(destination_dir).map_err(|err| err.to_string())?;
    let tar_bz2 = File::open(&archive_filename)
        .map_err(|err| format!("Unable to open {:?}: {:?}", archive_filename, err))?;
    let tar = BzDecoder::new(BufReader::new(tar_bz2));
    let mut archive = Archive::new(tar);
    unpack_genesis(&mut archive, destination_dir)
        .map_err(|err| format!("Unable to unpack {:?}: {:?}", archive_filename, err))?;
    info!(
        "Extracted {:?} in {:?}",
        archive_filename,
        Instant::now().duration_since(extract_start)
    );
    Ok(())
}

fn unpack_genesis<A: Read, P: AsRef<Path>>(archive: &mut Archive<A>, unpack_dir: P) -> Result<()> {
    unpack_archive(
        archive,
        unpack_dir,
        MAX_GENESIS_ARCHIVE_UNPACKED_SIZE,
        MAX_GENESIS_ARCHIVE_UNPACKED_COUNT,
        is_valid_genesis_archive_entry,
    )
}

fn is_valid_genesis_archive_entry(parts: &[&str], kind: tar::EntryType) -> bool {
    trace!("validating: {:?} {:?}", parts, kind);
    match (parts, kind) {
        (["genesis.bin"], Regular) => true,
        (["rocksdb"], Directory) => true,
        (["rocksdb", ..], GNUSparse) => true,
        (["rocksdb", ..], Regular) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matches::assert_matches;
    use tar::{Builder, Header};

    #[test]
    fn test_archive_is_valid_entry() {
        assert!(is_valid_snapshot_archive_entry(
            &["accounts", "0.0"],
            tar::EntryType::Regular
        ));
        assert!(is_valid_snapshot_archive_entry(
            &["snapshots"],
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
            &["accounts", "0x0"],
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
    fn test_archive_is_valid_archive_entry() {
        assert!(is_valid_genesis_archive_entry(
            &["genesis.bin"],
            tar::EntryType::Regular
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
            &["rocksdb", "foo", "bar"],
            tar::EntryType::Regular
        ));

        assert!(!is_valid_genesis_archive_entry(
            &["aaaa"],
            tar::EntryType::Regular
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

        checker(&mut archive, &temp_dir.into_path())
    }

    fn finalize_and_unpack_snapshot(archive: tar::Builder<Vec<u8>>) -> Result<()> {
        with_finalize_and_unpack(archive, |a, b| unpack_snapshot(a, b))
    }

    fn finalize_and_unpack_genesis(archive: tar::Builder<Vec<u8>>) -> Result<()> {
        with_finalize_and_unpack(archive, |a, b| unpack_genesis(a, b))
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
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message.to_string() == *"invalid path found: \"foo/../../../dangerous\"");
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
                if entry?.unpack_in(path)? == false {
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
        assert_matches!(with_archive_unpack_snapshot_invalid_path("../../../dangerous"), Err(UnpackError::Archive(ref message)) if message.to_string() == "failed!");
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
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message.to_string() == *"extra entry found: \"foo\"");
    }

    #[test]
    fn test_archive_unpack_snapshot_too_large() {
        let mut header = Header::new_gnu();
        header.set_path("version").unwrap();
        header.set_size(1 * 1024 * 1024 * 1024 * 1024 * 1024);
        header.set_cksum();

        let data: &[u8] = &[1, 2, 3, 4];

        let mut archive = Builder::new(Vec::new());
        archive.append(&header, data).unwrap();
        let result = finalize_and_unpack_snapshot(archive);
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message.to_string() == *"too large snapshot: 1125899906842624");
    }

    #[test]
    fn test_archive_unpack_snapshot_bad_unpack() {
        let result = check_unpack_result(false, "abc".to_string());
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message.to_string() == *"failed to unpack: \"abc\"");
    }

    #[test]
    fn test_archive_checked_total_size_sum() {
        let result = checked_total_size_sum(500, 500, MAX_SNAPSHOT_ARCHIVE_UNPACKED_SIZE);
        assert_matches!(result, Ok(1000));

        let result =
            checked_total_size_sum(u64::max_value() - 2, 2, MAX_SNAPSHOT_ARCHIVE_UNPACKED_SIZE);
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message.to_string() == *"too large snapshot: 18446744073709551615");
    }

    #[test]
    fn test_archive_checked_total_size_count() {
        let result = checked_total_count_increment(101, MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT);
        assert_matches!(result, Ok(102));

        let result =
            checked_total_count_increment(999_999_999_999, MAX_SNAPSHOT_ARCHIVE_UNPACKED_COUNT);
        assert_matches!(result, Err(UnpackError::Archive(ref message)) if message.to_string() == *"too many files in snapshot: 1000000000000");
    }
}
