//! The `registry_service` module implements the Solana cargo registry service.
use {
    crate::{
        client::Client,
        dummy_git_index::DummyGitIndex,
        publisher::{Error, Publisher},
    },
    hyper::{
        body,
        service::{make_service_fn, service_fn},
        Method, Server,
    },
    hyper_staticfile::Static,
    log::*,
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::PathBuf,
        sync::Arc,
    },
};

mod client;
mod dummy_git_index;
mod publisher;

const PATH_PREFIX: &str = "/api/v1/crates";

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

    fn success_response() -> hyper::Response<hyper::Body> {
        hyper::Response::builder()
            .status(hyper::StatusCode::OK)
            .body(hyper::Body::from(""))
            .unwrap()
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

    async fn handler(
        request: hyper::Request<hyper::Body>,
        client: Arc<Client>,
    ) -> Result<hyper::Response<hyper::Body>, Error> {
        let path = request.uri().path();
        if path.starts_with("/git") {
            return Static::new("/tmp/dummy-git")
                .serve(request)
                .await
                .or_else(|_| {
                    Ok(Self::error_response(
                        hyper::StatusCode::BAD_REQUEST,
                        "Failed to serve git index",
                    ))
                });
        }

        if !path.starts_with(PATH_PREFIX) {
            return Ok(Self::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for the request",
            ));
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
    let port = client.port;

    let registry_service = make_service_fn(move |_| {
        let client_inner = client.clone();
        async move {
            Ok::<_, Error>(service_fn(move |request| {
                CargoRegistryService::handler(request, client_inner.clone())
            }))
        }
    });

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
    DummyGitIndex::create_or_update_git_repo(PathBuf::from("/tmp/dummy-git"), &addr);

    let server = Server::bind(&addr).serve(registry_service);
    info!("Server running on on http://{}", addr);

    let _ = server.await;
}
