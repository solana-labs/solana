use {
    crate::nonblocking,
    clap::ArgMatches,
    solana_clap_utils::{
        input_parsers::{pubkey_of, value_of},
        nonce::*,
        offline::*,
    },
    solana_rpc_client::nonblocking::rpc_client::RpcClient,
    solana_sdk::{commitment_config::CommitmentConfig, hash::Hash, pubkey::Pubkey},
};

#[derive(Debug, PartialEq, Eq)]
pub enum Source {
    Cluster,
    NonceAccount(Pubkey),
}

impl Source {
    pub async fn get_blockhash(
        &self,
        rpc_client: &RpcClient,
        commitment: CommitmentConfig,
    ) -> Result<Hash, Box<dyn std::error::Error>> {
        match self {
            Self::Cluster => {
                let (blockhash, _) = rpc_client
                    .get_latest_blockhash_with_commitment(commitment)
                    .await?;
                Ok(blockhash)
            }
            Self::NonceAccount(ref pubkey) => {
                #[allow(clippy::redundant_closure)]
                let data = nonblocking::get_account_with_commitment(rpc_client, pubkey, commitment)
                    .await
                    .and_then(|ref a| nonblocking::data_from_account(a))?;
                Ok(data.blockhash())
            }
        }
    }

    pub async fn is_blockhash_valid(
        &self,
        rpc_client: &RpcClient,
        blockhash: &Hash,
        commitment: CommitmentConfig,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        Ok(match self {
            Self::Cluster => rpc_client.is_blockhash_valid(blockhash, commitment).await?,
            Self::NonceAccount(ref pubkey) => {
                #[allow(clippy::redundant_closure)]
                let _ = nonblocking::get_account_with_commitment(rpc_client, pubkey, commitment)
                    .await
                    .and_then(|ref a| nonblocking::data_from_account(a))?;
                true
            }
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BlockhashQuery {
    Static(Hash),
    Validated(Source, Hash),
    Rpc(Source),
}

impl BlockhashQuery {
    pub fn new(blockhash: Option<Hash>, sign_only: bool, nonce_account: Option<Pubkey>) -> Self {
        let source = nonce_account
            .map(Source::NonceAccount)
            .unwrap_or(Source::Cluster);
        match blockhash {
            Some(hash) if sign_only => Self::Static(hash),
            Some(hash) if !sign_only => Self::Validated(source, hash),
            None if !sign_only => Self::Rpc(source),
            _ => panic!("Cannot resolve blockhash"),
        }
    }

    pub fn new_from_matches(matches: &ArgMatches<'_>) -> Self {
        let blockhash = value_of(matches, BLOCKHASH_ARG.name);
        let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
        let nonce_account = pubkey_of(matches, NONCE_ARG.name);
        BlockhashQuery::new(blockhash, sign_only, nonce_account)
    }

    pub async fn get_blockhash(
        &self,
        rpc_client: &RpcClient,
        commitment: CommitmentConfig,
    ) -> Result<Hash, Box<dyn std::error::Error>> {
        match self {
            BlockhashQuery::Static(hash) => Ok(*hash),
            BlockhashQuery::Validated(source, hash) => {
                if !source
                    .is_blockhash_valid(rpc_client, hash, commitment)
                    .await?
                {
                    return Err(format!("Hash has expired {hash:?}").into());
                }
                Ok(*hash)
            }
            BlockhashQuery::Rpc(source) => source.get_blockhash(rpc_client, commitment).await,
        }
    }
}

impl Default for BlockhashQuery {
    fn default() -> Self {
        BlockhashQuery::Rpc(Source::Cluster)
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::nonblocking::blockhash_query,
        clap::App,
        serde_json::{self, json},
        solana_account_decoder::{UiAccount, UiAccountEncoding},
        solana_rpc_client_api::{
            request::RpcRequest,
            response::{Response, RpcBlockhash, RpcResponseContext},
        },
        solana_sdk::{
            account::Account,
            fee_calculator::FeeCalculator,
            hash::hash,
            nonce::{self, state::DurableNonce},
            system_program,
        },
        std::collections::HashMap,
    };

    #[test]
    fn test_blockhash_query_new_ok() {
        let blockhash = hash(&[1u8]);
        let nonce_pubkey = Pubkey::new(&[1u8; 32]);

        assert_eq!(
            BlockhashQuery::new(Some(blockhash), true, None),
            BlockhashQuery::Static(blockhash),
        );
        assert_eq!(
            BlockhashQuery::new(Some(blockhash), false, None),
            BlockhashQuery::Validated(blockhash_query::Source::Cluster, blockhash),
        );
        assert_eq!(
            BlockhashQuery::new(None, false, None),
            BlockhashQuery::Rpc(blockhash_query::Source::Cluster)
        );

        assert_eq!(
            BlockhashQuery::new(Some(blockhash), true, Some(nonce_pubkey)),
            BlockhashQuery::Static(blockhash),
        );
        assert_eq!(
            BlockhashQuery::new(Some(blockhash), false, Some(nonce_pubkey)),
            BlockhashQuery::Validated(
                blockhash_query::Source::NonceAccount(nonce_pubkey),
                blockhash
            ),
        );
        assert_eq!(
            BlockhashQuery::new(None, false, Some(nonce_pubkey)),
            BlockhashQuery::Rpc(blockhash_query::Source::NonceAccount(nonce_pubkey)),
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
            BlockhashQuery::Static(blockhash),
        );

        let matches = test_commands.clone().get_matches_from(vec![
            "blockhash_query_test",
            "--blockhash",
            &blockhash_string,
        ]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::Validated(blockhash_query::Source::Cluster, blockhash),
        );

        let matches = test_commands
            .clone()
            .get_matches_from(vec!["blockhash_query_test"]);
        assert_eq!(
            BlockhashQuery::new_from_matches(&matches),
            BlockhashQuery::Rpc(blockhash_query::Source::Cluster),
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
            BlockhashQuery::Static(blockhash),
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
            BlockhashQuery::Validated(
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
        let nonce_pubkey = Pubkey::new(&[1u8; 32]);
        let nonce_string = nonce_pubkey.to_string();

        let matches = test_commands.get_matches_from(vec![
            "blockhash_query_test",
            "--sign-only",
            "--nonce",
            &nonce_string,
        ]);
        BlockhashQuery::new_from_matches(&matches);
    }

    #[tokio::test]
    async fn test_blockhash_query_get_blockhash() {
        let test_blockhash = hash(&[0u8]);
        let rpc_blockhash = hash(&[1u8]);

        let get_latest_blockhash_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: json!(RpcBlockhash {
                blockhash: rpc_blockhash.to_string(),
                last_valid_block_height: 42,
            }),
        });

        let is_blockhash_valid_response = json!(Response {
            context: RpcResponseContext {
                slot: 1,
                api_version: None
            },
            value: true
        });

        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetLatestBlockhash,
            get_latest_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::default()
                .get_blockhash(&rpc_client, CommitmentConfig::default())
                .await
                .unwrap(),
            rpc_blockhash,
        );

        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetLatestBlockhash,
            get_latest_blockhash_response.clone(),
        );
        mocks.insert(
            RpcRequest::IsBlockhashValid,
            is_blockhash_valid_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::Validated(Source::Cluster, test_blockhash)
                .get_blockhash(&rpc_client, CommitmentConfig::default())
                .await
                .unwrap(),
            test_blockhash,
        );

        let mut mocks = HashMap::new();
        mocks.insert(
            RpcRequest::GetLatestBlockhash,
            get_latest_blockhash_response.clone(),
        );
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::Static(test_blockhash)
                .get_blockhash(&rpc_client, CommitmentConfig::default())
                .await
                .unwrap(),
            test_blockhash,
        );

        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::default()
            .get_blockhash(&rpc_client, CommitmentConfig::default())
            .await
            .is_err());

        let durable_nonce = DurableNonce::from_blockhash(&Hash::new(&[2u8; 32]));
        let nonce_blockhash = *durable_nonce.as_hash();
        let nonce_fee_calc = FeeCalculator::new(4242);
        let data = nonce::state::Data {
            authority: Pubkey::new(&[3u8; 32]),
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
        let nonce_pubkey = Pubkey::new(&[4u8; 32]);
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
            BlockhashQuery::Rpc(Source::NonceAccount(nonce_pubkey))
                .get_blockhash(&rpc_client, CommitmentConfig::default())
                .await
                .unwrap(),
            nonce_blockhash,
        );

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::Validated(Source::NonceAccount(nonce_pubkey), nonce_blockhash)
                .get_blockhash(&rpc_client, CommitmentConfig::default())
                .await
                .unwrap(),
            nonce_blockhash,
        );

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetAccountInfo, get_account_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            BlockhashQuery::Static(nonce_blockhash)
                .get_blockhash(&rpc_client, CommitmentConfig::default())
                .await
                .unwrap(),
            nonce_blockhash,
        );

        let rpc_client = RpcClient::new_mock("fails".to_string());
        assert!(BlockhashQuery::Rpc(Source::NonceAccount(nonce_pubkey))
            .get_blockhash(&rpc_client, CommitmentConfig::default())
            .await
            .is_err());
    }
}
