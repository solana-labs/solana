//! Durable transaction nonce helpers.

pub mod blockhash_query;
pub mod nonblocking;

pub use crate::nonblocking::{
    account_identity_ok, data_from_account, data_from_state, state_from_account, Error,
};
use {
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey},
};

/// Get a nonce account from the network.
///
/// This is like [`RpcClient::get_account`] except:
///
/// - it returns this module's [`Error`] type,
/// - it returns an error if any of the checks from [`account_identity_ok`] fail.
pub fn get_account(rpc_client: &RpcClient, nonce_pubkey: &Pubkey) -> Result<Account, Error> {
    get_account_with_commitment(rpc_client, nonce_pubkey, CommitmentConfig::default())
}

/// Get a nonce account from the network.
///
/// This is like [`RpcClient::get_account_with_commitment`] except:
///
/// - it returns this module's [`Error`] type,
/// - it returns an error if the account does not exist,
/// - it returns an error if any of the checks from [`account_identity_ok`] fail.
pub fn get_account_with_commitment(
    rpc_client: &RpcClient,
    nonce_pubkey: &Pubkey,
    commitment: CommitmentConfig,
) -> Result<Account, Error> {
    rpc_client
        .get_account_with_commitment(nonce_pubkey, commitment)
        .map_err(|e| Error::Client(format!("{e}")))
        .and_then(|result| {
            result
                .value
                .ok_or_else(|| Error::Client(format!("AccountNotFound: pubkey={nonce_pubkey}")))
        })
        .and_then(|a| account_identity_ok(&a).map(|()| a))
}
