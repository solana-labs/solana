use log::error;

pub(crate) fn error_response(status: hyper::StatusCode, msg: &str) -> hyper::Response<hyper::Body> {
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

pub(crate) fn success_response_str(value: &str) -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .status(hyper::StatusCode::OK)
        .body(hyper::Body::from(value.to_string()))
        .unwrap()
}

pub(crate) fn success_response_bytes(bytes: hyper::body::Bytes) -> hyper::Response<hyper::Body> {
    hyper::Response::builder()
        .status(hyper::StatusCode::OK)
        .body(hyper::Body::from(bytes))
        .unwrap()
}

pub(crate) fn success_response() -> hyper::Response<hyper::Body> {
    success_response_str("")
}

pub(crate) fn error_not_allowed() -> hyper::Response<hyper::Body> {
    error_response(hyper::StatusCode::METHOD_NOT_ALLOWED, "Unknown request")
}

pub(crate) fn error_not_implemented() -> hyper::Response<hyper::Body> {
    error_response(
        hyper::StatusCode::NOT_IMPLEMENTED,
        "This command is not implemented yet",
    )
}

pub(crate) fn error_in_parsing() -> hyper::Response<hyper::Body> {
    error_response(
        hyper::StatusCode::BAD_REQUEST,
        "Failed to parse the request",
    )
}

pub(crate) fn error_incorrect_length() -> hyper::Response<hyper::Body> {
    error_response(
        hyper::StatusCode::BAD_REQUEST,
        "Request length is incorrect",
    )
}
