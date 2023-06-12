use std::path::{Path, PathBuf};

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
pub struct AccountDirectory {
    /// The path to the `run` directory.
    run: PathBuf,
    /// The path to the `snapshot` directory.
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
        let run = account_path.join("run");
        let snapshot = account_path.join("snapshot");
        std::fs::create_dir_all(&run)?;
        std::fs::create_dir_all(&snapshot)?;

        Ok(Self { run, snapshot })
    }

    /// Return the path to the directory containing the current account state.
    pub fn current_state_dir(&self) -> &Path {
        &self.run
    }

    /// Return the path to the directory containing the snapshots.
    pub fn snapshot_dir(&self) -> &Path {
        &self.snapshot
    }
}
