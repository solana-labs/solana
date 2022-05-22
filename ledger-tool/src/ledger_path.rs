use {
    clap::ArgMatches,
    std::{
        fs,
        path::{Path, PathBuf},
        process::exit,
    },
};

pub fn parse_ledger_path(matches: &ArgMatches, usage: &str, name: &str) -> PathBuf {
    PathBuf::from(matches.value_of_t::<String>(name).unwrap_or_else(|_err| {
        eprintln!(
            "Error: Missing --ledger <DIR> argument.\n\n{}",
            usage
        );
        exit(1);
    }))
}

// Canonicalize ledger path to avoid issues with symlink creation
pub fn canonicalize_ledger_path(ledger_path: &Path) -> PathBuf {
    fs::canonicalize(&ledger_path).unwrap_or_else(|err| {
        eprintln!(
            "Unable to access ledger path '{}': {}",
            ledger_path.display(),
            err
        );
        exit(1);
    })
}
