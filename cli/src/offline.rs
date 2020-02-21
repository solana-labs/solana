use clap::{App, Arg, ArgMatches};
use serde_json::Value;
use solana_clap_utils::{
    input_parsers::value_of,
    input_validators::{is_hash, is_pubkey_sig},
    offline::{BLOCKHASH_ARG, SIGNER_ARG, SIGN_ONLY_ARG},
};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{fee_calculator::FeeCalculator, hash::Hash, pubkey::Pubkey, signature::Signature};
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq)]
pub enum BlockhashQuery {
    None(Hash, FeeCalculator),
    FeeCalculator(Hash),
    All,
}

impl BlockhashQuery {
    pub fn new(blockhash: Option<Hash>, sign_only: bool) -> Self {
        match blockhash {
            Some(hash) if sign_only => Self::None(hash, FeeCalculator::default()),
            Some(hash) if !sign_only => Self::FeeCalculator(hash),
            None if !sign_only => Self::All,
            _ => panic!("Cannot resolve blockhash"),
        }
    }

    pub fn new_from_matches(matches: &ArgMatches<'_>) -> Self {
        let blockhash = value_of(matches, BLOCKHASH_ARG.name);
        let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
        BlockhashQuery::new(blockhash, sign_only)
    }

    pub fn get_blockhash_fee_calculator(
        &self,
        rpc_client: &RpcClient,
    ) -> Result<(Hash, FeeCalculator), Box<dyn std::error::Error>> {
        let (hash, fee_calc) = match self {
            BlockhashQuery::None(hash, fee_calc) => (Some(hash), Some(fee_calc)),
            BlockhashQuery::FeeCalculator(hash) => (Some(hash), None),
            BlockhashQuery::All => (None, None),
        };
        if None == fee_calc {
            let (cluster_hash, fee_calc) = rpc_client.get_recent_blockhash()?;
            Ok((*hash.unwrap_or(&cluster_hash), fee_calc))
        } else {
            Ok((*hash.unwrap(), fee_calc.unwrap().clone()))
        }
    }
}

impl Default for BlockhashQuery {
    fn default() -> Self {
        BlockhashQuery::All
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::App;
    use serde_json::{self, json, Value};
    use solana_client::{
        rpc_request::RpcRequest,
        rpc_response::{Response, RpcResponseContext},
    };
    use solana_sdk::{fee_calculator::FeeCalculator, hash::hash};
    use std::collections::HashMap;

    #[test]
    fn test_blockhashspec_new_ok() {
        let blockhash = hash(&[1u8]);

        assert_eq!(
            BlockhashQuery::new(Some(blockhash), true),
            BlockhashQuery::None(blockhash, FeeCalculator::default()),
        );
        assert_eq!(
            BlockhashQuery::new(Some(blockhash), false),
            BlockhashQuery::FeeCalculator(blockhash),
        );
        assert_eq!(BlockhashQuery::new(None, false), BlockhashQuery::All,);
    }

    #[test]
    #[should_panic]
    fn test_blockhashspec_new_fail() {
        BlockhashQuery::new(None, true);
    }

    #[test]
    fn test_blockhashspec_new_from_matches_ok() {
        let test_commands = App::new("blockhashspec_test").offline_args();
        let blockhash = hash(&[1u8]);
        let blockhash_string = blockhash.to_string();

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhashspec_test",
            "--blockhash",
            &blockhash_string,
            "--sign-only",
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::None(blockhash, FeeCalculator::default()),
        );

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhashspec_test",
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::FeeCalculator(blockhash),
        );

        let matches = test_commands
            .clone()
            .get_matches_from(vec!["blockhashspec_test"]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::All,
        );
    }

    #[test]
    #[should_panic]
    fn test_blockhashspec_new_from_matches_fail() {
        let test_commands = App::new("blockhashspec_test")
            .arg(blockhash_arg())
            // We can really only hit this case unless the arg requirements
            // are broken, so unset the requires() to recreate that condition
            .arg(sign_only_arg().requires(""));

        let matches = test_commands
            .clone()
            .get_matches_from(vec!["blockhashspec_test", "--sign-only"]);
        BlockhashQuery::new_from_matches(&matches);
    }

    #[test]
    fn test_blockhashspec_get_blockhash_fee_calc() {
        let test_blockhash = hash(&[0u8]);
        let rpc_blockhash = hash(&[1u8]);
        let rpc_fee_calc = FeeCalculator::new(42, 42);
        let get_recent_blockhash_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!((
                Value::String(rpc_blockhash.to_string()),
                serde_json::to_value(rpc_fee_calc.clone()).unwrap()
            )),
        });
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetRecentBlockhash,
            get_recent_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::All
                .get_blockhash_fee_calculator(&rpc_client)
                .unwrap(),
            (rpc_blockhash, rpc_fee_calc.clone()),
        );
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetRecentBlockhash,
            get_recent_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::FeeCalculator(test_blockhash)
                .get_blockhash_fee_calculator(&rpc_client)
                .unwrap(),
            (test_blockhash, rpc_fee_calc.clone()),
        );
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetRecentBlockhash,
            get_recent_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::None(test_blockhash, FeeCalculator::default())
                .get_blockhash_fee_calculator(&rpc_client)
                .unwrap(),
            (test_blockhash, FeeCalculator::default()),
        );
        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::All
            .get_blockhash_fee_calculator(&rpc_client)
            .is_err());
    }
}
