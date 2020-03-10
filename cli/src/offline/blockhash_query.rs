use super::*;

#[derive(Debug, PartialEq)]
pub enum Source {
    Cluster,
    NonceAccount(Pubkey),
}

impl Source {
    pub fn get_blockhash_fee_calculator(
        &self,
        rpc_client: &RpcClient,
    ) -> Result<(Hash, FeeCalculator), Box<dyn std::error::Error>> {
        match self {
            Self::Cluster => {
                let res = rpc_client.get_recent_blockhash()?;
                Ok(res)
            }
            Self::NonceAccount(ref pubkey) => {
                let data = nonce::get_account(rpc_client, pubkey)
                    .and_then(|ref a| nonce::data_from_account(a))?;
                Ok((data.blockhash, data.fee_calculator))
            }
        }
    }

    pub fn get_fee_calculator(
        &self,
        rpc_client: &RpcClient,
        blockhash: &Hash,
    ) -> Result<Option<FeeCalculator>, Box<dyn std::error::Error>> {
        match self {
            Self::Cluster => {
                let res = rpc_client.get_fee_calculator_for_blockhash(blockhash)?;
                Ok(res)
            }
            Self::NonceAccount(ref pubkey) => {
                let res = nonce::get_account(rpc_client, pubkey)
                    .and_then(|ref a| nonce::data_from_account(a))
                    .and_then(|d| {
                        if d.blockhash == *blockhash {
                            Ok(Some(d.fee_calculator))
                        } else {
                            Ok(None)
                        }
                    })?;
                Ok(res)
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum BlockhashQuery {
    None(Hash),
    FeeCalculator(Source, Hash),
    All(Source),
}

impl BlockhashQuery {
    pub fn new(
        blockhash: Option<Hash>,
        sign_only: bool,
        nonce_account: Option<Pubkey>,
    ) -> Self {
        let source = nonce_account
            .map(Source::NonceAccount)
            .unwrap_or(Source::Cluster);
        match blockhash {
            Some(hash) if sign_only => Self::None(hash),
            Some(hash) if !sign_only => Self::FeeCalculator(source, hash),
            None if !sign_only => Self::All(source),
            _ => panic!("Cannot resolve blockhash"),
        }
    }

    pub fn new_from_matches(matches: &ArgMatches<'_>) -> Self {
        let blockhash = value_of(matches, BLOCKHASH_ARG.name);
        let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
        let nonce_account = pubkey_of(matches, nonce::NONCE_ARG.name);
        BlockhashQuery::new(blockhash, sign_only, nonce_account)
    }

    pub fn get_blockhash_fee_calculator(
        &self,
        rpc_client: &RpcClient,
    ) -> Result<(Hash, FeeCalculator), Box<dyn std::error::Error>> {
        match self {
            BlockhashQuery::None(hash) => Ok((*hash, FeeCalculator::default())),
            BlockhashQuery::FeeCalculator(source, hash) => {
                let fee_calculator = source
                    .get_fee_calculator(rpc_client, hash)?
                    .ok_or(format!("Has has expired {:?}", hash))?;
                Ok((*hash, fee_calculator))
            }
            BlockhashQuery::All(source) => source.get_blockhash_fee_calculator(rpc_client),
        }
    }
}

impl Default for BlockhashQuery {
    fn default() -> Self {
        BlockhashQuery::All(Source::Cluster)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline::blockhash_query::BlockhashQuery;
    use clap::App;
    use serde_json::{self, json, Value};
    use solana_client::{
        rpc_request::RpcRequest,
        rpc_response::{Response, RpcFeeCalculator, RpcResponseContext},
    };
    use solana_sdk::{fee_calculator::FeeCalculator, hash::hash};
    use std::collections::HashMap;

    #[test]
    fn test_blockhash_query_new_ok() {
        let blockhash = hash(&[1u8]);

        assert_eq!(
            BlockhashQuery::new(Some(blockhash), true, None),
            BlockhashQuery::None(blockhash),
        );
        assert_eq!(
            BlockhashQuery::new(Some(blockhash), false, None),
            BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        );
        assert_eq!(
            BlockhashQuery::new(None, false, None),
            BlockhashQuery::All(blockhash_query::Source::Cluster)
        );
    }

    #[test]
    #[should_panic]
    fn test_blockhash_query_new_fail() {
        BlockhashQuery::new(None, true, None);
    }

    #[test]
    fn test_blockhash_query_new_from_matches_ok() {
        let test_commands = App::new("blockhash_query_test").offline_args();
        let blockhash = hash(&[1u8]);
        let blockhash_string = blockhash.to_string();

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhash_query_test",
            "--blockhash",
            &blockhash_string,
            "--sign-only",
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::None(blockhash),
        );

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhash_query_test",
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::FeeCalculator(blockhash_query::Source::Cluster, blockhash),
        );

        let matches = test_commands
            .clone()
            .get_matches_from(vec!["blockhash_query_test"]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::All(blockhash_query::Source::Cluster),
        );
    }

    #[test]
    #[should_panic]
    fn test_blockhash_query_new_from_matches_fail() {
        let test_commands = App::new("blockhash_query_test")
            .arg(blockhash_arg())
            // We can really only hit this case unless the arg requirements
            // are broken, so unset the requires() to recreate that condition
            .arg(sign_only_arg().requires(""));

        let matches = test_commands
            .clone()
            .get_matches_from(vec!["blockhash_query_test", "--sign-only"]);
        BlockhashQuery::new_from_matches(&matches);
    }

    #[test]
    fn test_blockhash_query_get_blockhash_fee_calc() {
        let test_blockhash = hash(&[0u8]);
        let rpc_blockhash = hash(&[1u8]);
        let rpc_fee_calc = FeeCalculator::new(42);
        let get_recent_blockhash_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!((
                Value::String(rpc_blockhash.to_string()),
                serde_json::to_value(rpc_fee_calc.clone()).unwrap()
            )),
        });
        let get_fee_calculator_for_blockhash_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!(RpcFeeCalculator {
                fee_calculator: rpc_fee_calc.clone()
            }),
        });
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetRecentBlockhash,
            get_recent_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::default()
                .get_blockhash_fee_calculator(&rpc_client)
                .unwrap(),
            (rpc_blockhash, rpc_fee_calc.clone()),
        );
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetRecentBlockhash,
            get_recent_blockhash_response.clone(),
        );
        mocks.insert(
            RpcRequest::GetFeeCalculatorForBlockhash,
            get_fee_calculator_for_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::new(Some(test_blockhash), false, None)
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
            BlockhashQuery::new(Some(test_blockhash), true, None)
                .get_blockhash_fee_calculator(&rpc_client)
                .unwrap(),
            (test_blockhash, FeeCalculator::default()),
        );
        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::default()
            .get_blockhash_fee_calculator(&rpc_client)
            .is_err());
    }
}
