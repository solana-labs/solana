//! The `registry_service` module implements the Solana cargo registry service.
use {
    crate::{
        client::Client,
        crate_handler::{Error, Program, UnpackedCrate},
        sparse_index::RegistryIndex,
    },
    hyper::{
        body,
        service::{make_service_fn, service_fn},
        Method, Server,
    },
    log::*,
    std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
    },
};

mod client;
mod crate_handler;

mod response_builder;
mod sparse_index;

const PATH_PREFIX: &str = "/api/v1/crates";

pub struct CargoRegistryService {}

impl CargoRegistryService {
    async fn handle_publish_request(
        request: hyper::Request<hyper::Body>,
        client: Arc<Client>,
        index: Arc<RegistryIndex>,
    ) -> hyper::Response<hyper::Body> {
        info!("Handling request to publish the crate");
        let bytes = body::to_bytes(request.into_body()).await;

        match bytes {
            Ok(data) => {
                let Ok(unpacked_crate) = UnpackedCrate::new(data) else {
                    return response_builder::error_response(
                        hyper::StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to parse the crate information",
                    );
                };
                let Ok(result) =
                    tokio::task::spawn_blocking(move || unpacked_crate.publish(client, index))
                        .await
                else {
                    return response_builder::error_response(
                        hyper::StatusCode::INTERNAL_SERVER_ERROR,
                        "Internal error. Failed to wait for program deployment",
                    );
                };

                if result.is_ok() {
                    info!("Published the crate successfully. {:?}", result);
                    response_builder::success_response()
                } else {
                    response_builder::error_response(
                        hyper::StatusCode::BAD_REQUEST,
                        format!("Failed to publish the crate. {:?}", result).as_str(),
                    )
                }
            }
            Err(_) => response_builder::error_response(
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

    fn handle_download_crate_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
        client: Arc<Client>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, crate_name, version)) = Self::get_crate_name_and_version(path) else {
            return response_builder::error_in_parsing();
        };

        if path.len() != PATH_PREFIX.len() {
            return response_builder::error_incorrect_length();
        }

        let package = Program::crate_name_to_program_id(crate_name)
            .and_then(|id| UnpackedCrate::fetch(id, version, client).ok());

        // Return the package to the caller in the response
        if let Some((package, _meta)) = package {
            response_builder::success_response_bytes(package.0)
        } else {
            response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Failed to find the package",
            )
        }
    }

    fn handle_yank_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name, _version)) = Self::get_crate_name_and_version(path) else {
            return response_builder::error_in_parsing();
        };

        if path.len() != PATH_PREFIX.len() {
            return response_builder::error_incorrect_length();
        }

        response_builder::error_not_implemented()
    }

    fn handle_unyank_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name, _version)) = Self::get_crate_name_and_version(path) else {
            return response_builder::error_in_parsing();
        };

        if path.len() != PATH_PREFIX.len() {
            return response_builder::error_incorrect_length();
        }

        response_builder::error_not_implemented()
    }

    fn get_crate_name(path: &str) -> Option<(&str, &str)> {
        path.rsplit_once('/')
    }

    fn handle_get_owners_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name)) = Self::get_crate_name(path) else {
            return response_builder::error_in_parsing();
        };

        if path.len() != PATH_PREFIX.len() {
            return response_builder::error_incorrect_length();
        }

        response_builder::error_not_implemented()
    }

    fn handle_add_owners_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name)) = Self::get_crate_name(path) else {
            return response_builder::error_in_parsing();
        };

        if path.len() != PATH_PREFIX.len() {
            return response_builder::error_incorrect_length();
        }

        response_builder::error_not_implemented()
    }

    fn handle_delete_owners_request(
        path: &str,
        _request: &hyper::Request<hyper::Body>,
    ) -> hyper::Response<hyper::Body> {
        let Some((path, _crate_name)) = Self::get_crate_name(path) else {
            return response_builder::error_in_parsing();
        };

        if path.len() != PATH_PREFIX.len() {
            return response_builder::error_incorrect_length();
        }

        response_builder::error_not_implemented()
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
            return response_builder::error_incorrect_length();
        }

        response_builder::error_not_implemented()
    }

    async fn handler(
        index: Arc<sparse_index::RegistryIndex>,
        request: hyper::Request<hyper::Body>,
        client: Arc<Client>,
    ) -> Result<hyper::Response<hyper::Body>, Error> {
        let path = request.uri().path();
        if path.starts_with("/git") {
            return Ok(response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "This registry server does not support GIT index. Please use sparse index.",
            ));
        }

        if path.starts_with(index.index_root.as_str()) {
            return Ok(index.handler(request, client.clone()));
        }

        if !path.starts_with(PATH_PREFIX) {
            return Ok(response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid path for the request",
            ));
        }

        let Some((path, endpoint)) = path.rsplit_once('/') else {
            return Ok(response_builder::error_response(
                hyper::StatusCode::BAD_REQUEST,
                "Invalid endpoint in the path",
            ));
        };

        Ok(match *request.method() {
            Method::PUT => match endpoint {
                "new" => {
                    if path.len() != PATH_PREFIX.len() {
                        response_builder::error_incorrect_length()
                    } else {
                        Self::handle_publish_request(request, client.clone(), index.clone()).await
                    }
                }
                "unyank" => Self::handle_unyank_request(path, &request),
                "owners" => Self::handle_add_owners_request(path, &request),
                _ => response_builder::error_not_allowed(),
            },
            Method::GET => match endpoint {
                "crates" => Self::handle_get_crates_request(path, &request),
                "owners" => Self::handle_get_owners_request(path, &request),
                "download" => Self::handle_download_crate_request(path, &request, client.clone()),
                _ => response_builder::error_not_allowed(),
            },
            Method::DELETE => match endpoint {
                "yank" => Self::handle_yank_request(path, &request),
                "owners" => Self::handle_delete_owners_request(path, &request),
                _ => response_builder::error_not_allowed(),
            },
            _ => response_builder::error_not_allowed(),
        })
    }
}

#[tokio::main]
async fn main() {
    solana_logger::setup_with_default("solana=info");
    let client = Arc::new(Client::new().expect("Failed to get RPC Client instance"));

    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), client.port);
    let index = Arc::new(sparse_index::RegistryIndex::new(
        "/index",
        &client.server_url,
    ));

    let registry_service = make_service_fn(move |_| {
        let client_inner = client.clone();
        let index = index.clone();
        async move {
            Ok::<_, Error>(service_fn(move |request| {
                CargoRegistryService::handler(index.clone(), request, client_inner.clone())
            }))
        }
    });

    let server = Server::bind(&bind_addr).serve(registry_service);
    info!("Server running on http://{}", bind_addr);

    let _ = server.await;
}
