use {crate::response_builder, log::error};

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

pub(crate) fn success_response() -> hyper::Response<hyper::Body> {
    response_builder::success_response_str("")
}
