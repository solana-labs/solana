use {
    crate::snapshot_utils::move_and_async_delete_path_contents,
    std::path::{Path, PathBuf},
};

/// Many functions in snapshot-utils expect the `account_paths` directories
/// to have a specific structure:
/// ```text
///      <account-path>
///     ├── run
///     └── snapshot
/// ```
///
/// The `run` directory is used to store the current account state, while the `snapshot`
/// directory contains directories for snapshots containing hardlinks to storage files.
///
/// This type is meant to provide a type-safe and convenient way to access and store
/// these account directories.
#[derive(Debug, Clone)]
pub struct AccountDirectory {
    run: PathBuf,
    snapshot: PathBuf,
}

impl AccountDirectory {
    /// Given the base `account_path` directory, create a new `AccountDirectory`.
    /// If the directory does not exist, it will be created.
    /// If the path exists, and is a link, it will be resolved.
    /// If the path exists, and is not a directory, an error will be returned.
    pub fn new(account_path: impl AsRef<Path>) -> std::io::Result<Self> {
        std::fs::create_dir_all(&account_path)?;
        let account_path = account_path.as_ref().canonicalize()?;
        let (run, snapshot) = Self::subdirectories(account_path);
        std::fs::create_dir_all(&run)?;
        std::fs::create_dir_all(&snapshot)?;

        Ok(Self { run, snapshot })
    }

    /// Given the base `account_path` directory, create a new `AccountDirectory`
    /// with the assumption that the account directory structure is already in place.
    /// If the structure is not in place, an error will be returned.
    pub fn existing(account_path: impl AsRef<Path>) -> std::io::Result<Self> {
        let account_path = account_path.as_ref().canonicalize()?;

        Self::check_directory_exists(&account_path)?;
        let (run, snapshot) = Self::subdirectories(&account_path);
        Self::check_directory_exists(&run)?;
        Self::check_directory_exists(&snapshot)?;

        Ok(Self { run, snapshot })
    }

    /// Return the root of the account directory.
    pub fn root_dir(&self) -> &Path {
        self.run.parent().unwrap()
    }

    /// Return the path to the directory containing the current account state.
    pub fn run_dir(&self) -> &Path {
        &self.run
    }

    /// Return the path to the directory containing the snapshots.
    pub fn snapshot_dir(&self) -> &Path {
        &self.snapshot
    }

    /// Clean entire directory.
    pub fn clean(&self) {
        move_and_async_delete_path_contents(self.run_dir());
        move_and_async_delete_path_contents(self.snapshot_dir());
    }

    /// Check that a path exists and is a directory.
    fn check_directory_exists(path: impl AsRef<Path>) -> std::io::Result<()> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Path does not exist: {}", path.display()),
            ));
        }

        if !path.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Path is not a directory: {}", path.display()),
            ));
        }

        Ok(())
    }

    /// Subdirectories of the account directory.
    /// Returns tuple of (run, snapshot) directories.
    fn subdirectories(account_path: impl AsRef<Path>) -> (PathBuf, PathBuf) {
        let account_path = account_path.as_ref();
        let run = account_path.join("run");
        let snapshot = account_path.join("snapshot");
        (run, snapshot)
    }
}
