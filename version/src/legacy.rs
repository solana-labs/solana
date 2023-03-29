use {
    crate::compute_commit,
    serde_derive::{Deserialize, Serialize},
    solana_sdk::sanitize::Sanitize,
    std::{convert::TryInto, fmt},
};

// Older version structure used earlier 1.3.x releases
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct LegacyVersion1 {
    major: u16,
    minor: u16,
    patch: u16,
    commit: Option<u32>, // first 4 bytes of the sha1 commit hash
}

impl Sanitize for LegacyVersion1 {}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, AbiExample)]
pub struct LegacyVersion2 {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub commit: Option<u32>, // first 4 bytes of the sha1 commit hash
    pub feature_set: u32,    // first 4 bytes of the FeatureSet identifier
}

impl From<LegacyVersion1> for LegacyVersion2 {
    fn from(legacy_version: LegacyVersion1) -> Self {
        Self {
            major: legacy_version.major,
            minor: legacy_version.minor,
            patch: legacy_version.patch,
            commit: legacy_version.commit,
            feature_set: 0,
        }
    }
}

impl Default for LegacyVersion2 {
    fn default() -> Self {
        let feature_set = u32::from_le_bytes(
            solana_sdk::feature_set::ID.as_ref()[..4]
                .try_into()
                .unwrap(),
        );
        Self {
            major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap(),
            minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap(),
            patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap(),
            commit: compute_commit(option_env!("CI_COMMIT")),
            feature_set,
        }
    }
}

impl fmt::Display for LegacyVersion2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch,)
    }
}

impl fmt::Debug for LegacyVersion2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{} (src:{}; feat:{})",
            self.major,
            self.minor,
            self.patch,
            match self.commit {
                None => "devbuild".to_string(),
                Some(commit) => format!("{commit:08x}"),
            },
            self.feature_set,
        )
    }
}

impl Sanitize for LegacyVersion2 {}
