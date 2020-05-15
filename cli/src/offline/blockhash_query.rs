use super::*;

#[derive(Debug, PartialEq)]
pub enum Source {
    Cluster,
    NonceAccount(Pubkey),
}

impl Source {
    pub fn get_blockhash_and_fee_calculator(
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
    pub fn new(blockhash: Option<Hash>, sign_only: bool, nonce_account: Option<Pubkey>) -> Self {
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

    pub fn get_blockhash_and_fee_calculator(
        &self,
        rpc_client: &RpcClient,
    ) -> Result<(Hash, FeeCalculator), Box<dyn std::error::Error>> {
        match self {
            BlockhashQuery::None(hash) => Ok((*hash, FeeCalculator::default())),
            BlockhashQuery::FeeCalculator(source, hash) => {
                let fee_calculator = source
                    .get_fee_calculator(rpc_client, hash)?
                    .ok_or(format!("Hash has expired {:?}", hash))?;
                Ok((*hash, fee_calculator))
            }
            BlockhashQuery::All(source) => source.get_blockhash_and_fee_calculator(rpc_client),
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
    use crate::{nonce::nonce_arg, offline::blockhash_query::BlockhashQuery};
    use clap::App;
    use serde_json::{self, json, Value};
    use solana_client::{
        rpc_request::RpcRequest,
        rpc_response::{Response, RpcAccount, RpcFeeCalculator, RpcResponseContext},
    };
    use solana_sdk::{
        account::Account, fee_calculator::FeeCalculator, hash::hash, nonce, system_program,
    };
    use std::collections::HashMap;

    #[test]
    fn test_blockhash_query_new_ok() {
        let blockhash = hash(&[1u8]);
        let nonce_pubkey = Pubkey::new(&[1u8; 32]);

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

        assert_eq!(
            BlockhashQuery::new(Some(blockhash), true, Some(nonce_pubkey)),
            BlockhashQuery::None(blockhash),
        );
        assert_eq!(
            BlockhashQuery::new(Some(blockhash), false, Some(nonce_pubkey)),
            BlockhashQuery::FeeCalculator(
                blockhash_query::Source::NonceAccount(nonce_pubkey),
                blockhash
            ),
        );
        assert_eq!(
            BlockhashQuery::new(None, false, Some(nonce_pubkey)),
            BlockhashQuery::All(blockhash_query::Source::NonceAccount(nonce_pubkey)),
        );
    }

    #[test]
    #[should_panic]
    fn test_blockhash_query_new_no_nonce_fail() {
        BlockhashQuery::new(None, true, None);
    }

    #[test]
    #[should_panic]
    fn test_blockhash_query_new_nonce_fail() {
        let nonce_pubkey = Pubkey::new(&[1u8; 32]);
        BlockhashQuery::new(None, true, Some(nonce_pubkey));
    }

    #[test]
    fn test_blockhash_query_new_from_matches_ok() {
        let test_commands = App::new("blockhash_query_test")
            .arg(nonce_arg())
            .offline_args();
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

        let nonce_pubkey = Pubkey::new(&[1u8; 32]);
        let nonce_string = nonce_pubkey.to_string();
        let matches = test_commands.clone().get_matches_from(vec![
            "blockhash_query_test",
            "--blockhash",
            &blockhash_string,
            "--sign-only",
            "--nonce",
            &nonce_string,
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::None(blockhash),
        );

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhash_query_test",
            "--blockhash",
            &blockhash_string,
            "--nonce",
            &nonce_string,
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::FeeCalculator(
                blockhash_query::Source::NonceAccount(nonce_pubkey),
                blockhash
            ),
        );
    }

    #[test]
    #[should_panic]
    fn test_blockhash_query_new_from_matches_without_nonce_fail() {
        let test_commands = App::new("blockhash_query_test")
            .arg(blockhash_arg())
            // We can really only hit this case if the arg requirements
            // are broken, so unset the requires() to recreate that condition
            .arg(sign_only_arg().requires(""));

        let matches = test_commands
            .clone()
            .get_matches_from(vec!["blockhash_query_test", "--sign-only"]);
        BlockhashQuery::new_from_matches(&matches);
    }

    #[test]
    #[should_panic]
    fn test_blockhash_query_new_from_matches_with_nonce_fail() {
        let test_commands = App::new("blockhash_query_test")
            .arg(blockhash_arg())
            // We can really only hit this case if the arg requirements
            // are broken, so unset the requires() to recreate that condition
            .arg(sign_only_arg().requires(""));
        let nonce_pubkey = Pubkey::new(&[1u8; 32]);
        let nonce_string = nonce_pubkey.to_string();

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhash_query_test",
            "--sign-only",
            "--nonce",
            &nonce_string,
        ]);
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
                .get_blockhash_and_fee_calculator(&rpc_client)
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
            get_fee_calculator_for_blockhash_response,
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::FeeCalculator(Source::Cluster, test_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client)
                .unwrap(),
            (test_blockhash, rpc_fee_calc),
        );
        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetRecentBlockhash,
            get_recent_blockhash_response,
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::None(test_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client)
                .unwrap(),
            (test_blockhash, FeeCalculator::default()),
        );
        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::default()
            .get_blockhash_and_fee_calculator(&rpc_client)
            .is_err());

        let nonce_blockhash = Hash::new(&[2u8; 32]);
        let nonce_fee_calc = FeeCalculator::new(4242);
        let data = nonce::state::Data {
            authority: Pubkey::new(&[3u8; 32]),
            blockhash: nonce_blockhash,
            fee_calculator: nonce_fee_calc.clone(),
        };
        let nonce_account = Account::new_data_with_space(
            42,
            &nonce::state::Versions::new_current(nonce::State::Initialized(data)),
            nonce::State::size(),
            &system_program::id(),
        )
        .unwrap();
        let nonce_pubkey = Pubkey::new(&[4u8; 32]);
        let rpc_nonce_account = RpcAccount::encode(nonce_account);
        let get_account_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!(Some(rpc_nonce_account)),
        });

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::All(Source::NonceAccount(nonce_pubkey))
                .get_blockhash_and_fee_calculator(&rpc_client)
                .unwrap(),
            (nonce_blockhash, nonce_fee_calc.clone()),
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::FeeCalculator(Source::NonceAccount(nonce_pubkey), nonce_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client)
                .unwrap(),
            (nonce_blockhash, nonce_fee_calc),
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert!(
            BlockhashQuery::FeeCalculator(Source::NonceAccount(nonce_pubkey), test_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client)
                .is_err()
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::None(nonce_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client)
                .unwrap(),
            (nonce_blockhash, FeeCalculator::default()),
        );

        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::All(Source::NonceAccount(nonce_pubkey))
            .get_blockhash_and_fee_calculator(&rpc_client)
            .is_err());
    }
}
