//! The `rpc_signature_status` module defines transaction status codes

use jsonrpc_core::{Error, Result};
use std::str::FromStr;

#[derive(Copy, Clone, PartialEq, Serialize, Debug)]
pub enum RpcSignatureStatus {
    AccountInUse,
    AccountLoadedTwice,
    Confirmed,
    GenericFailure,
    ProgramRuntimeError,
    SignatureNotFound,
}

impl FromStr for RpcSignatureStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<RpcSignatureStatus> {
        match s {
            "AccountInUse" => Ok(RpcSignatureStatus::AccountInUse),
            "AccountLoadedTwice" => Ok(RpcSignatureStatus::AccountLoadedTwice),
            "Confirmed" => Ok(RpcSignatureStatus::Confirmed),
            "GenericFailure" => Ok(RpcSignatureStatus::GenericFailure),
            "ProgramRuntimeError" => Ok(RpcSignatureStatus::ProgramRuntimeError),
            "SignatureNotFound" => Ok(RpcSignatureStatus::SignatureNotFound),
            _ => Err(Error::parse_error()),
        }
    }
}
