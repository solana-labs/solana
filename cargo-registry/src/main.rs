//! The `registry_service` module implements the Solana cargo registry service.
use {
    crate::{
        client::Client,
        publisher::{Error, Publisher},
    },
    hyper::{
        body,
        service::{make_service_fn, service_fn},
        Method, Server,
    },
    log::*,
    serde::{Deserialize, Serialize},
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
    },
};

mod client;
mod publisher;

const PATH_PREFIX: &str = "/api/v1/crates";

#[derive(Debug, Default, Deserialize, Serialize)]
struct RegistryConfig {
    dl: String,
    api: Option<String>,
}

pub struct CargoRegistryService {}

impl CargoRegistryService {
    fn error_response(status: hyper::StatusCode, msg: &str) -> hyper::Response<hyper::Body> {
        error!("{}", msg);
        hyper::Response::builder()
            .status(status)
            .body(hyper::Body::from(
                serde_json::json!({
                    "errors" : [
                        {"details": msg}
                    ]
                })
                .to_string(),
            ))
            .unwrap()
    }

    fn success_response_str(value: &str) -> hyper::Response<hyper::Body> {
        hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(value.to_string()))
            .unwrap()
    }

    fn success_response() -> hyper::Response<hyper::Body> {
        Self::success_response_str("")
    }

    async fn handle_publish_request(
        request: hyper::Request<hyper::Body>,
        client: Arc<Client>,
    ) -> hyper::Response<hyper::Body> {
        info!("Handling request to publish the crate");
        let bytes = body::to_bytes(request.into_body()).await;

        match bytes {
            Ok(data) => {
                let Ok(result) =
                    tokio::task::spawn_blocking(move || Publisher::publish_crate(data, client))
                        .await
                else {
                    return Self::error_response(
                        hyper::StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal error. Failed to wait for program deployment",
                    );
                };

                if result.is_ok() {
                    info!("Published the crate successfully. {:?}", result);
                    Self::success_response()
                } else {
                    Self::error_response(
                        hyper::StatusCode::BAD_REQUEST,
                        format!("Failed to publish the crate. {:?}", result).as_str(),
                    )
                }
            }
            Err(_) => Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to receive the crate data from the client.",
            ),
        }
    }

    fn get_crate_name_and_version(path: &str) -> Option<(&str, &str, &str)> {
        path.rsplit_once('/').and_then(|(remainder, version)| {
            remainder
                .rsplit_once('/')
                .map(|(remainder, name)| (remainder, name, version))
        })
    }

    fn handle_yank_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name, _version)) = Self::get_crate_name_and_version(path) else {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to parse the request.",
            );
        };

        if path.len() != PATH_PREFIX.len() {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Request length is incorrect",
            );
        }

        Self::error_response(
            hyper::StatusCode::NOT_IMPLEMENTED,
            "This command is not implemented yet",
        )
    }

    fn handle_unyank_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name, _version)) = Self::get_crate_name_and_version(path) else {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to parse the request.",
            );
        };

        if path.len() != PATH_PREFIX.len() {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Request length is incorrect",
            );
        }

        Self::error_response(
            hyper::StatusCode::NOT_IMPLEMENTED,
            "This command is not implemented yet",
        )
    }

    fn get_crate_name(path: &str) -> Option<(&str, &str)> {
        path.rsplit_once('/')
    }

    fn handle_get_owners_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name)) = Self::get_crate_name(path) else {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to parse the request.",
            );
        };

        if path.len() != PATH_PREFIX.len() {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Request length is incorrect",
            );
        }

        Self::error_response(
            hyper::StatusCode::NOT_IMPLEMENTED,
            "This command is not implemented yet",
        )
    }

    fn handle_add_owners_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name)) = Self::get_crate_name(path) else {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to parse the request.",
            );
        };

        if path.len() != PATH_PREFIX.len() {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Request length is incorrect",
            );
        }

        Self::error_response(
            hyper::StatusCode::NOT_IMPLEMENTED,
            "This command is not implemented yet",
        )
    }

    fn handle_delete_owners_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name)) = Self::get_crate_name(path) else {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to parse the request.",
            );
        };

        if path.len() != PATH_PREFIX.len() {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Request length is incorrect",
            );
        }

        Self::error_response(
            hyper::StatusCode::NOT_IMPLEMENTED,
            "This command is not implemented yet",
        )
    }

    fn handle_get_crates_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        // The endpoint for this type of request is `/api/v1/crates` (same as PATH_PREFIX).
        // The `crates` substring has already been extracted out of the endpoint string.
        // So the path should only contain `/api/v1". The caller already checked that the
        // full path started with PATH_PREFIX. So it's sufficient to check that provided
        // path is smaller than PATH_PREFIX.
        if path.len() >= PATH_PREFIX.len() {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Request length is incorrect",
            );
        }

        Self::error_response(
            hyper::StatusCode::NOT_IMPLEMENTED,
            "This command is not implemented yet",
        )
    }

    fn get_crate_name_from_path(path: &str) -> Option<&str> {
        let (path, crate_name) = path.rsplit_once('/')?;

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

    fn handle_crate_dl_request(
        request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some(crate_name) = Self::get_crate_name_from_path(request.uri().path()) else {
            return Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for the request",
            );
        };

        // Fetch the program and return a crate buffer.
        info!("Received a request to fetch {:?}", crate_name);

        Self::success_response()
    }

    async fn handler(
        config: String,
        request: hyper::Request<hyper::Body>,
        client: Arc<Client>,
    ) -> Result<hyper::Response<hyper::Body>, Error> {
        let path = request.uri().path();
        if path.starts_with("/git") {
            return Ok(Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "This registry server does not support GIT index. Please use sparse index.",
            ));
        }

        if path == "/config.json" {
            return Ok(Self::success_response_str(&config));
        }

        if !path.starts_with(PATH_PREFIX) {
            return Ok(Self::handle_crate_dl_request(&request));
        }

        let Some((path, endpoint)) = path.rsplit_once('/') else {
            return Ok(Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid endpoint in the path",
            ));
        };

        Ok(match *request.method() {
            Method::PUT => match endpoint {
                "new" => {
                    if path.len() != PATH_PREFIX.len() {
                        Self::error_response(
                            hyper::StatusCode::BAD_REQUEST,
                            "Invalid length of the request.",
                        )
                    } else {
                        Self::handle_publish_request(request, client.clone()).await
                    }
                }
                "unyank" => Self::handle_unyank_request(path, &request),
                "owners" => Self::handle_add_owners_request(path, &request),
                _ => Self::error_response(hyper::StatusCode::METHOD_NOT_ALLOWED, "Unknown request"),
            },
            Method::GET => match endpoint {
                "crates" => Self::handle_get_crates_request(path, &request),
                "owners" => Self::handle_get_owners_request(path, &request),
                _ => Self::error_response(hyper::StatusCode::METHOD_NOT_ALLOWED, "Unknown request"),
            },
            Method::DELETE => match endpoint {
                "yank" => Self::handle_yank_request(path, &request),
                "owners" => Self::handle_delete_owners_request(path, &request),
                _ => Self::error_response(hyper::StatusCode::METHOD_NOT_ALLOWED, "Unknown request"),
            },
            _ => Self::error_response(hyper::StatusCode::METHOD_NOT_ALLOWED, "Unknown request"),
        })
    }
}

