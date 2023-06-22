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
    /// If snapshot archive are not used, then the local snapshot state already on disk is
    /// used instead.  If there is no local state on disk, startup will fail.
    Never,
}

impl UseSnapshotArchivesAtStartup {
    pub const fn variants() -> &'static [&'static str] {
        Self::VARIANTS
    }
}
