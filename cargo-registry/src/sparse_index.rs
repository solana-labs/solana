use {
    crate::response_builder,
    log::info,
    serde::{Deserialize, Serialize},
};

#[derive(Debug, Default, Deserialize, Serialize)]
struct RegistryConfig {
    dl: String,
    api: Option<String>,
}

#[derive(Clone)]
pub struct RegistryIndex {
    pub(crate) index_root: String,
    config: String,
}

impl RegistryIndex {
    pub fn new(root: &str, server_url: &str) -> Self {
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
        }
    }

    pub fn handler(&self, request: hyper::Request<hyper::Body>) -> hyper::Response<hyper::Body> {
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

        Self::handle_crate_lookup_request(path)
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

    fn handle_crate_lookup_request(path: &str) -> hyper::Response<hyper::Body> {
        let Some(crate_name) = Self::get_crate_name_from_path(path) else {
            return response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for the request",
            );
        };

        // Fetch the index information for the crate
        info!("Received a request to fetch {:?}", crate_name);

        response_builder::success_response()
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
