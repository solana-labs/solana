use {
    crate::rpc_client::RpcClient,
    solana_sdk::{
        account::{Account, ReadableAccount},
        account_utils::StateMut,
        commitment_config::CommitmentConfig,
        nonce::{
            state::{Data, Versions},
            State,
        },
        pubkey::Pubkey,
        system_program,
    },
};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum Error {
    #[error("invalid account owner")]
    InvalidAccountOwner,
    #[error("invalid account data")]
    InvalidAccountData,
    #[error("unexpected account data size")]
    UnexpectedDataSize,
    #[error("query hash does not match stored hash")]
    InvalidHash,
    #[error("query authority does not match account authority")]
    InvalidAuthority,
    #[error("invalid state for requested operation")]
    InvalidStateForOperation,
    #[error("client error: {0}")]
    Client(String),
}

pub fn get_account(rpc_client: &RpcClient, nonce_pubkey: &Pubkey) -> Result<Account, Error> {
    get_account_with_commitment(rpc_client, nonce_pubkey, CommitmentConfig::default())
}

pub fn get_account_with_commitment(
    rpc_client: &RpcClient,
    nonce_pubkey: &Pubkey,
    commitment: CommitmentConfig,
) -> Result<Account, Error> {
    rpc_client
        .get_account_with_commitment(nonce_pubkey, commitment)
        .map_err(|e| Error::Client(format!("{}", e)))
        .and_then(|result| {
            result
                .value
                .ok_or_else(|| Error::Client(format!("AccountNotFound: pubkey={}", nonce_pubkey)))
        })
        .and_then(|a| account_identity_ok(&a).map(|()| a))
}

pub fn account_identity_ok<T: ReadableAccount>(account: &T) -> Result<(), Error> {
    if account.owner() != &system_program::id() {
        Err(Error::InvalidAccountOwner)
    } else if account.data().is_empty() {
        Err(Error::UnexpectedDataSize)
    } else {
        Ok(())
    }
}

pub fn state_from_account<T: ReadableAccount + StateMut<Versions>>(
    account: &T,
) -> Result<State, Error> {
    account_identity_ok(account)?;
    StateMut::<Versions>::state(account)
        .map_err(|_| Error::InvalidAccountData)
        .map(|v| v.convert_to_current())
}

pub fn data_from_account<T: ReadableAccount + StateMut<Versions>>(
    account: &T,
) -> Result<Data, Error> {
    account_identity_ok(account)?;
    state_from_account(account).and_then(|ref s| data_from_state(s).map(|d| d.clone()))
}

pub fn data_from_state(state: &State) -> Result<&Data, Error> {
    match state {
        State::Uninitialized => Err(Error::InvalidStateForOperation),
        State::Initialized(data) => Ok(data),
    }
}
