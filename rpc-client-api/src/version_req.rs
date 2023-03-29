pub(crate) struct VersionReq(Vec<semver::VersionReq>);

impl VersionReq {
    pub(crate) fn from_strs<T>(versions: &[T]) -> Result<Self, String>
    where
        T: AsRef<str> + std::fmt::Debug,
    {
        let mut version_reqs = vec![];
        for version in versions {
            let version_req = semver::VersionReq::parse(version.as_ref())
                .map_err(|err| format!("Could not parse version {version:?}: {err:?}"))?;
            version_reqs.push(version_req);
        }
        Ok(Self(version_reqs))
    }

    pub(crate) fn matches_any(&self, version: &semver::Version) -> bool {
        self.0.iter().any(|r| r.matches(version))
    }
}
