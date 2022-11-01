use {
    serde::{Deserialize, Deserializer, Serialize, Serializer},
    std::{fmt, str::FromStr},
};

#[derive(Default, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Clone)]
pub struct CliVersion(Option<semver::Version>);

impl CliVersion {
    pub fn unknown_version() -> Self {
        Self(None)
    }
}

impl fmt::Display for CliVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match &self.0 {
            None => "unknown".to_string(),
            Some(version) => version.to_string(),
        };
        write!(f, "{}", s)
    }
}

impl From<semver::Version> for CliVersion {
    fn from(version: semver::Version) -> Self {
        Self(Some(version))
    }
}

impl FromStr for CliVersion {
    type Err = semver::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let version_option = if s == "unknown" {
            None
        } else {
            Some(semver::Version::from_str(s)?)
        };
        Ok(CliVersion(version_option))
    }
}

impl Serialize for CliVersion {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CliVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        CliVersion::from_str(s).map_err(serde::de::Error::custom)
    }
}