#[tokio::main]
async fn main() {
    solana_logger::setup_with_default("solana=info");
    let client = Arc::new(Client::new().expect("Failed to get RPC Client instance"));

    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), client.port);

    let registry_config = RegistryConfig {
        dl: format!("{}/api/v1/crates", client.server_url),
        api: Some(client.server_url.to_string()),
    };
    let registry_config_json =
        serde_json::to_string(&registry_config).expect("Failed to create registry config");

    let registry_service = make_service_fn(move |_| {
        let client_inner = client.clone();
        let config = registry_config_json.clone();
        async move {
            Ok::<_, Error>(service_fn(move |request| {
                CargoRegistryService::handler(config.clone(), request, client_inner.clone())
            }))
        }
    });

    let server = Server::bind(&bind_addr).serve(registry_service);
    info!("Server running on on http://{}", bind_addr);

    let _ = server.await;
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_crate_name_from_path() {
        assert_eq!(CargoRegistryService::get_crate_name_from_path(""), None);
        assert_eq!(CargoRegistryService::get_crate_name_from_path("/"), None);

        // Single character crate name
        assert_eq!(CargoRegistryService::get_crate_name_from_path("/a"), None);
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/1/a"),
            Some("a")
        );
        assert_eq!(CargoRegistryService::get_crate_name_from_path("/2/a"), None);
        assert_eq!(CargoRegistryService::get_crate_name_from_path("/a/a"), None);

        // Two character crate name
        assert_eq!(CargoRegistryService::get_crate_name_from_path("/ab"), None);
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/1/ab"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/2/ab"),
            Some("ab")
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/ab"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/ab/ab"),
            None
        );

        // Three character crate name
        assert_eq!(CargoRegistryService::get_crate_name_from_path("/abc"), None);
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/1/abc"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/2/abc"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/abc"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/a/abc"),
            Some("abc")
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/ab/abc"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/ab/c/abc"),
            None
        );

        // Four character crate name
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/abcd"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/1/abcd"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/2/abcd"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/abcd"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/a/abcd"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/4/abcd"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/ab/cd/abcd"),
            Some("abcd")
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/ab/cd/abc"),
            None
        );

        // More character crate name
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/abcdefgh"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/1/abcdefgh"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/2/abcdefgh"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/abcdefgh"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/3/a/abcdefgh"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/4/abcdefgh"),
            None
        );
        assert_eq!(
            CargoRegistryService::get_crate_name_from_path("/ab/cd/abcdefgh"),
            Some("abcdefgh")
        );
    }
}
