pub mod blockhash_query;

use crate::nonce;
use clap::{App, Arg, ArgMatches};
use serde_json::Value;
use solana_clap_utils::{
    input_parsers::{pubkey_of, value_of},
    input_validators::{is_hash, is_pubkey_sig},
    offline::{BLOCKHASH_ARG, SIGNER_ARG, SIGN_ONLY_ARG},
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{fee_calculator::FeeCalculator, hash::Hash, pubkey::Pubkey, signature::Signature};
use std::str::FromStr;

fn blockhash_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(BLOCKHASH_ARG.name)
        .long(BLOCKHASH_ARG.long)
        .takes_value(true)
        .value_name("BLOCKHASH")
        .validator(is_hash)
        .help(BLOCKHASH_ARG.help)
}

fn sign_only_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(SIGN_ONLY_ARG.name)
        .long(SIGN_ONLY_ARG.long)
        .takes_value(false)
        .requires(BLOCKHASH_ARG.name)
        .help(SIGN_ONLY_ARG.help)
}

fn signer_arg<'a, 'b>() -> Arg<'a, 'b> {
    Arg::with_name(SIGNER_ARG.name)
        .long(SIGNER_ARG.long)
        .takes_value(true)
        .value_name("BASE58_PUBKEY=BASE58_SIG")
        .validator(is_pubkey_sig)
        .requires(BLOCKHASH_ARG.name)
        .multiple(true)
        .help(SIGNER_ARG.help)
}

pub trait OfflineArgs {
    fn offline_args(self) -> Self;
}

impl OfflineArgs for App<'_, '_> {
    fn offline_args(self) -> Self {
        self.arg(blockhash_arg())
            .arg(sign_only_arg())
            .arg(signer_arg())
    }
}

pub fn parse_sign_only_reply_string(reply: &str) -> (Hash, Vec<(Pubkey, Signature)>) {
    let object: Value = serde_json::from_str(&reply).unwrap();
    let blockhash_str = object.get("blockhash").unwrap().as_str().unwrap();
    let blockhash = blockhash_str.parse::<Hash>().unwrap();
    let signer_strings = object.get("signers").unwrap().as_array().unwrap();
    let signers = signer_strings
        .iter()
        .map(|signer_string| {
            let mut signer = signer_string.as_str().unwrap().split('=');
            let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
            let sig = Signature::from_str(signer.next().unwrap()).unwrap();
            (key, sig)
        })
        .collect();
    (blockhash, signers)
}
