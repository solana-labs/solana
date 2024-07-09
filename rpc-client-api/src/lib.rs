#![allow(clippy::arithmetic_side_effects)]

pub mod client_error;
pub mod config;
pub mod custom_error;
pub mod error_object;
pub mod filter;
pub mod request;
pub mod response;

#[macro_use]
extern crate serde_derive;
