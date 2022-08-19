#![allow(clippy::integer_arithmetic)]

pub mod client_error;
pub mod rpc_config;
pub mod rpc_custom_error;
pub mod rpc_deprecated_config;
pub mod rpc_error_object;
pub mod rpc_filter;
pub mod rpc_request;
pub mod rpc_response;
pub mod spinner;

#[macro_use]
extern crate serde_derive;
