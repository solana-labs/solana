use strum::{Display, EnumString, EnumVariantNames, IntoStaticStr, VariantNames};

/// When should snapshot archives be used at startup?
#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, Display, EnumString, EnumVariantNames, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum UseSnapshotArchivesAtStartup {
    /// If snapshot archives are used, they will be extracted and overwrite any existing state
    /// already on disk.  This will incur the associated runtime costs for extracting.
    #[default]
    Always,
    /// If snapshot archives are not used, then the local snapshot state already on disk is
    /// used instead.  If there is no local state on disk, startup will fail.
    Never,
    /// Only use snapshot archives if they are newer than the local snapshot state on disk.
    /// This can happen if a node is stopped and a new snapshot archive is downloaded before
    /// restarting.  At startup, the snapshot archive would be the newest and loaded from.
    /// Note, this also implies that snapshot archives will be used if there is no local snapshot
    /// state on disk.
    WhenNewest,
}

pub mod cli {
    use super::*;

    pub const NAME: &str = "use_snapshot_archives_at_startup";
    pub const LONG_ARG: &str = "use-snapshot-archives-at-startup";
    pub const HELP: &str = "When should snapshot archives be used at startup?";
    pub const LONG_HELP: &str = "At startup, when should snapshot archives be extracted \
        versus using what is already on disk? \
        \nSpecifying \"always\" will always startup by extracting snapshot archives \
        and disregard any snapshot-related state already on disk. \
        Note that starting up from snapshot archives will incur the runtime costs \
        associated with extracting the archives and rebuilding the local state. \
        \nSpecifying \"never\" will never startup from snapshot archives \
        and will only use snapshot-related state already on disk. \
        If there is no state already on disk, startup will fail. \
        Note, this will use the latest state available, \
        which may be newer than the latest snapshot archive. \
        \nSpecifying \"when-newest\" will use snapshot-related state \
        already on disk unless there are snapshot archives newer than it. \
        This can happen if a new snapshot archive is downloaded \
        while the node is stopped.";

    pub const POSSIBLE_VALUES: &[&str] = UseSnapshotArchivesAtStartup::VARIANTS;

    pub fn default_value() -> &'static str {
        UseSnapshotArchivesAtStartup::default().into()
    }
}
