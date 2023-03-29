use {
    clap::ArgMatches,
    solana_clap_utils::{
        input_parsers::{pubkey_of, value_of},
        nonce::*,
        offline::*,
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig, fee_calculator::FeeCalculator, hash::Hash,
        pubkey::Pubkey,
    },
};

#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Cluster,
    NonceAccount(Pubkey),
}

impl Source {
    #[deprecated(since = "1.9.0", note = "Please use `get_blockhash` instead")]
    pub fn get_blockhash_and_fee_calculator(
        &self,
        rpc_client: &RpcClient,
        commitment: CommitmentConfig,
    ) -> Result<(Hash, FeeCalculator), Box<dyn std::error::Error>> {
        match self {
            Self::Cluster => {
                #[allow(deprecated)]
                let res = rpc_client
                    .get_recent_blockhash_with_commitment(commitment)?
                    .value;
                Ok((res.0, res.1))
            }
            Self::NonceAccount(ref pubkey) => {
                #[allow(clippy::redundant_closure)]
                let data = crate::get_account_with_commitment(rpc_client, pubkey, commitment)
                    .and_then(|ref a| crate::data_from_account(a))?;
                Ok((data.blockhash(), data.fee_calculator))
            }
        }
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please do not use, will no longer be available in the future"
    )]
    pub fn get_fee_calculator(
        &self,
        rpc_client: &RpcClient,
        blockhash: &Hash,
        commitment: CommitmentConfig,
    ) -> Result<Option<FeeCalculator>, Box<dyn std::error::Error>> {
        match self {
            Self::Cluster => {
                #[allow(deprecated)]
                let res = rpc_client
                    .get_fee_calculator_for_blockhash_with_commitment(blockhash, commitment)?
                    .value;
                Ok(res)
            }
            Self::NonceAccount(ref pubkey) => {
                let res = crate::get_account_with_commitment(rpc_client, pubkey, commitment)?;
                let res = crate::data_from_account(&res)?;
                Ok(Some(res)
                    .filter(|d| d.blockhash() == *blockhash)
                    .map(|d| d.fee_calculator))
            }
        }
    }

    pub fn get_blockhash(
        &self,
        rpc_client: &RpcClient,
        commitment: CommitmentConfig,
    ) -> Result<Hash, Box<dyn std::error::Error>> {
        match self {
            Self::Cluster => {
                let (blockhash, _) = rpc_client.get_latest_blockhash_with_commitment(commitment)?;
                Ok(blockhash)
            }
            Self::NonceAccount(ref pubkey) => {
                #[allow(clippy::redundant_closure)]
                let data = crate::get_account_with_commitment(rpc_client, pubkey, commitment)
                    .and_then(|ref a| crate::data_from_account(a))?;
                Ok(data.blockhash())
            }
        }
    }

    pub fn is_blockhash_valid(
        &self,
        rpc_client: &RpcClient,
        blockhash: &Hash,
        commitment: CommitmentConfig,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(match self {
            Self::Cluster => rpc_client.is_blockhash_valid(blockhash, commitment)?,
            Self::NonceAccount(ref pubkey) => {
                #[allow(clippy::redundant_closure)]
                let _ = crate::get_account_with_commitment(rpc_client, pubkey, commitment)
                    .and_then(|ref a| crate::data_from_account(a))?;
                true
            }
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
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
        let nonce_account = pubkey_of(matches, NONCE_ARG.name);
        BlockhashQuery::new(blockhash, sign_only, nonce_account)
    }

    #[deprecated(since = "1.9.0", note = "Please use `get_blockhash` instead")]
    pub fn get_blockhash_and_fee_calculator(
        &self,
        rpc_client: &RpcClient,
        commitment: CommitmentConfig,
    ) -> Result<(Hash, FeeCalculator), Box<dyn std::error::Error>> {
        match self {
            BlockhashQuery::None(hash) => Ok((*hash, FeeCalculator::default())),
            BlockhashQuery::FeeCalculator(source, hash) => {
                #[allow(deprecated)]
                let fee_calculator = source
                    .get_fee_calculator(rpc_client, hash, commitment)?
                    .ok_or(format!("Hash has expired {hash:?}"))?;
                Ok((*hash, fee_calculator))
            }
            BlockhashQuery::All(source) =>
            {
                #[allow(deprecated)]
                source.get_blockhash_and_fee_calculator(rpc_client, commitment)
            }
        }
    }

    pub fn get_blockhash(
        &self,
        rpc_client: &RpcClient,
        commitment: CommitmentConfig,
    ) -> Result<Hash, Box<dyn std::error::Error>> {
        match self {
            BlockhashQuery::None(hash) => Ok(*hash),
            BlockhashQuery::FeeCalculator(source, hash) => {
                if !source.is_blockhash_valid(rpc_client, hash, commitment)? {
                    return Err(format!("Hash has expired {hash:?}").into());
                }
                Ok(*hash)
            }
            BlockhashQuery::All(source) => source.get_blockhash(rpc_client, commitment),
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
    use {
        super::*,
        crate::blockhash_query,
        clap::App,
        serde_json::{self, json},
        solana_account_decoder::{UiAccount, UiAccountEncoding},
        solana_rpc_client_api::{
            request::RpcRequest,
            response::{Response, RpcFeeCalculator, RpcFees, RpcResponseContext},
        },
        solana_sdk::{
            account::Account,
            hash::hash,
            nonce::{self, state::DurableNonce},
            system_program,
        },
        std::collections::HashMap,
    };

    #[test]
    fn test_blockhash_query_new_ok() {
        let blockhash = hash(&[1u8]);
        let nonce_pubkey = Pubkey::from([1u8; 32]);

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
        let nonce_pubkey = Pubkey::from([1u8; 32]);
        BlockhashQuery::new(None, true, Some(nonce_pubkey));
    }

    #[test]
    fn test_blockhash_query_new_from_matches_ok() {
        let test_commands = App::new("blockhash_query_test")
            .nonce_args(false)
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

        let nonce_pubkey = Pubkey::from([1u8; 32]);
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

        let matches = test_commands.get_matches_from(vec!["blockhash_query_test", "--sign-only"]);
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
        let nonce_pubkey = Pubkey::from([1u8; 32]);
        let nonce_string = nonce_pubkey.to_string();

        let matches = test_commands.get_matches_from(vec![
            "blockhash_query_test",
            "--sign-only",
            "--nonce",
            &nonce_string,
        ]);
        BlockhashQuery::new_from_matches(&matches);
    }

    #[test]
    #[allow(deprecated)]
    fn test_blockhash_query_get_blockhash_fee_calc() {
        let test_blockhash = hash(&[0u8]);
        let rpc_blockhash = hash(&[1u8]);
        let rpc_fee_calc = FeeCalculator::new(42);
        let get_recent_blockhash_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!(RpcFees {
                blockhash: rpc_blockhash.to_string(),
                fee_calculator: rpc_fee_calc,
                last_valid_slot: 42,
                last_valid_block_height: 42,
            }),
        });
        let get_fee_calculator_for_blockhash_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!(RpcFeeCalculator {
                fee_calculator: rpc_fee_calc
            }),
        });
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetFees, get_recent_blockhash_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::default()
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .unwrap(),
            (rpc_blockhash, rpc_fee_calc),
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetFees, get_recent_blockhash_response.clone());
        mocks.insert(
            RpcRequest::GetFeeCalculatorForBlockhash,
            get_fee_calculator_for_blockhash_response,
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::FeeCalculator(Source::Cluster, test_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .unwrap(),
            (test_blockhash, rpc_fee_calc),
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetFees, get_recent_blockhash_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::None(test_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .unwrap(),
            (test_blockhash, FeeCalculator::default()),
        );
        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::default()
            .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
            .is_err());

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new(&[2u8; 32]));
        let nonce_blockhash = *durable_nonce.as_hash();
        let nonce_fee_calc = FeeCalculator::new(4242);
        let data = nonce::state::Data {
            authority: Pubkey::from([3u8; 32]),
            durable_nonce,
            fee_calculator: nonce_fee_calc,
        };
        let nonce_account = Account::new_data_with_space(
            42,
            &nonce::state::Versions::new(nonce::State::Initialized(data)),
            nonce::State::size(),
            &system_program::id(),
        )
        .unwrap();
        let nonce_pubkey = Pubkey::from([4u8; 32]);
        let rpc_nonce_account = UiAccount::encode(
            &nonce_pubkey,
            &nonce_account,
            UiAccountEncoding::Base64,
            None,
            None,
        );
        let get_account_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!(Some(rpc_nonce_account)),
        });

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::All(Source::NonceAccount(nonce_pubkey))
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .unwrap(),
            (nonce_blockhash, nonce_fee_calc),
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::FeeCalculator(Source::NonceAccount(nonce_pubkey), nonce_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .unwrap(),
            (nonce_blockhash, nonce_fee_calc),
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert!(
            BlockhashQuery::FeeCalculator(Source::NonceAccount(nonce_pubkey), test_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .is_err()
        );
        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::None(nonce_blockhash)
                .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
                .unwrap(),
            (nonce_blockhash, FeeCalculator::default()),
        );

        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::All(Source::NonceAccount(nonce_pubkey))
            .get_blockhash_and_fee_calculator(&rpc_client, CommitmentConfig::default())
            .is_err());
    }
}
