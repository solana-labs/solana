use strum::{Display, EnumString, EnumVariantNames, IntoStaticStr, VariantNames};

/// When should snapshot archives be used at startup?
#[derive(
    Debug, Display, Default, Clone, Copy, PartialEq, Eq, EnumVariantNames, EnumString, IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum UseSnapshotArchivesAtStartup {
    /// If snapshot archives are used, they will be extracted and overwrite any existing state
    /// already on disk.  This will incur the associated runtime costs for extracting.
    #[default]
    Always,
    /// If snapshot archive are not used, then the local snapshot state already on disk is
    /// used instead.  If there is no local state on disk, startup will fail.
    Never,
    //Newer <-- will be added later
}

impl UseSnapshotArchivesAtStartup {
    pub const fn variants() -> &'static [&'static str] {
        Self::VARIANTS
    }
}
