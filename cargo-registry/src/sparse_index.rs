use {
    crate::{
        client::Client,
        crate_handler::{Dependency, Error, PackageMetaData, Program, UnpackedCrate},
        response_builder,
    },
    log::info,
    serde::{Deserialize, Serialize},
    std::{
        collections::BTreeMap,
        sync::{Arc, RwLock},
    },
};

#[derive(Debug, Default, Deserialize, Serialize)]
struct RegistryConfig {
    dl: String,
    api: Option<String>,
}

pub(crate) struct RegistryIndex {
    pub(crate) index_root: String,
    config: String,
    index: RwLock<BTreeMap<String, IndexEntry>>,
}

#[derive(Serialize)]
pub(crate) struct IndexEntryDep {
    pub name: String,
    pub req: String,
    pub features: Vec<String>,
    pub optional: bool,
    pub default_features: bool,
    pub target: Option<String>,
    pub kind: String,
    pub registry: Option<String>,
    pub package: Option<String>,
}

impl From<Dependency> for IndexEntryDep {
    fn from(v: Dependency) -> Self {
        IndexEntryDep {
            name: v.name,
            req: v.version_req,
            features: v.features,
            optional: v.optional,
            default_features: v.default_features,
            target: v.target,
            kind: serde_json::to_string(&v.kind).expect("Failed to stringify dep kind"),
            registry: v.registry,
            package: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct IndexEntry {
    pub name: String,
    pub vers: String,
    pub deps: Vec<IndexEntryDep>,
    pub cksum: String,
    pub features: BTreeMap<String, Vec<String>>,
    pub yanked: bool,
    pub links: Option<String>,
}

impl From<PackageMetaData> for IndexEntry {
    fn from(v: PackageMetaData) -> Self {
        IndexEntry {
            name: v.name,
            vers: v.vers,
            deps: v.deps.into_iter().map(|v| v.into()).collect(),
            cksum: String::new(),
            features: v.features,
            yanked: false,
            links: v.links,
        }
    }
}

impl RegistryIndex {
    pub(crate) fn new(root: &str, server_url: &str) -> Self {
        let registry_config = RegistryConfig {
            dl: format!("{}/api/v1/crates", server_url),
            api: Some(server_url.to_string()),
        };
        let config =
            serde_json::to_string(&registry_config).expect("Failed to create registry config");

        info!("Registry index is available at {}{}/", server_url, root);
        Self {
            index_root: root.to_string(),
            config,
            index: RwLock::new(BTreeMap::new()),
        }
    }

    pub(crate) fn handler(
        &self,
        request: hyper::Request<hyper::Body>,
        client: Arc<Client>,
    ) -> hyper::Response<hyper::Body> {
        let path = request.uri().path();
        let expected_root = self.index_root.as_str();
        if !path.starts_with(expected_root) {
            return response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for index",
            );
        }

        let Some((_, path)) = path.split_once(expected_root) else {
            return response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for index",
            );
        };

        if path == "/config.json" {
            return response_builder::success_response_str(&self.config);
        }

        self.handle_crate_lookup_request(path, client)
    }

    pub(crate) fn insert_entry(&self, entry: IndexEntry) -> Result<(), Error> {
        let mut write_index = self
            .index
            .write()
            .map_err(|e| format!("Failed to lock the index for writing: {}", e))?;
        info!("Inserting {}-{} in registry index", entry.name, entry.vers);
        write_index.insert(entry.name.clone(), entry);
        Ok(())
    }

    fn get_crate_name_from_path(path: &str) -> Option<&str> {
        let (path, crate_name) = path.rsplit_once('/')?;

        // The index for deployed crates follow the path naming described here
        // https://doc.rust-lang.org/cargo/reference/registry-index.html#index-files
        match crate_name.len() {
            0 => false,
            1 => path == "/1",
            2 => path == "/2",
            3 => {
                let first_char = crate_name.chars().next()?;
                path == format!("/3/{}", first_char)
            }
            _ => {
                let (first_two_char, rest) = crate_name.split_at(2);
                let (next_two_char, _) = rest.split_at(2);
                path == format!("/{}/{}", first_two_char, next_two_char)
            }
        }
        .then_some(crate_name)
    }

    fn handle_crate_lookup_request(
        &self,
        path: &str,
        client: Arc<Client>,
    ) -> hyper::Response<hyper::Body> {
        let Some(crate_name) = Self::get_crate_name_from_path(path) else {
            return response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for the request",
            );
        };

        info!("Looking up index for {:?}", crate_name);

        let Ok(read_index) = self.index.read() else {
            return response_builder::error_response(
                hyper::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal error. Failed to lock the index for reading",
            );
        };

        let response = if let Some(entry) = read_index.get(crate_name) {
            Some(serde_json::to_string(entry))
        } else {
            // The index currently doesn't contain the program entry.
            // Fetch the program information from the network using RPC client.
            Program::crate_name_to_program_id(crate_name)
                .and_then(|id| UnpackedCrate::fetch_index(id, client).ok())
                .map(|entry| serde_json::to_string(&entry))
        };

        let Some(Ok(response)) = response else {
            return response_builder::error_response(
                hyper::StatusCode::INTERNAL_SERVER_ERROR,
                "Internal error. index entry is corrupted",
            );
        };

        response_builder::success_response_str(response.as_str())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_crate_name_from_path() {
        assert_eq!(RegistryIndex::get_crate_name_from_path(""), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/"), None);

        // Single character crate name
        assert_eq!(RegistryIndex::get_crate_name_from_path("/a"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/1/a"), Some("a"));
        assert_eq!(RegistryIndex::get_crate_name_from_path("/2/a"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/a/a"), None);

        // Two character crate name
        assert_eq!(RegistryIndex::get_crate_name_from_path("/ab"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/1/ab"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/2/ab"), Some("ab"));
        assert_eq!(RegistryIndex::get_crate_name_from_path("/3/ab"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/ab/ab"), None);

        // Three character crate name
        assert_eq!(RegistryIndex::get_crate_name_from_path("/abc"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/1/abc"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/2/abc"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/3/abc"), None);
        assert_eq!(
            RegistryIndex::get_crate_name_from_path("/3/a/abc"),
            Some("abc")
        );
        assert_eq!(RegistryIndex::get_crate_name_from_path("/ab/abc"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/ab/c/abc"), None);

        // Four character crate name
        assert_eq!(RegistryIndex::get_crate_name_from_path("/abcd"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/1/abcd"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/2/abcd"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/3/abcd"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/3/a/abcd"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/4/abcd"), None);
        assert_eq!(
            RegistryIndex::get_crate_name_from_path("/ab/cd/abcd"),
            Some("abcd")
        );
        assert_eq!(RegistryIndex::get_crate_name_from_path("/ab/cd/abc"), None);

        // More character crate name
        assert_eq!(RegistryIndex::get_crate_name_from_path("/abcdefgh"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/1/abcdefgh"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/2/abcdefgh"), None);
        assert_eq!(RegistryIndex::get_crate_name_from_path("/3/abcdefgh"), None);
        assert_eq!(
            RegistryIndex::get_crate_name_from_path("/3/a/abcdefgh"),
            None
        );
        assert_eq!(RegistryIndex::get_crate_name_from_path("/4/abcdefgh"), None);
        assert_eq!(
            RegistryIndex::get_crate_name_from_path("/ab/cd/abcdefgh"),
            Some("abcdefgh")
        );
    }
}
