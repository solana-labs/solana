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

pub mod solana_rpc_client {
    pub mod rpc_client {
        use {
            super::super::{
                solana_rpc_client_api::client_error::Result as ClientResult,
                solana_sdk::{
                    account::Account, hash::Hash, pubkey::Pubkey, signature::Signature,
                    transaction::Transaction,
                },
            },
            std::{cell::RefCell, collections::HashMap, rc::Rc},
        };

        #[derive(Default)]
        pub struct RpcClient {
            get_account_responses: Rc<RefCell<HashMap<Pubkey, Account>>>,
        }

        impl RpcClient {
            pub fn new(_url: String) -> Self {
                RpcClient::default()
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

            pub fn get_account(&self, pubkey: &Pubkey) -> ClientResult<Account> {
                Ok(self
                    .get_account_responses
                    .borrow()
                    .get(pubkey)
                    .cloned()
                    .unwrap())
            }

            pub fn set_get_account_response(&self, pubkey: Pubkey, account: Account) {
                self.get_account_responses
                    .borrow_mut()
                    .insert(pubkey, account);
            }

            pub fn get_balance(&self, _pubkey: &Pubkey) -> ClientResult<u64> {
                Ok(0)
            }
        }
    }
}

pub mod solana_rpc_client_api {
    pub mod client_error {
        #[derive(thiserror::Error, Debug)]
        #[error("mock-error")]
        pub struct ClientError;
        pub type Result<T> = std::result::Result<T, ClientError>;
    }
}

pub mod solana_rpc_client_nonce_utils {
    use {
        super::solana_sdk::{account::ReadableAccount, account_utils::StateMut, pubkey::Pubkey},
        crate::nonce::state::{Data, DurableNonce, Versions},
    };

    #[derive(thiserror::Error, Debug)]
    #[error("mock-error")]
    pub struct Error;

    pub fn data_from_account<T: ReadableAccount + StateMut<Versions>>(
        _account: &T,
    ) -> Result<Data, Error> {
        Ok(Data::new(
            Pubkey::new_unique(),
            DurableNonce::default(),
            5000,
        ))
    }
}

/// Re-exports and mocks of solana-program modules that mirror those from
/// solana-program.
///
/// This lets examples in solana-program appear to be written as client
/// programs.
pub mod solana_sdk {
    pub use crate::{
        address_lookup_table_account, hash, instruction, keccak, message, nonce,
        pubkey::{self, Pubkey},
        system_instruction, system_program,
        sysvar::{
            self,
            clock::{self, Clock},
        },
    };

    pub mod account {
        use crate::{clock::Epoch, pubkey::Pubkey};
        #[derive(Clone)]
        pub struct Account {
            pub lamports: u64,
            pub data: Vec<u8>,
            pub owner: Pubkey,
            pub executable: bool,
            pub rent_epoch: Epoch,
        }

        pub trait ReadableAccount: Sized {
            fn data(&self) -> &[u8];
        }

        impl ReadableAccount for Account {
            fn data(&self) -> &[u8] {
                &self.data
            }
        }
    }

    pub mod account_utils {
        use super::account::Account;

        pub trait StateMut<T> {}

        impl<T> StateMut<T> for Account {}
    }

    pub mod signature {
        use crate::pubkey::Pubkey;

        #[derive(Default, Debug)]
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

        impl<T: Signer> Signers for [&T] {}
        impl<T: Signer> Signers for [&T; 1] {}
        impl<T: Signer> Signers for [&T; 2] {}
    }

    pub mod signer {
        use thiserror::Error;

        #[derive(Error, Debug)]
        #[error("mock-error")]
        pub struct SignerError;
    }

    pub mod transaction {
        use {
            super::{signature::Signature, signer::SignerError, signers::Signers},
            crate::{
                hash::Hash,
                instruction::Instruction,
                message::{Message, VersionedMessage},
                pubkey::Pubkey,
            },
            serde::Serialize,
        };

        pub struct VersionedTransaction {
            pub signatures: Vec<Signature>,
            pub message: VersionedMessage,
        }

        impl VersionedTransaction {
            pub fn try_new<T: Signers>(
                message: VersionedMessage,
                _keypairs: &T,
            ) -> std::result::Result<Self, SignerError> {
                Ok(VersionedTransaction {
                    signatures: vec![],
                    message,
                })
            }
        }

        #[derive(Serialize)]
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

            pub fn try_sign<T: Signers>(
                &mut self,
                _keypairs: &T,
                _recent_blockhash: Hash,
            ) -> Result<(), SignerError> {
                Ok(())
            }
        }
    }
}

pub mod solana_address_lookup_table_program {
    crate::declare_id!("AddressLookupTab1e1111111111111111111111111");

    pub mod state {
        use {
            crate::{instruction::InstructionError, pubkey::Pubkey},
            std::borrow::Cow,
        };

        pub struct AddressLookupTable<'a> {
            pub addresses: Cow<'a, [Pubkey]>,
        }

        impl<'a> AddressLookupTable<'a> {
            pub fn serialize_for_tests(self) -> Result<Vec<u8>, InstructionError> {
                let mut data = vec![];
                self.addresses.iter().for_each(|address| {
                    data.extend_from_slice(address.as_ref());
                });
                Ok(data)
            }

            pub fn deserialize(data: &'a [u8]) -> Result<AddressLookupTable<'a>, InstructionError> {
                Ok(Self {
                    addresses: Cow::Borrowed(bytemuck::try_cast_slice(data).unwrap()),
                })
            }
        }
    }
}
