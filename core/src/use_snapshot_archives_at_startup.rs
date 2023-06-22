use std::str::FromStr;

/// When should snapshot archives be used at startup?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseSnapshotArchivesAtStartup {
    /// If snapshot archives are used, they will be extracted and overwrite any existing state
    /// already on disk.  This will incur the associated runtime costs for extracting.
    Always,
    /// If snapshot archive are not used, then the local snapshot state already on disk is
    /// used instead.  If there is no local state on disk, startup will fail.
    Never,
    //Newer <-- will be added later
}

impl UseSnapshotArchivesAtStartup {
    pub const fn variants() -> [&'static str; 2] {
        ["always", "never"]
    }
}

impl std::fmt::Display for UseSnapshotArchivesAtStartup {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use UseSnapshotArchivesAtStartup::*;
        match *self {
            Always => f.write_str("always"),
            Never => f.write_str("never"),
        }
    }
}

impl FromStr for UseSnapshotArchivesAtStartup {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use UseSnapshotArchivesAtStartup::*;
        match s {
            "always" => Ok(Always),
            "never" => Ok(Never),
            _ => Err("valid values: always, never"),
        }
    }
}
