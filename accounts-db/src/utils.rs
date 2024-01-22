use {
    log::*,
    std::{
        fs,
        path::{Path, PathBuf},
    },
};

pub const ACCOUNTS_RUN_DIR: &str = "run";
pub const ACCOUNTS_SNAPSHOT_DIR: &str = "snapshot";

/// For all account_paths, create the run/ and snapshot/ sub directories.
/// If an account_path directory does not exist, create it.
/// It returns (account_run_paths, account_snapshot_paths) or error
pub fn create_all_accounts_run_and_snapshot_dirs(
    account_paths: &[PathBuf],
) -> std::io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut run_dirs = Vec::with_capacity(account_paths.len());
    let mut snapshot_dirs = Vec::with_capacity(account_paths.len());
    for account_path in account_paths {
        // create the run/ and snapshot/ sub directories for each account_path
        let (run_dir, snapshot_dir) = create_accounts_run_and_snapshot_dirs(account_path)?;
        run_dirs.push(run_dir);
        snapshot_dirs.push(snapshot_dir);
    }
    Ok((run_dirs, snapshot_dirs))
}

/// To allow generating a bank snapshot directory with full state information, we need to
/// hardlink account appendvec files from the runtime operation directory to a snapshot
/// hardlink directory.  This is to create the run/ and snapshot sub directories for an
/// account_path provided by the user.  These two sub directories are on the same file
/// system partition to allow hard-linking.
pub fn create_accounts_run_and_snapshot_dirs(
    account_dir: impl AsRef<Path>,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let run_path = account_dir.as_ref().join(ACCOUNTS_RUN_DIR);
    let snapshot_path = account_dir.as_ref().join(ACCOUNTS_SNAPSHOT_DIR);
    if (!run_path.is_dir()) || (!snapshot_path.is_dir()) {
        // If the "run/" or "snapshot" sub directories do not exist, the directory may be from
        // an older version for which the appendvec files are at this directory.  Clean up
        // them first.
        // This will be done only once when transitioning from an old image without run directory
        // to this new version using run and snapshot directories.
        // The run/ content cleanup will be done at a later point.  The snapshot/ content persists
        // across the process boot, and will be purged by the account_background_service.
        if fs::remove_dir_all(&account_dir).is_err() {
            delete_contents_of_path(&account_dir);
        }
        fs::create_dir_all(&run_path)?;
        fs::create_dir_all(&snapshot_path)?;
    }

    Ok((run_path, snapshot_path))
}

/// Delete the files and subdirectories in a directory.
/// This is useful if the process does not have permission
/// to delete the top level directory it might be able to
/// delete the contents of that directory.
pub fn delete_contents_of_path(path: impl AsRef<Path>) {
    match fs::read_dir(&path) {
        Err(err) => {
            warn!(
                "Failed to delete contents of '{}': could not read dir: {err}",
                path.as_ref().display(),
            )
        }
        Ok(dir_entries) => {
            for entry in dir_entries.flatten() {
                let sub_path = entry.path();
                let result = if sub_path.is_dir() {
                    fs::remove_dir_all(&sub_path)
                } else {
                    fs::remove_file(&sub_path)
                };
                if let Err(err) = result {
                    warn!(
                        "Failed to delete contents of '{}': {err}",
                        sub_path.display(),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use {super::*, tempfile::TempDir};

    #[test]
    pub fn test_create_all_accounts_run_and_snapshot_dirs() {
        let (_tmp_dirs, account_paths): (Vec<TempDir>, Vec<PathBuf>) = (0..4)
            .map(|_| {
                let tmp_dir = tempfile::TempDir::new().unwrap();
                let account_path = tmp_dir.path().join("accounts");
                (tmp_dir, account_path)
            })
            .unzip();

        // create the `run/` and `snapshot/` dirs, and ensure they're there
        let (account_run_paths, account_snapshot_paths) =
            create_all_accounts_run_and_snapshot_dirs(&account_paths).unwrap();
        account_run_paths.iter().all(|path| path.is_dir());
        account_snapshot_paths.iter().all(|path| path.is_dir());

        // delete a `run/` and `snapshot/` dir, then re-create it
        let account_path_first = account_paths.first().unwrap();
        delete_contents_of_path(account_path_first);
        assert!(account_path_first.exists());
        assert!(!account_path_first.join(ACCOUNTS_RUN_DIR).exists());
        assert!(!account_path_first.join(ACCOUNTS_SNAPSHOT_DIR).exists());

        _ = create_all_accounts_run_and_snapshot_dirs(&account_paths).unwrap();
        account_run_paths.iter().all(|path| path.is_dir());
        account_snapshot_paths.iter().all(|path| path.is_dir());
    }
}
