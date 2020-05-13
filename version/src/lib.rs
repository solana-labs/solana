extern crate serde_derive;
use serde_derive::{Deserialize, Serialize};
use solana_sdk::sanitize::Sanitize;
use std::fmt;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Version {
    major: u16,
    minor: u16,
    patch: u16,
    commit: Option<u32>, // first 4 bytes of the sha1 commit hash
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
        Self {
            major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap(),
            minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap(),
            patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap(),
            commit: compute_commit(option_env!("CI_COMMIT")),
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{} {}",
            self.major,
            self.minor,
            self.patch,
            match self.commit {
                None => "devbuild".to_string(),
                Some(commit) => format!("{:08x}", commit),
            }
        )
    }
}

impl Sanitize for Version {}

#[macro_export]
macro_rules! version {
    () => {
        &*format!("{}", $crate::Version::default())
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
