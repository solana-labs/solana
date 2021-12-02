use {
    clap::{value_t, ArgMatches},
    std::{
        fs,
        path::{Path, PathBuf},
        process::exit,
    },
};

pub fn parse_ledger_path(matches: &ArgMatches<'_>, name: &str) -> PathBuf {
    PathBuf::from(value_t!(matches, name, String).unwrap_or_else(|_err| {
        eprintln!(
            "Error: Missing --ledger <DIR> argument.\n\n{}",
            matches.usage()
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
