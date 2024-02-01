use {
    lazy_static,
    log::*,
    solana_measure::measure,
    std::{
        collections::HashSet,
        fs,
        path::{Path, PathBuf},
        sync::Mutex,
        thread,
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

/// Moves and asynchronously deletes the contents of a directory to avoid blocking on it.
/// The directory is re-created after the move, and should now be empty.
pub fn move_and_async_delete_path_contents(path: impl AsRef<Path>) {
    move_and_async_delete_path(&path);
    // The following could fail if the rename failed.
    // If that happens, the directory should be left as is.
    // So we ignore errors here.
    _ = std::fs::create_dir(path);
}

/// Delete directories/files asynchronously to avoid blocking on it.
/// First, in sync context, check if the original path exists, if it
/// does, rename the original path to *_to_be_deleted.
/// If there's an in-progress deleting thread for this path, return.
/// Then spawn a thread to delete the renamed path.
pub fn move_and_async_delete_path(path: impl AsRef<Path>) {
    lazy_static! {
        static ref IN_PROGRESS_DELETES: Mutex<HashSet<PathBuf>> = Mutex::new(HashSet::new());
    };

    // Grab the mutex so no new async delete threads can be spawned for this path.
    let mut lock = IN_PROGRESS_DELETES.lock().unwrap();

    // If the path does not exist, there's nothing to delete.
    if !path.as_ref().exists() {
        return;
    }

    // If the original path (`pathbuf` here) is already being deleted,
    // then the path should not be moved and deleted again.
    if lock.contains(path.as_ref()) {
        return;
    }

    let mut path_delete = path.as_ref().to_path_buf();
    path_delete.set_file_name(format!(
        "{}{}",
        path_delete.file_name().unwrap().to_str().unwrap(),
        "_to_be_deleted"
    ));
    if let Err(err) = fs::rename(&path, &path_delete) {
        warn!(
            "Cannot async delete, retrying in sync mode: failed to rename '{}' to '{}': {err}",
            path.as_ref().display(),
            path_delete.display(),
        );
        // Although the delete here is synchronous, we want to prevent another thread
        // from moving & deleting this directory via `move_and_async_delete_path`.
        lock.insert(path.as_ref().to_path_buf());
        drop(lock); // unlock before doing sync delete

        delete_contents_of_path(&path);
        IN_PROGRESS_DELETES.lock().unwrap().remove(path.as_ref());
        return;
    }

    lock.insert(path_delete.clone());
    drop(lock);
    thread::Builder::new()
        .name("solDeletePath".to_string())
        .spawn(move || {
            trace!("background deleting {}...", path_delete.display());
            let (result, measure_delete) = measure!(fs::remove_dir_all(&path_delete));
            if let Err(err) = result {
                panic!("Failed to async delete '{}': {err}", path_delete.display());
            }
            trace!(
                "background deleting {}... Done, and{measure_delete}",
                path_delete.display()
            );

            IN_PROGRESS_DELETES.lock().unwrap().remove(&path_delete);
        })
        .expect("spawn background delete thread");
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

/// Creates directories if they do not exist, and canonicalizes the paths.
pub fn create_and_canonicalize_directories(
    directories: impl IntoIterator<Item = impl AsRef<Path>>,
) -> std::io::Result<Vec<PathBuf>> {
    directories
        .into_iter()
        .map(|path| {
            fs::create_dir_all(&path)?;
            let path = fs::canonicalize(&path)?;
            Ok(path)
        })
        .collect()
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
