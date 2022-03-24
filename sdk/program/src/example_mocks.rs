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
#![allow(clippy::new_without_default)]

pub mod solana_client {
    pub mod client_error {
        use thiserror::Error;

        #[derive(Error, Debug)]
        #[error("mock-error")]
        pub struct ClientError;
        pub type Result<T> = std::result::Result<T, ClientError>;
    }

    pub mod rpc_client {
        use super::{
            super::solana_sdk::{hash::Hash, signature::Signature, transaction::Transaction},
            client_error::Result as ClientResult,
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

            pub fn get_minimum_balance_for_rent_exemption(
                &self,
                _data_len: usize,
            ) -> ClientResult<u64> {
                Ok(0)
            }
        }
    }
}

/// Re-exports and mocks of solana-program modules that mirror those from
/// solana-program.
///
/// This lets examples in solana-program appear to be written as client
/// programs.
pub mod solana_sdk {
    pub use crate::{hash, instruction, message, nonce, pubkey, system_instruction};

    pub mod signature {
        use crate::pubkey::Pubkey;

        #[derive(Default)]
        pub struct Signature;

        pub struct Keypair;

        impl Keypair {
            pub fn new() -> Keypair {
                Keypair
            }
        }

        impl Signer for Keypair {
            fn pubkey(&self) -> Pubkey {
                Pubkey::default()
            }
        }

        pub trait Signer {
            fn pubkey(&self) -> Pubkey;
        }
    }

    pub mod signers {
        use super::signature::Signer;

        pub trait Signers {}

        impl<T: Signer> Signers for [&T; 1] {}
        impl<T: Signer> Signers for [&T; 2] {}
    }

    pub mod transaction {
        use {
            super::signers::Signers,
            crate::{hash::Hash, instruction::Instruction, message::Message, pubkey::Pubkey},
        };

        pub struct Transaction {
            pub message: Message,
        }

        impl Transaction {
            pub fn new<T: Signers>(
                _from_keypairs: &T,
                _message: Message,
                _recent_blockhash: Hash,
            ) -> Transaction {
                Transaction {
                    message: Message::new(&[], None),
                }
            }

            pub fn new_unsigned(_message: Message) -> Self {
                Transaction {
                    message: Message::new(&[], None),
                }
            }

            pub fn new_with_payer(_instructions: &[Instruction], _payer: Option<&Pubkey>) -> Self {
                Transaction {
                    message: Message::new(&[], None),
                }
            }

            pub fn new_signed_with_payer<T: Signers>(
                instructions: &[Instruction],
                payer: Option<&Pubkey>,
                signing_keypairs: &T,
                recent_blockhash: Hash,
            ) -> Self {
                let message = Message::new(instructions, payer);
                Self::new(signing_keypairs, message, recent_blockhash)
            }

            pub fn sign<T: Signers>(&mut self, _keypairs: &T, _recent_blockhash: Hash) {}
        }
    }
}
