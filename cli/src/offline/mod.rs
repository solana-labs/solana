pub mod blockhash_query;

use crate::nonce;
use clap::{App, Arg, ArgMatches};
use serde_json::Value;
use solana_clap_utils::{
    input_parsers::{pubkey_of, value_of},
    input_validators::{is_hash, is_pubkey_sig},
    keypair::presigner_from_pubkey_sigs,
    offline::{BLOCKHASH_ARG, SIGNER_ARG, SIGN_ONLY_ARG},
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    fee_calculator::FeeCalculator,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Presigner, Signature},
};
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
        .value_name("PUBKEY=SIGNATURE")
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

pub struct SignOnly {
    pub blockhash: Hash,
    pub present_signers: Vec<(Pubkey, Signature)>,
    pub absent_signers: Vec<Pubkey>,
    pub bad_signers: Vec<Pubkey>,
}

impl SignOnly {
    pub fn has_all_signers(&self) -> bool {
        self.absent_signers.is_empty() && self.bad_signers.is_empty()
    }

    pub fn presigner_of(&self, pubkey: &Pubkey) -> Option<Presigner> {
        presigner_from_pubkey_sigs(pubkey, &self.present_signers)
    }
}

pub fn parse_sign_only_reply_string(reply: &str) -> SignOnly {
    let object: Value = serde_json::from_str(&reply).unwrap();
    let blockhash_str = object.get("blockhash").unwrap().as_str().unwrap();
    let blockhash = blockhash_str.parse::<Hash>().unwrap();
    let signer_strings = object.get("signers").unwrap().as_array().unwrap();
    let present_signers = signer_strings
        .iter()
        .map(|signer_string| {
            let mut signer = signer_string.as_str().unwrap().split('=');
            let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
            let sig = Signature::from_str(signer.next().unwrap()).unwrap();
            (key, sig)
        })
        .collect();
    let signer_strings = object.get("absent").unwrap().as_array().unwrap();
    let absent_signers = signer_strings
        .iter()
        .map(|val| {
            let s = val.as_str().unwrap();
            Pubkey::from_str(s).unwrap()
        })
        .collect();
    let signer_strings = object.get("badSig").unwrap().as_array().unwrap();
    let bad_signers = signer_strings
        .iter()
        .map(|val| {
            let s = val.as_str().unwrap();
            Pubkey::from_str(s).unwrap()
        })
        .collect();
    SignOnly {
        blockhash,
        present_signers,
        absent_signers,
        bad_signers,
    }
}
