//! Mock types for use in examples.
//!
//! These represent APIs from crates that themselves depend on this crate, and
//! which are useful for illustrating the examples for APIs in this crate.
//!
//! Directly depending on these crates though would cause problematic circular
//! dependencies, so instead they are mocked out here in a way that allows
//! examples to appear to use crates that this crate must not depend on.
//!
//! Each mod here has the name of a crate, so that examples can be structured to
//! appear to import from that crate.

#![doc(hidden)]
#![cfg(feature = "full")]

pub mod solana_rpc_client {
    pub mod rpc_client {
        use {
            super::super::solana_rpc_client_api::client_error::Result as ClientResult,
            crate::{hash::Hash, signature::Signature, transaction::Transaction},
        };

        pub struct RpcClient;

        impl RpcClient {
            pub fn new(_url: String) -> Self {
                RpcClient
            }
            pub fn get_latest_blockhash(&self) -> ClientResult<Hash> {
                Ok(Hash::default())
            }
            pub fn send_and_confirm_transaction(
                &self,
                _transaction: &Transaction,
            ) -> ClientResult<Signature> {
                Ok(Signature::default())
            }
        }
    }
}

pub mod solana_rpc_client_api {
    pub mod client_error {
        use thiserror::Error;

        #[derive(Error, Debug)]
        #[error("mock-error")]
        pub struct ClientError;
        pub type Result<T> = std::result::Result<T, ClientError>;
    }
}
