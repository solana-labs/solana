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

impl Default for Version {
    fn default() -> Self {
        Self {
            major: env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap(),
            minor: env!("CARGO_PKG_VERSION_MINOR").parse().unwrap(),
            patch: env!("CARGO_PKG_VERSION_PATCH").parse().unwrap(),
            commit: option_env!("CI_COMMIT")
                .map(|sha1| u32::from_str_radix(&sha1[..8], 16).unwrap()),
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
