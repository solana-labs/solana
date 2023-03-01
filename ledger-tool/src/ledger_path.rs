use std::{
    fs,
    path::{Path, PathBuf},
    process::exit,
};

// Canonicalize ledger path to avoid issues with symlink creation
pub fn canonicalize_ledger_path(ledger_path: &Path) -> PathBuf {
    fs::canonicalize(ledger_path).unwrap_or_else(|err| {
        eprintln!(
            "Unable to access ledger path '{}': {}",
            ledger_path.display(),
            err
        );
        exit(1);
    })
}
