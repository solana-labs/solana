#![cfg_attr(RUSTC_WITH_SPECIALIZATION, feature(min_specialization))]

extern crate serde_derive;
use {
    serde_derive::{Deserialize, Serialize},
    solana_sdk::sanitize::Sanitize,
    std::{convert::TryInto, fmt},
};
#[macro_use]
extern crate solana_frozen_abi_macro;

// Older version structure used earlier 1.3.x releases
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, AbiExample)]
pub struct LegacyVersion {
    major: u16,
    minor: u16,
    patch: u16,
    commit: Option<u32>, // first 4 bytes of the sha1 commit hash
}

impl Sanitize for LegacyVersion {}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, AbiExample)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub commit: Option<u32>, // first 4 bytes of the sha1 commit hash
    pub feature_set: u32,    // first 4 bytes of the FeatureSet identifier
}

impl Version {
    pub fn as_semver_version(&self) -> semver::Version {
        semver::Version::new(self.major as u64, self.minor as u64, self.patch as u64)
    }
}

impl From<LegacyVersion> for Version {
    fn from(legacy_version: LegacyVersion) -> Self {
        Self {
            major: legacy_version.major,
            minor: legacy_version.minor,
            patch: legacy_version.patch,
            commit: legacy_version.commit,
            feature_set: 0,
        }
    }
}

fn compute_commit(sha1: Option<&'static str>) -> Option<u32> {
    let sha1 = sha1?;
    if sha1.len() < 8 {
        None
    } else {
        u32::from_str_radix(&sha1[..8], 16).ok()
    }
}

impl Default for Version {
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

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch,)
    }
}

impl fmt::Debug for Version {
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

impl Sanitize for Version {}

#[macro_export]
macro_rules! semver {
    () => {
        &*format!("{}", $crate::Version::default())
    };
}

#[macro_export]
macro_rules! version {
    () => {
        &*format!("{:?}", $crate::Version::default())
    };
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_compute_commit() {
        assert_eq!(compute_commit(None), None);
        assert_eq!(compute_commit(Some("1234567890")), Some(0x1234_5678));
        assert_eq!(compute_commit(Some("HEAD")), None);
        assert_eq!(compute_commit(Some("garbagein")), None);
    }
}
