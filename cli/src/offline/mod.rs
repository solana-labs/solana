pub mod blockhash_query;

use serde_json::Value;
use solana_clap_utils::keypair::presigner_from_pubkey_sigs;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    fee_calculator::FeeCalculator,
    hash::Hash,
    pubkey::Pubkey,
    signature::{Presigner, Signature},
};
use std::str::FromStr;

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
    let mut present_signers: Vec<(Pubkey, Signature)> = Vec::new();
    let signer_strings = object.get("signers");
    if let Some(sig_strings) = signer_strings {
        present_signers = sig_strings
            .as_array()
            .unwrap()
            .iter()
            .map(|signer_string| {
                let mut signer = signer_string.as_str().unwrap().split('=');
                let key = Pubkey::from_str(signer.next().unwrap()).unwrap();
                let sig = Signature::from_str(signer.next().unwrap()).unwrap();
                (key, sig)
            })
            .collect();
    }
    let mut absent_signers: Vec<Pubkey> = Vec::new();
    let signer_strings = object.get("absent");
    if let Some(sig_strings) = signer_strings {
        absent_signers = sig_strings
            .as_array()
            .unwrap()
            .iter()
            .map(|val| {
                let s = val.as_str().unwrap();
                Pubkey::from_str(s).unwrap()
            })
            .collect();
    }
    let mut bad_signers: Vec<Pubkey> = Vec::new();
    let signer_strings = object.get("badSig");
    if let Some(sig_strings) = signer_strings {
        bad_signers = sig_strings
            .as_array()
            .unwrap()
            .iter()
            .map(|val| {
                let s = val.as_str().unwrap();
                Pubkey::from_str(s).unwrap()
            })
            .collect();
    }
    SignOnly {
        blockhash,
        present_signers,
        absent_signers,
        bad_signers,
    }
}
