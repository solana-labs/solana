//! Communication with a Solana node over RPC.
//!
//! Software that interacts with the Solana blockchain, whether querying its
//! state or submitting transactions, communicates with a Solana node over
//! [JSON-RPC], using the [`RpcClient`] type.
//!
//! [JSON-RPC]: https://www.jsonrpc.org/specification
//!
//! This is a blocking API. For a non-blocking API use the asynchronous client
//! in [`crate::nonblocking::rpc_client`].

pub use crate::mock_sender::Mocks;
#[allow(deprecated)]
use solana_rpc_client_api::deprecated_config::{
    RpcConfirmedBlockConfig, RpcConfirmedTransactionConfig,
};
use {
    crate::{
        http_sender::HttpSender,
        mock_sender::MockSender,
        nonblocking::{self, rpc_client::get_rpc_request_str},
        rpc_sender::*,
    },
    serde::Serialize,
    serde_json::Value,
    solana_account_decoder::{
        parse_token::{UiTokenAccount, UiTokenAmount},
        UiAccount, UiAccountEncoding,
    },
    solana_rpc_client_api::{
        client_error::{Error as ClientError, ErrorKind, Result as ClientResult},
        config::{RpcAccountInfoConfig, *},
        request::{RpcRequest, TokenAccountsFilter},
        response::*,
    },
    solana_sdk::{
        account::{Account, ReadableAccount},
        clock::{Epoch, Slot, UnixTimestamp},
        commitment_config::CommitmentConfig,
        epoch_info::EpochInfo,
        epoch_schedule::EpochSchedule,
        feature::Feature,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        hash::Hash,
        message::{v0, Message as LegacyMessage},
        pubkey::Pubkey,
        signature::Signature,
        transaction::{self, uses_durable_nonce, Transaction, VersionedTransaction},
    },
    solana_transaction_status::{
        EncodedConfirmedBlock, EncodedConfirmedTransactionWithStatusMeta, TransactionStatus,
        UiConfirmedBlock, UiTransactionEncoding,
    },
    std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration},
};

#[derive(Default)]
pub struct RpcClientConfig {
    pub commitment_config: CommitmentConfig,
    pub confirm_transaction_initial_timeout: Option<Duration>,
}

impl RpcClientConfig {
    pub fn with_commitment(commitment_config: CommitmentConfig) -> Self {
        RpcClientConfig {
            commitment_config,
            ..Self::default()
        }
    }
}

/// Trait used to add support for versioned messages to RPC APIs while
/// retaining backwards compatibility
pub trait SerializableMessage: Serialize {}
impl SerializableMessage for LegacyMessage {}
impl SerializableMessage for v0::Message {}

/// Trait used to add support for versioned transactions to RPC APIs while
/// retaining backwards compatibility
pub trait SerializableTransaction: Serialize {
    fn get_signature(&self) -> &Signature;
    fn get_recent_blockhash(&self) -> &Hash;
    fn uses_durable_nonce(&self) -> bool;
}
impl SerializableTransaction for Transaction {
    fn get_signature(&self) -> &Signature {
        &self.signatures[0]
    }
    fn get_recent_blockhash(&self) -> &Hash {
        &self.message.recent_blockhash
    }
    fn uses_durable_nonce(&self) -> bool {
        uses_durable_nonce(self).is_some()
    }
}
impl SerializableTransaction for VersionedTransaction {
    fn get_signature(&self) -> &Signature {
        &self.signatures[0]
    }
    fn get_recent_blockhash(&self) -> &Hash {
        self.message.recent_blockhash()
    }
    fn uses_durable_nonce(&self) -> bool {
        self.uses_durable_nonce()
    }
}

#[derive(Debug, Default)]
pub struct GetConfirmedSignaturesForAddress2Config {
    pub before: Option<Signature>,
    pub until: Option<Signature>,
    pub limit: Option<usize>,
    pub commitment: Option<CommitmentConfig>,
}

/// A client of a remote Solana node.
///
/// `RpcClient` communicates with a Solana node over [JSON-RPC], with the
/// [Solana JSON-RPC protocol][jsonprot]. It is the primary Rust interface for
/// querying and transacting with the network from external programs.
///
/// This type builds on the underlying RPC protocol, adding extra features such
/// as timeout handling, retries, and waiting on transaction [commitment levels][cl].
/// Some methods simply pass through to the underlying RPC protocol. Not all RPC
/// methods are encapsulated by this type, but `RpcClient` does expose a generic
/// [`send`](RpcClient::send) method for making any [`RpcRequest`].
///
/// The documentation for most `RpcClient` methods contains an "RPC Reference"
/// section that links to the documentation for the underlying JSON-RPC method.
/// The documentation for `RpcClient` does not reproduce the documentation for
/// the underlying JSON-RPC methods. Thus reading both is necessary for complete
/// understanding.
///
/// `RpcClient`s generally communicate over HTTP on port 8899, a typical server
/// URL being "http://localhost:8899".
///
/// Methods that query information from recent [slots], including those that
/// confirm transactions, decide the most recent slot to query based on a
/// [commitment level][cl], which determines how committed or finalized a slot
/// must be to be considered for the query. Unless specified otherwise, the
/// commitment level is [`Finalized`], meaning the slot is definitely
/// permanently committed. The default commitment level can be configured by
/// creating `RpcClient` with an explicit [`CommitmentConfig`], and that default
/// configured commitment level can be overridden by calling the various
/// `_with_commitment` methods, like
/// [`RpcClient::confirm_transaction_with_commitment`]. In some cases the
/// configured commitment level is ignored and `Finalized` is used instead, as
/// in [`RpcClient::get_blocks`], where it would be invalid to use the
/// [`Processed`] commitment level. These exceptions are noted in the method
/// documentation.
///
/// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
/// [`Processed`]: solana_sdk::commitment_config::CommitmentLevel::Processed
/// [jsonprot]: https://docs.solana.com/developing/clients/jsonrpc-api
/// [JSON-RPC]: https://www.jsonrpc.org/specification
/// [slots]: https://docs.solana.com/terminology#slot
/// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
///
/// # Errors
///
/// Methods on `RpcClient` return
/// [`client_error::Result`][solana_rpc_client_api::client_error::Result], and many of them
/// return the [`RpcResult`][solana_rpc_client_api::response::RpcResult] typedef, which
/// contains [`Response<T>`][solana_rpc_client_api::response::Response] on `Ok`. Both
/// `client_error::Result` and [`RpcResult`] contain `ClientError` on error. In
/// the case of `RpcResult`, the actual return value is in the
/// [`value`][solana_rpc_client_api::response::Response::value] field, with RPC contextual
/// information in the [`context`][solana_rpc_client_api::response::Response::context]
/// field, so it is common for the value to be accessed with `?.value`, as in
///
/// ```
/// # use solana_sdk::system_transaction;
/// # use solana_rpc_client_api::client_error::Error;
/// # use solana_rpc_client::rpc_client::RpcClient;
/// # use solana_sdk::signature::{Keypair, Signer};
/// # use solana_sdk::hash::Hash;
/// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
/// # let key = Keypair::new();
/// # let to = solana_sdk::pubkey::new_rand();
/// # let lamports = 50;
/// # let latest_blockhash = Hash::default();
/// # let tx = system_transaction::transfer(&key, &to, lamports, latest_blockhash);
/// let signature = rpc_client.send_transaction(&tx)?;
/// let statuses = rpc_client.get_signature_statuses(&[signature])?.value;
/// # Ok::<(), Error>(())
/// ```
///
/// Requests may timeout, in which case they return a [`ClientError`] where the
/// [`ClientErrorKind`] is [`ClientErrorKind::Reqwest`], and where the interior
/// [`reqwest::Error`](solana_rpc_client_api::client_error::reqwest::Error)s
/// [`is_timeout`](solana_rpc_client_api::client_error::reqwest::Error::is_timeout) method
/// returns `true`. The default timeout is 30 seconds, and may be changed by
/// calling an appropriate constructor with a `timeout` parameter.
///
/// [`ClientError`]: solana_rpc_client_api::client_error::Error
/// [`ClientErrorKind`]: solana_rpc_client_api::client_error::ErrorKind
/// [`ClientErrorKind::Reqwest`]: solana_rpc_client_api::client_error::ErrorKind::Reqwest
pub struct RpcClient {
    rpc_client: Arc<nonblocking::rpc_client::RpcClient>,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.runtime.take().expect("runtime").shutdown_background();
    }
}

impl RpcClient {
    /// Create an `RpcClient` from an [`RpcSender`] and an [`RpcClientConfig`].
    ///
    /// This is the basic constructor, allowing construction with any type of
    /// `RpcSender`. Most applications should use one of the other constructors,
    /// such as [`RpcClient::new`], [`RpcClient::new_with_commitment`] or
    /// [`RpcClient::new_with_timeout`].
    pub fn new_sender<T: RpcSender + Send + Sync + 'static>(
        sender: T,
        config: RpcClientConfig,
    ) -> Self {
        Self {
            rpc_client: Arc::new(nonblocking::rpc_client::RpcClient::new_sender(
                sender, config,
            )),
            runtime: Some(
                tokio::runtime::Builder::new_current_thread()
                    .thread_name("solRpcClient")
                    .enable_io()
                    .enable_time()
                    .build()
                    .unwrap(),
            ),
        }
    }

    /// Create an HTTP `RpcClient`.
    ///
    /// The URL is an HTTP URL, usually for port 8899, as in
    /// "http://localhost:8899".
    ///
    /// The client has a default timeout of 30 seconds, and a default [commitment
    /// level][cl] of [`Finalized`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// let url = "http://localhost:8899".to_string();
    /// let client = RpcClient::new(url);
    /// ```
    pub fn new<U: ToString>(url: U) -> Self {
        Self::new_with_commitment(url, CommitmentConfig::default())
    }

    /// Create an HTTP `RpcClient` with specified [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// The URL is an HTTP URL, usually for port 8899, as in
    /// "http://localhost:8899".
    ///
    /// The client has a default timeout of 30 seconds, and a user-specified
    /// [`CommitmentLevel`] via [`CommitmentConfig`].
    ///
    /// [`CommitmentLevel`]: solana_sdk::commitment_config::CommitmentLevel
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// let url = "http://localhost:8899".to_string();
    /// let commitment_config = CommitmentConfig::processed();
    /// let client = RpcClient::new_with_commitment(url, commitment_config);
    /// ```
    pub fn new_with_commitment<U: ToString>(url: U, commitment_config: CommitmentConfig) -> Self {
        Self::new_sender(
            HttpSender::new(url),
            RpcClientConfig::with_commitment(commitment_config),
        )
    }

    /// Create an HTTP `RpcClient` with specified timeout.
    ///
    /// The URL is an HTTP URL, usually for port 8899, as in
    /// "http://localhost:8899".
    ///
    /// The client has and a default [commitment level][cl] of
    /// [`Finalized`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// let url = "http://localhost::8899".to_string();
    /// let timeout = Duration::from_secs(1);
    /// let client = RpcClient::new_with_timeout(url, timeout);
    /// ```
    pub fn new_with_timeout<U: ToString>(url: U, timeout: Duration) -> Self {
        Self::new_sender(
            HttpSender::new_with_timeout(url, timeout),
            RpcClientConfig::with_commitment(CommitmentConfig::default()),
        )
    }

    /// Create an HTTP `RpcClient` with specified timeout and [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// The URL is an HTTP URL, usually for port 8899, as in
    /// "http://localhost:8899".
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// let url = "http://localhost::8899".to_string();
    /// let timeout = Duration::from_secs(1);
    /// let commitment_config = CommitmentConfig::processed();
    /// let client = RpcClient::new_with_timeout_and_commitment(
    ///     url,
    ///     timeout,
    ///     commitment_config,
    /// );
    /// ```
    pub fn new_with_timeout_and_commitment<U: ToString>(
        url: U,
        timeout: Duration,
        commitment_config: CommitmentConfig,
    ) -> Self {
        Self::new_sender(
            HttpSender::new_with_timeout(url, timeout),
            RpcClientConfig::with_commitment(commitment_config),
        )
    }

    /// Create an HTTP `RpcClient` with specified timeout and [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// The URL is an HTTP URL, usually for port 8899, as in
    /// "http://localhost:8899".
    ///
    /// The `confirm_transaction_initial_timeout` argument specifies the amount of
    /// time to allow for the server to initially process a transaction, when
    /// confirming a transaction via one of the `_with_spinner` methods, like
    /// [`RpcClient::send_and_confirm_transaction_with_spinner`]. In
    /// other words, setting `confirm_transaction_initial_timeout` to > 0 allows
    /// `RpcClient` to wait for confirmation of a transaction that the server
    /// has not "seen" yet.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::time::Duration;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// let url = "http://localhost::8899".to_string();
    /// let timeout = Duration::from_secs(1);
    /// let commitment_config = CommitmentConfig::processed();
    /// let confirm_transaction_initial_timeout = Duration::from_secs(10);
    /// let client = RpcClient::new_with_timeouts_and_commitment(
    ///     url,
    ///     timeout,
    ///     commitment_config,
    ///     confirm_transaction_initial_timeout,
    /// );
    /// ```
    pub fn new_with_timeouts_and_commitment<U: ToString>(
        url: U,
        timeout: Duration,
        commitment_config: CommitmentConfig,
        confirm_transaction_initial_timeout: Duration,
    ) -> Self {
        Self::new_sender(
            HttpSender::new_with_timeout(url, timeout),
            RpcClientConfig {
                commitment_config,
                confirm_transaction_initial_timeout: Some(confirm_transaction_initial_timeout),
            },
        )
    }

    /// Create a mock `RpcClient`.
    ///
    /// A mock `RpcClient` contains an implementation of [`RpcSender`] that does
    /// not use the network, and instead returns synthetic responses, for use in
    /// tests.
    ///
    /// It is primarily for internal use, with limited customizability, and
    /// behaviors determined by internal Solana test cases. New users should
    /// consider implementing `RpcSender` themselves and constructing
    /// `RpcClient` with [`RpcClient::new_sender`] to get mock behavior.
    ///
    /// Unless directed otherwise, a mock `RpcClient` will generally return a
    /// reasonable default response to any request, at least for [`RpcRequest`]
    /// values for which responses have been implemented.
    ///
    /// This mock can be customized by changing the `url` argument, which is not
    /// actually a URL, but a simple string directive that changes the mock
    /// behavior in specific scenarios:
    ///
    /// - It is customary to set the `url` to "succeeds" for mocks that should
    ///   return successfully, though this value is not actually interpreted.
    ///
    /// - If `url` is "fails" then any call to `send` will return `Ok(Value::Null)`.
    ///
    /// - Other possible values of `url` are specific to different `RpcRequest`
    ///   values. Read the implementation of (non-public) `MockSender` for
    ///   details.
    ///
    /// The [`RpcClient::new_mock_with_mocks`] function offers further
    /// customization options.
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// // Create an `RpcClient` that always succeeds
    /// let url = "succeeds".to_string();
    /// let successful_client = RpcClient::new_mock(url);
    /// ```
    ///
    /// ```
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// // Create an `RpcClient` that always fails
    /// let url = "fails".to_string();
    /// let successful_client = RpcClient::new_mock(url);
    /// ```
    pub fn new_mock<U: ToString>(url: U) -> Self {
        Self::new_sender(
            MockSender::new(url),
            RpcClientConfig::with_commitment(CommitmentConfig::default()),
        )
    }

    /// Create a mock `RpcClient`.
    ///
    /// A mock `RpcClient` contains an implementation of [`RpcSender`] that does
    /// not use the network, and instead returns synthetic responses, for use in
    /// tests.
    ///
    /// It is primarily for internal use, with limited customizability, and
    /// behaviors determined by internal Solana test cases. New users should
    /// consider implementing `RpcSender` themselves and constructing
    /// `RpcClient` with [`RpcClient::new_sender`] to get mock behavior.
    ///
    /// Unless directed otherwise, a mock `RpcClient` will generally return a
    /// reasonable default response to any request, at least for [`RpcRequest`]
    /// values for which responses have been implemented.
    ///
    /// This mock can be customized in two ways:
    ///
    /// 1) By changing the `url` argument, which is not actually a URL, but a
    ///    simple string directive that changes the mock behavior in specific
    ///    scenarios.
    ///
    ///    It is customary to set the `url` to "succeeds" for mocks that should
    ///    return successfully, though this value is not actually interpreted.
    ///
    ///    If `url` is "fails" then any call to `send` will return `Ok(Value::Null)`.
    ///
    ///    Other possible values of `url` are specific to different `RpcRequest`
    ///    values. Read the implementation of `MockSender` (which is non-public)
    ///    for details.
    ///
    /// 2) Custom responses can be configured by providing [`Mocks`]. This type
    ///    is a [`HashMap`] from [`RpcRequest`] to a JSON [`Value`] response,
    ///    Any entries in this map override the default behavior for the given
    ///    request.
    ///
    /// The [`RpcClient::new_mock_with_mocks`] function offers further
    /// customization options.
    ///
    /// [`HashMap`]: std::collections::HashMap
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     request::RpcRequest,
    /// #     response::{Response, RpcResponseContext},
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use std::collections::HashMap;
    /// # use serde_json::json;
    /// // Create a mock with a custom repsonse to the `GetBalance` request
    /// let account_balance = 50;
    /// let account_balance_response = json!(Response {
    ///     context: RpcResponseContext { slot: 1, api_version: None },
    ///     value: json!(account_balance),
    /// });
    ///
    /// let mut mocks = HashMap::new();
    /// mocks.insert(RpcRequest::GetBalance, account_balance_response);
    /// let url = "succeeds".to_string();
    /// let client = RpcClient::new_mock_with_mocks(url, mocks);
    /// ```
    pub fn new_mock_with_mocks<U: ToString>(url: U, mocks: Mocks) -> Self {
        Self::new_sender(
            MockSender::new_with_mocks(url, mocks),
            RpcClientConfig::with_commitment(CommitmentConfig::default()),
        )
    }

    /// Create an HTTP `RpcClient` from a [`SocketAddr`].
    ///
    /// The client has a default timeout of 30 seconds, and a default [commitment
    /// level][cl] of [`Finalized`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::net::{Ipv4Addr, SocketAddr};
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 8899));
    /// let client = RpcClient::new_socket(addr);
    /// ```
    pub fn new_socket(addr: SocketAddr) -> Self {
        Self::new(get_rpc_request_str(addr, false))
    }

    /// Create an HTTP `RpcClient` from a [`SocketAddr`] with specified [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// The client has a default timeout of 30 seconds, and a user-specified
    /// [`CommitmentLevel`] via [`CommitmentConfig`].
    ///
    /// [`CommitmentLevel`]: solana_sdk::commitment_config::CommitmentLevel
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::net::{Ipv4Addr, SocketAddr};
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 8899));
    /// let commitment_config = CommitmentConfig::processed();
    /// let client = RpcClient::new_socket_with_commitment(
    ///     addr,
    ///     commitment_config
    /// );
    /// ```
    pub fn new_socket_with_commitment(
        addr: SocketAddr,
        commitment_config: CommitmentConfig,
    ) -> Self {
        Self::new_with_commitment(get_rpc_request_str(addr, false), commitment_config)
    }

    /// Create an HTTP `RpcClient` from a [`SocketAddr`] with specified timeout.
    ///
    /// The client has a default [commitment level][cl] of [`Finalized`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::net::{Ipv4Addr, SocketAddr};
    /// # use std::time::Duration;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, 8899));
    /// let timeout = Duration::from_secs(1);
    /// let client = RpcClient::new_socket_with_timeout(addr, timeout);
    /// ```
    pub fn new_socket_with_timeout(addr: SocketAddr, timeout: Duration) -> Self {
        let url = get_rpc_request_str(addr, false);
        Self::new_with_timeout(url, timeout)
    }

    /// Get the configured url of the client's sender
    pub fn url(&self) -> String {
        (self.rpc_client.as_ref()).url()
    }

    /// Get the configured default [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// The commitment config may be specified during construction, and
    /// determines how thoroughly committed a transaction must be when waiting
    /// for its confirmation or otherwise checking for confirmation. If not
    /// specified, the default commitment level is
    /// [`Finalized`].
    ///
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    ///
    /// The default commitment level is overridden when calling methods that
    /// explicitly provide a [`CommitmentConfig`], like
    /// [`RpcClient::confirm_transaction_with_commitment`].
    pub fn commitment(&self) -> CommitmentConfig {
        (self.rpc_client.as_ref()).commitment()
    }

    /// Submit a transaction and wait for confirmation.
    ///
    /// Once this function returns successfully, the given transaction is
    /// guaranteed to be processed with the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// After sending the transaction, this method polls in a loop for the
    /// status of the transaction until it has ben confirmed.
    ///
    /// # Errors
    ///
    /// If the transaction is not signed then an error with kind [`RpcError`] is
    /// returned, containing an [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`].
    ///
    /// If the preflight transaction simulation fails then an error with kind
    /// [`RpcError`] is returned, containing an [`RpcResponseError`] with `code`
    /// set to [`JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE`].
    ///
    /// If the receiving node is unhealthy, e.g. it is not fully synced to
    /// the cluster, then an error with kind [`RpcError`] is returned,
    /// containing an [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY`].
    ///
    /// [`RpcError`]: solana_rpc_client_api::request::RpcError
    /// [`RpcResponseError`]: solana_rpc_client_api::request::RpcError::RpcResponseError
    /// [`JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE
    /// [`JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE
    /// [`JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`sendTransaction`] RPC method, and the
    /// [`getLatestBlockhash`] RPC method.
    ///
    /// [`sendTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#sendtransaction
    /// [`getLatestBlockhash`]: https://docs.solana.com/developing/clients/jsonrpc-api#getlatestblockhash
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn send_and_confirm_transaction(
        &self,
        transaction: &impl SerializableTransaction,
    ) -> ClientResult<Signature> {
        self.invoke((self.rpc_client.as_ref()).send_and_confirm_transaction(transaction))
    }

    #[cfg(feature = "spinner")]
    pub fn send_and_confirm_transaction_with_spinner(
        &self,
        transaction: &impl SerializableTransaction,
    ) -> ClientResult<Signature> {
        self.invoke(
            (self.rpc_client.as_ref()).send_and_confirm_transaction_with_spinner(transaction),
        )
    }

    #[cfg(feature = "spinner")]
    pub fn send_and_confirm_transaction_with_spinner_and_commitment(
        &self,
        transaction: &impl SerializableTransaction,
        commitment: CommitmentConfig,
    ) -> ClientResult<Signature> {
        self.invoke(
            (self.rpc_client.as_ref())
                .send_and_confirm_transaction_with_spinner_and_commitment(transaction, commitment),
        )
    }

    #[cfg(feature = "spinner")]
    pub fn send_and_confirm_transaction_with_spinner_and_config(
        &self,
        transaction: &impl SerializableTransaction,
        commitment: CommitmentConfig,
        config: RpcSendTransactionConfig,
    ) -> ClientResult<Signature> {
        self.invoke(
            (self.rpc_client.as_ref()).send_and_confirm_transaction_with_spinner_and_config(
                transaction,
                commitment,
                config,
            ),
        )
    }

    /// Submits a signed transaction to the network.
    ///
    /// Before a transaction is processed, the receiving node runs a "preflight
    /// check" which verifies signatures, checks that the node is healthy,
    /// and simulates the transaction. If the preflight check fails then an
    /// error is returned immediately. Preflight checks can be disabled by
    /// calling [`send_transaction_with_config`] and setting the
    /// [`skip_preflight`] field of [`RpcSendTransactionConfig`] to `true`.
    ///
    /// This method does not wait for the transaction to be processed or
    /// confirmed before returning successfully. To wait for the transaction to
    /// be processed or confirmed, use the [`send_and_confirm_transaction`]
    /// method.
    ///
    /// [`send_transaction_with_config`]: RpcClient::send_transaction_with_config
    /// [`skip_preflight`]: solana_rpc_client_api::config::RpcSendTransactionConfig::skip_preflight
    /// [`RpcSendTransactionConfig`]: solana_rpc_client_api::config::RpcSendTransactionConfig
    /// [`send_and_confirm_transaction`]: RpcClient::send_and_confirm_transaction
    ///
    /// # Errors
    ///
    /// If the transaction is not signed then an error with kind [`RpcError`] is
    /// returned, containing an [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`].
    ///
    /// If the preflight transaction simulation fails then an error with kind
    /// [`RpcError`] is returned, containing an [`RpcResponseError`] with `code`
    /// set to [`JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE`].
    ///
    /// If the receiving node is unhealthy, e.g. it is not fully synced to
    /// the cluster, then an error with kind [`RpcError`] is returned,
    /// containing an [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY`].
    ///
    /// [`RpcError`]: solana_rpc_client_api::request::RpcError
    /// [`RpcResponseError`]: solana_rpc_client_api::request::RpcError::RpcResponseError
    /// [`JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE
    /// [`JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE
    /// [`JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`sendTransaction`] RPC method.
    ///
    /// [`sendTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#sendtransaction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Transfer lamports from Alice to Bob
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_transaction(&tx)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn send_transaction(
        &self,
        transaction: &impl SerializableTransaction,
    ) -> ClientResult<Signature> {
        self.invoke((self.rpc_client.as_ref()).send_transaction(transaction))
    }

    /// Submits a signed transaction to the network.
    ///
    /// Before a transaction is processed, the receiving node runs a "preflight
    /// check" which verifies signatures, checks that the node is healthy, and
    /// simulates the transaction. If the preflight check fails then an error is
    /// returned immediately. Preflight checks can be disabled by setting the
    /// [`skip_preflight`] field of [`RpcSendTransactionConfig`] to `true`.
    ///
    /// This method does not wait for the transaction to be processed or
    /// confirmed before returning successfully. To wait for the transaction to
    /// be processed or confirmed, use the [`send_and_confirm_transaction`]
    /// method.
    ///
    /// [`send_transaction_with_config`]: RpcClient::send_transaction_with_config
    /// [`skip_preflight`]: solana_rpc_client_api::config::RpcSendTransactionConfig::skip_preflight
    /// [`RpcSendTransactionConfig`]: solana_rpc_client_api::config::RpcSendTransactionConfig
    /// [`send_and_confirm_transaction`]: RpcClient::send_and_confirm_transaction
    ///
    /// # Errors
    ///
    /// If preflight checks are enabled, if the transaction is not signed
    /// then an error with kind [`RpcError`] is returned, containing an
    /// [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`].
    ///
    /// If preflight checks are enabled, if the preflight transaction simulation
    /// fails then an error with kind [`RpcError`] is returned, containing an
    /// [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE`].
    ///
    /// If the receiving node is unhealthy, e.g. it is not fully synced to
    /// the cluster, then an error with kind [`RpcError`] is returned,
    /// containing an [`RpcResponseError`] with `code` set to
    /// [`JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY`].
    ///
    /// [`RpcError`]: solana_rpc_client_api::request::RpcError
    /// [`RpcResponseError`]: solana_rpc_client_api::request::RpcError::RpcResponseError
    /// [`JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_TRANSACTION_SIGNATURE_VERIFICATION_FAILURE
    /// [`JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_SEND_TRANSACTION_PREFLIGHT_FAILURE
    /// [`JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY`]: solana_rpc_client_api::custom_error::JSON_RPC_SERVER_ERROR_NODE_UNHEALTHY
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`sendTransaction`] RPC method.
    ///
    /// [`sendTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#sendtransaction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::RpcSendTransactionConfig,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Transfer lamports from Alice to Bob
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let config = RpcSendTransactionConfig {
    ///     skip_preflight: true,
    ///     .. RpcSendTransactionConfig::default()
    /// };
    /// let signature = rpc_client.send_transaction_with_config(
    ///     &tx,
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn send_transaction_with_config(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSendTransactionConfig,
    ) -> ClientResult<Signature> {
        self.invoke((self.rpc_client.as_ref()).send_transaction_with_config(transaction, config))
    }

    pub fn send<T>(&self, request: RpcRequest, params: Value) -> ClientResult<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.invoke((self.rpc_client.as_ref()).send(request, params))
    }

    /// Check the confirmation status of a transaction.
    ///
    /// Returns `true` if the given transaction succeeded and has been committed
    /// with the configured [commitment level][cl], which can be retrieved with
    /// the [`commitment`](RpcClient::commitment) method.
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// Note that this method does not wait for a transaction to be confirmed
    /// &mdash; it only checks whether a transaction has been confirmed. To
    /// submit a transaction and wait for it to confirm, use
    /// [`send_and_confirm_transaction`][RpcClient::send_and_confirm_transaction].
    ///
    /// _This method returns `false` if the transaction failed, even if it has
    /// been confirmed._
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getSignatureStatuses`] RPC method.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Transfer lamports from Alice to Bob and wait for confirmation
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_transaction(&tx)?;
    ///
    /// loop {
    ///     let confirmed = rpc_client.confirm_transaction(&signature)?;
    ///     if confirmed {
    ///         break;
    ///     }
    /// }
    /// # Ok::<(), Error>(())
    /// ```
    pub fn confirm_transaction(&self, signature: &Signature) -> ClientResult<bool> {
        self.invoke((self.rpc_client.as_ref()).confirm_transaction(signature))
    }

    /// Check the confirmation status of a transaction.
    ///
    /// Returns an [`RpcResult`] with value `true` if the given transaction
    /// succeeded and has been committed with the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// Note that this method does not wait for a transaction to be confirmed
    /// &mdash; it only checks whether a transaction has been confirmed. To
    /// submit a transaction and wait for it to confirm, use
    /// [`send_and_confirm_transaction`][RpcClient::send_and_confirm_transaction].
    ///
    /// _This method returns an [`RpcResult`] with value `false` if the
    /// transaction failed, even if it has been confirmed._
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getSignatureStatuses`] RPC method.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     commitment_config::CommitmentConfig,
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # use std::time::Duration;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Transfer lamports from Alice to Bob and wait for confirmation
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_transaction(&tx)?;
    ///
    /// loop {
    ///     let commitment_config = CommitmentConfig::processed();
    ///     let confirmed = rpc_client.confirm_transaction_with_commitment(&signature, commitment_config)?;
    ///     if confirmed.value {
    ///         break;
    ///     }
    /// }
    /// # Ok::<(), Error>(())
    /// ```
    pub fn confirm_transaction_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<bool> {
        self.invoke(
            (self.rpc_client.as_ref())
                .confirm_transaction_with_commitment(signature, commitment_config),
        )
    }

    #[cfg(feature = "spinner")]
    pub fn confirm_transaction_with_spinner(
        &self,
        signature: &Signature,
        recent_blockhash: &Hash,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<()> {
        self.invoke((self.rpc_client.as_ref()).confirm_transaction_with_spinner(
            signature,
            recent_blockhash,
            commitment_config,
        ))
    }

    /// Simulates sending a transaction.
    ///
    /// If the transaction fails, then the [`err`] field of the returned
    /// [`RpcSimulateTransactionResult`] will be `Some`. Any logs emitted from
    /// the transaction are returned in the [`logs`] field.
    ///
    /// [`err`]: solana_rpc_client_api::response::RpcSimulateTransactionResult::err
    /// [`logs`]: solana_rpc_client_api::response::RpcSimulateTransactionResult::logs
    ///
    /// Simulating a transaction is similar to the ["preflight check"] that is
    /// run by default when sending a transaction.
    ///
    /// ["preflight check"]: https://docs.solana.com/developing/clients/jsonrpc-api#sendtransaction
    ///
    /// By default, signatures are not verified during simulation. To verify
    /// signatures, call the [`simulate_transaction_with_config`] method, with
    /// the [`sig_verify`] field of [`RpcSimulateTransactionConfig`] set to
    /// `true`.
    ///
    /// [`simulate_transaction_with_config`]: RpcClient::simulate_transaction_with_config
    /// [`sig_verify`]: solana_rpc_client_api::config::RpcSimulateTransactionConfig::sig_verify
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`simulateTransaction`] RPC method.
    ///
    /// [`simulateTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#simulatetransaction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     response::RpcSimulateTransactionResult,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Transfer lamports from Alice to Bob
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let result = rpc_client.simulate_transaction(&tx)?;
    /// assert!(result.value.err.is_none());
    /// # Ok::<(), Error>(())
    /// ```
    pub fn simulate_transaction(
        &self,
        transaction: &impl SerializableTransaction,
    ) -> RpcResult<RpcSimulateTransactionResult> {
        self.invoke((self.rpc_client.as_ref()).simulate_transaction(transaction))
    }

    /// Simulates sending a transaction.
    ///
    /// If the transaction fails, then the [`err`] field of the returned
    /// [`RpcSimulateTransactionResult`] will be `Some`. Any logs emitted from
    /// the transaction are returned in the [`logs`] field.
    ///
    /// [`err`]: solana_rpc_client_api::response::RpcSimulateTransactionResult::err
    /// [`logs`]: solana_rpc_client_api::response::RpcSimulateTransactionResult::logs
    ///
    /// Simulating a transaction is similar to the ["preflight check"] that is
    /// run by default when sending a transaction.
    ///
    /// ["preflight check"]: https://docs.solana.com/developing/clients/jsonrpc-api#sendtransaction
    ///
    /// By default, signatures are not verified during simulation. To verify
    /// signatures, call the [`simulate_transaction_with_config`] method, with
    /// the [`sig_verify`] field of [`RpcSimulateTransactionConfig`] set to
    /// `true`.
    ///
    /// [`simulate_transaction_with_config`]: RpcClient::simulate_transaction_with_config
    /// [`sig_verify`]: solana_rpc_client_api::config::RpcSimulateTransactionConfig::sig_verify
    ///
    /// This method can additionally query information about accounts by
    /// including them in the [`accounts`] field of the
    /// [`RpcSimulateTransactionConfig`] argument, in which case those results
    /// are reported in the [`accounts`][accounts2] field of the returned
    /// [`RpcSimulateTransactionResult`].
    ///
    /// [`accounts`]: solana_rpc_client_api::config::RpcSimulateTransactionConfig::accounts
    /// [accounts2]: solana_rpc_client_api::response::RpcSimulateTransactionResult::accounts
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`simulateTransaction`] RPC method.
    ///
    /// [`simulateTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#simulatetransaction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::RpcSimulateTransactionConfig,
    /// #     response::RpcSimulateTransactionResult,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Transfer lamports from Alice to Bob
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let config = RpcSimulateTransactionConfig {
    ///     sig_verify: true,
    ///     .. RpcSimulateTransactionConfig::default()
    /// };
    /// let result = rpc_client.simulate_transaction_with_config(
    ///     &tx,
    ///     config,
    /// )?;
    /// assert!(result.value.err.is_none());
    /// # Ok::<(), Error>(())
    /// ```
    pub fn simulate_transaction_with_config(
        &self,
        transaction: &impl SerializableTransaction,
        config: RpcSimulateTransactionConfig,
    ) -> RpcResult<RpcSimulateTransactionResult> {
        self.invoke(
            (self.rpc_client.as_ref()).simulate_transaction_with_config(transaction, config),
        )
    }

    /// Returns the highest slot information that the node has snapshots for.
    ///
    /// This will find the highest full snapshot slot, and the highest incremental snapshot slot
    /// _based on_ the full snapshot slot, if there is one.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getHighestSnapshotSlot`] RPC method.
    ///
    /// [`getHighestSnapshotSlot`]: https://docs.solana.com/developing/clients/jsonrpc-api#gethighestsnapshotslot
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let snapshot_slot_info = rpc_client.get_highest_snapshot_slot()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_highest_snapshot_slot(&self) -> ClientResult<RpcSnapshotSlotInfo> {
        self.invoke((self.rpc_client.as_ref()).get_highest_snapshot_slot())
    }

    #[deprecated(
        since = "1.8.0",
        note = "Please use RpcClient::get_highest_snapshot_slot() instead"
    )]
    #[allow(deprecated)]
    pub fn get_snapshot_slot(&self) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).get_snapshot_slot())
    }

    /// Check if a transaction has been processed with the default [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// If the transaction has been processed with the default commitment level,
    /// then this method returns `Ok` of `Some`. If the transaction has not yet
    /// been processed with the default commitment level, it returns `Ok` of
    /// `None`.
    ///
    /// If the transaction has been processed with the default commitment level,
    /// and the transaction succeeded, this method returns `Ok(Some(Ok(())))`.
    /// If the transaction has been processed with the default commitment level,
    /// and the transaction failed, this method returns `Ok(Some(Err(_)))`,
    /// where the interior error is type [`TransactionError`].
    ///
    /// [`TransactionError`]: solana_sdk::transaction::TransactionError
    ///
    /// This function only searches a node's recent history, including all
    /// recent slots, plus up to
    /// [`MAX_RECENT_BLOCKHASHES`][solana_sdk::clock::MAX_RECENT_BLOCKHASHES]
    /// rooted slots. To search the full transaction history use the
    /// [`get_signature_statuse_with_commitment_and_history`][RpcClient::get_signature_status_with_commitment_and_history]
    /// method.
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getSignatureStatuses`] RPC method.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#gitsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// # let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_transaction(&tx)?;
    /// let status = rpc_client.get_signature_status(&signature)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signature_status(
        &self,
        signature: &Signature,
    ) -> ClientResult<Option<transaction::Result<()>>> {
        self.invoke((self.rpc_client.as_ref()).get_signature_status(signature))
    }

    /// Gets the statuses of a list of transaction signatures.
    ///
    /// The returned vector of [`TransactionStatus`] has the same length as the
    /// input slice.
    ///
    /// For any transaction that has not been processed by the network, the
    /// value of the corresponding entry in the returned vector is `None`. As a
    /// result, a transaction that has recently been submitted will not have a
    /// status immediately.
    ///
    /// To submit a transaction and wait for it to confirm, use
    /// [`send_and_confirm_transaction`][RpcClient::send_and_confirm_transaction].
    ///
    /// This function ignores the configured confirmation level, and returns the
    /// transaction status whatever it is. It does not wait for transactions to
    /// be processed.
    ///
    /// This function only searches a node's recent history, including all
    /// recent slots, plus up to
    /// [`MAX_RECENT_BLOCKHASHES`][solana_sdk::clock::MAX_RECENT_BLOCKHASHES]
    /// rooted slots. To search the full transaction history use the
    /// [`get_signature_statuses_with_history`][RpcClient::get_signature_statuses_with_history]
    /// method.
    ///
    /// # Errors
    ///
    /// Any individual `TransactionStatus` may have triggered an error during
    /// processing, in which case its [`err`][`TransactionStatus::err`] field
    /// will be `Some`.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSignatureStatuses`] RPC method.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # use std::time::Duration;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// // Send lamports from Alice to Bob and wait for the transaction to be processed
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_transaction(&tx)?;
    ///
    /// let status = loop {
    ///    let statuses = rpc_client.get_signature_statuses(&[signature])?.value;
    ///    if let Some(status) = statuses[0].clone() {
    ///        break status;
    ///    }
    ///    std::thread::sleep(Duration::from_millis(100));
    /// };
    ///
    /// assert!(status.err.is_none());
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signature_statuses(
        &self,
        signatures: &[Signature],
    ) -> RpcResult<Vec<Option<TransactionStatus>>> {
        self.invoke((self.rpc_client.as_ref()).get_signature_statuses(signatures))
    }

    /// Gets the statuses of a list of transaction signatures.
    ///
    /// The returned vector of [`TransactionStatus`] has the same length as the
    /// input slice.
    ///
    /// For any transaction that has not been processed by the network, the
    /// value of the corresponding entry in the returned vector is `None`. As a
    /// result, a transaction that has recently been submitted will not have a
    /// status immediately.
    ///
    /// To submit a transaction and wait for it to confirm, use
    /// [`send_and_confirm_transaction`][RpcClient::send_and_confirm_transaction].
    ///
    /// This function ignores the configured confirmation level, and returns the
    /// transaction status whatever it is. It does not wait for transactions to
    /// be processed.
    ///
    /// This function searches a node's full ledger history and (if implemented) long-term storage. To search for
    /// transactions in recent slots only use the
    /// [`get_signature_statuses`][RpcClient::get_signature_statuses] method.
    ///
    /// # Errors
    ///
    /// Any individual `TransactionStatus` may have triggered an error during
    /// processing, in which case its [`err`][`TransactionStatus::err`] field
    /// will be `Some`.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSignatureStatuses`] RPC
    /// method, with the `searchTransactionHistory` configuration option set to
    /// `true`.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     hash::Hash,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # fn get_old_transaction_signature() -> Signature { Signature::default() }
    /// // Check if an old transaction exists
    /// let signature = get_old_transaction_signature();
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let statuses = rpc_client.get_signature_statuses_with_history(&[signature])?.value;
    /// if statuses[0].is_none() {
    ///     println!("old transaction does not exist");
    /// }
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signature_statuses_with_history(
        &self,
        signatures: &[Signature],
    ) -> RpcResult<Vec<Option<TransactionStatus>>> {
        self.invoke((self.rpc_client.as_ref()).get_signature_statuses_with_history(signatures))
    }

    /// Check if a transaction has been processed with the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// If the transaction has been processed with the given commitment level,
    /// then this method returns `Ok` of `Some`. If the transaction has not yet
    /// been processed with the given commitment level, it returns `Ok` of
    /// `None`.
    ///
    /// If the transaction has been processed with the given commitment level,
    /// and the transaction succeeded, this method returns `Ok(Some(Ok(())))`.
    /// If the transaction has been processed with the given commitment level,
    /// and the transaction failed, this method returns `Ok(Some(Err(_)))`,
    /// where the interior error is type [`TransactionError`].
    ///
    /// [`TransactionError`]: solana_sdk::transaction::TransactionError
    ///
    /// This function only searches a node's recent history, including all
    /// recent slots, plus up to
    /// [`MAX_RECENT_BLOCKHASHES`][solana_sdk::clock::MAX_RECENT_BLOCKHASHES]
    /// rooted slots. To search the full transaction history use the
    /// [`get_signature_statuse_with_commitment_and_history`][RpcClient::get_signature_status_with_commitment_and_history]
    /// method.
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getSignatureStatuses`] RPC method.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     commitment_config::CommitmentConfig,
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// # let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    /// let commitment_config = CommitmentConfig::processed();
    /// let status = rpc_client.get_signature_status_with_commitment(
    ///     &signature,
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signature_status_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Option<transaction::Result<()>>> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_signature_status_with_commitment(signature, commitment_config),
        )
    }

    /// Check if a transaction has been processed with the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// If the transaction has been processed with the given commitment level,
    /// then this method returns `Ok` of `Some`. If the transaction has not yet
    /// been processed with the given commitment level, it returns `Ok` of
    /// `None`.
    ///
    /// If the transaction has been processed with the given commitment level,
    /// and the transaction succeeded, this method returns `Ok(Some(Ok(())))`.
    /// If the transaction has been processed with the given commitment level,
    /// and the transaction failed, this method returns `Ok(Some(Err(_)))`,
    /// where the interior error is type [`TransactionError`].
    ///
    /// [`TransactionError`]: solana_sdk::transaction::TransactionError
    ///
    /// This method optionally searches a node's full ledger history and (if
    /// implemented) long-term storage.
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getSignatureStatuses`] RPC method.
    ///
    /// [`getSignatureStatuses`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturestatuses
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     commitment_config::CommitmentConfig,
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// # let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_transaction(&tx)?;
    /// let commitment_config = CommitmentConfig::processed();
    /// let search_transaction_history = true;
    /// let status = rpc_client.get_signature_status_with_commitment_and_history(
    ///     &signature,
    ///     commitment_config,
    ///     search_transaction_history,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signature_status_with_commitment_and_history(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
        search_transaction_history: bool,
    ) -> ClientResult<Option<transaction::Result<()>>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_signature_status_with_commitment_and_history(
                signature,
                commitment_config,
                search_transaction_history,
            ),
        )
    }

    /// Returns the slot that has reached the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSlot`] RPC method.
    ///
    /// [`getSlot`]: https://docs.solana.com/developing/clients/jsonrpc-api#getslot
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let slot = rpc_client.get_slot()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_slot(&self) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).get_slot())
    }

    /// Returns the slot that has reached the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSlot`] RPC method.
    ///
    /// [`getSlot`]: https://docs.solana.com/developing/clients/jsonrpc-api#getslot
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let commitment_config = CommitmentConfig::processed();
    /// let slot = rpc_client.get_slot_with_commitment(commitment_config)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).get_slot_with_commitment(commitment_config))
    }

    /// Returns the block height that has reached the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method is corresponds directly to the [`getBlockHeight`] RPC method.
    ///
    /// [`getBlockHeight`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblockheight
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let block_height = rpc_client.get_block_height()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_height(&self) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_block_height())
    }

    /// Returns the block height that has reached the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method is corresponds directly to the [`getBlockHeight`] RPC method.
    ///
    /// [`getBlockHeight`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblockheight
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let commitment_config = CommitmentConfig::processed();
    /// let block_height = rpc_client.get_block_height_with_commitment(
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_height_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_block_height_with_commitment(commitment_config))
    }

    /// Returns the slot leaders for a given slot range.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSlotLeaders`] RPC method.
    ///
    /// [`getSlotLeaders`]: https://docs.solana.com/developing/clients/jsonrpc-api#getslotleaders
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::slot_history::Slot;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let start_slot = 1;
    /// let limit = 3;
    /// let leaders = rpc_client.get_slot_leaders(start_slot, limit)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_slot_leaders(&self, start_slot: Slot, limit: u64) -> ClientResult<Vec<Pubkey>> {
        self.invoke((self.rpc_client.as_ref()).get_slot_leaders(start_slot, limit))
    }

    /// Get block production for the current epoch.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlockProduction`] RPC method.
    ///
    /// [`getBlockProduction`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblockproduction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let production = rpc_client.get_block_production()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_production(&self) -> RpcResult<RpcBlockProduction> {
        self.invoke((self.rpc_client.as_ref()).get_block_production())
    }

    /// Get block production for the current or previous epoch.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlockProduction`] RPC method.
    ///
    /// [`getBlockProduction`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblockproduction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::{RpcBlockProductionConfig, RpcBlockProductionConfigRange},
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let start_slot = 1;
    /// # let limit = 3;
    /// let leader = rpc_client.get_slot_leaders(start_slot, limit)?;
    /// let leader = leader[0];
    /// let range = RpcBlockProductionConfigRange {
    ///     first_slot: start_slot,
    ///     last_slot: Some(start_slot + limit),
    /// };
    /// let config = RpcBlockProductionConfig {
    ///     identity: Some(leader.to_string()),
    ///     range: Some(range),
    ///     commitment: Some(CommitmentConfig::processed()),
    /// };
    /// let production = rpc_client.get_block_production_with_config(
    ///     config
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_production_with_config(
        &self,
        config: RpcBlockProductionConfig,
    ) -> RpcResult<RpcBlockProduction> {
        self.invoke((self.rpc_client.as_ref()).get_block_production_with_config(config))
    }

    /// Returns epoch activation information for a stake account.
    ///
    /// This method uses the configured [commitment level].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getStakeActivation`] RPC method.
    ///
    /// [`getStakeActivation`]: https://docs.solana.com/developing/clients/jsonrpc-api#getstakeactivation
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     response::StakeActivationState,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signer::keypair::Keypair,
    /// #     signature::Signer,
    /// #     pubkey::Pubkey,
    /// #     stake,
    /// #     stake::state::{Authorized, Lockup},
    /// #     transaction::Transaction
    /// # };
    /// # use std::str::FromStr;
    /// # let alice = Keypair::new();
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Find some vote account to delegate to
    /// let vote_accounts = rpc_client.get_vote_accounts()?;
    /// let vote_account = vote_accounts.current.get(0).unwrap_or_else(|| &vote_accounts.delinquent[0]);
    /// let vote_account_pubkey = &vote_account.vote_pubkey;
    /// let vote_account_pubkey = Pubkey::from_str(vote_account_pubkey).expect("pubkey");
    ///
    /// // Create a stake account
    /// let stake_account = Keypair::new();
    /// let stake_account_pubkey = stake_account.pubkey();
    ///
    /// // Build the instructions to create new stake account,
    /// // funded by alice, and delegate to a validator's vote account.
    /// let instrs = stake::instruction::create_account_and_delegate_stake(
    ///     &alice.pubkey(),
    ///     &stake_account_pubkey,
    ///     &vote_account_pubkey,
    ///     &Authorized::auto(&stake_account_pubkey),
    ///     &Lockup::default(),
    ///     1_000_000,
    /// );
    ///
    /// let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// let tx = Transaction::new_signed_with_payer(
    ///     &instrs,
    ///     Some(&alice.pubkey()),
    ///     &[&alice, &stake_account],
    ///     latest_blockhash,
    /// );
    ///
    /// rpc_client.send_and_confirm_transaction(&tx)?;
    ///
    /// let epoch_info = rpc_client.get_epoch_info()?;
    /// let activation = rpc_client.get_stake_activation(
    ///     stake_account_pubkey,
    ///     Some(epoch_info.epoch),
    /// )?;
    ///
    /// assert_eq!(activation.state, StakeActivationState::Activating);
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_stake_activation(
        &self,
        stake_account: Pubkey,
        epoch: Option<Epoch>,
    ) -> ClientResult<RpcStakeActivation> {
        self.invoke((self.rpc_client.as_ref()).get_stake_activation(stake_account, epoch))
    }

    /// Returns information about the current supply.
    ///
    /// This method uses the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSupply`] RPC method.
    ///
    /// [`getSupply`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsupply
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let supply = rpc_client.supply()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn supply(&self) -> RpcResult<RpcSupply> {
        self.invoke((self.rpc_client.as_ref()).supply())
    }

    /// Returns information about the current supply.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSupply`] RPC method.
    ///
    /// [`getSupply`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsupply
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let commitment_config = CommitmentConfig::processed();
    /// let supply = rpc_client.supply_with_commitment(
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn supply_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<RpcSupply> {
        self.invoke((self.rpc_client.as_ref()).supply_with_commitment(commitment_config))
    }

    /// Returns the 20 largest accounts, by lamport balance.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getLargestAccounts`] RPC
    /// method.
    ///
    /// [`getLargestAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getlargestaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::{RpcLargestAccountsConfig, RpcLargestAccountsFilter},
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let commitment_config = CommitmentConfig::processed();
    /// let config = RpcLargestAccountsConfig {
    ///     commitment: Some(commitment_config),
    ///     filter: Some(RpcLargestAccountsFilter::Circulating),
    /// };
    /// let accounts = rpc_client.get_largest_accounts_with_config(
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_largest_accounts_with_config(
        &self,
        config: RpcLargestAccountsConfig,
    ) -> RpcResult<Vec<RpcAccountBalance>> {
        self.invoke((self.rpc_client.as_ref()).get_largest_accounts_with_config(config))
    }

    /// Returns the account info and associated stake for all the voting accounts
    /// that have reached the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getVoteAccounts`]
    /// RPC method.
    ///
    /// [`getVoteAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getvoteaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let accounts = rpc_client.get_vote_accounts()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_vote_accounts(&self) -> ClientResult<RpcVoteAccountStatus> {
        self.invoke((self.rpc_client.as_ref()).get_vote_accounts())
    }

    /// Returns the account info and associated stake for all the voting accounts
    /// that have reached the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getVoteAccounts`] RPC method.
    ///
    /// [`getVoteAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getvoteaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let commitment_config = CommitmentConfig::processed();
    /// let accounts = rpc_client.get_vote_accounts_with_commitment(
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_vote_accounts_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<RpcVoteAccountStatus> {
        self.invoke((self.rpc_client.as_ref()).get_vote_accounts_with_commitment(commitment_config))
    }

    /// Returns the account info and associated stake for all the voting accounts
    /// that have reached the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getVoteAccounts`] RPC method.
    ///
    /// [`getVoteAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getvoteaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::RpcGetVoteAccountsConfig,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signer::keypair::Keypair,
    /// #     signature::Signer,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let vote_keypair = Keypair::new();
    /// let vote_pubkey = vote_keypair.pubkey();
    /// let commitment = CommitmentConfig::processed();
    /// let config = RpcGetVoteAccountsConfig {
    ///     vote_pubkey: Some(vote_pubkey.to_string()),
    ///     commitment: Some(commitment),
    ///     keep_unstaked_delinquents: Some(true),
    ///     delinquent_slot_distance: Some(10),
    /// };
    /// let accounts = rpc_client.get_vote_accounts_with_config(
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_vote_accounts_with_config(
        &self,
        config: RpcGetVoteAccountsConfig,
    ) -> ClientResult<RpcVoteAccountStatus> {
        self.invoke((self.rpc_client.as_ref()).get_vote_accounts_with_config(config))
    }

    pub fn wait_for_max_stake(
        &self,
        commitment: CommitmentConfig,
        max_stake_percent: f32,
    ) -> ClientResult<()> {
        self.invoke((self.rpc_client.as_ref()).wait_for_max_stake(commitment, max_stake_percent))
    }

    /// Returns information about all the nodes participating in the cluster.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getClusterNodes`]
    /// RPC method.
    ///
    /// [`getClusterNodes`]: https://docs.solana.com/developing/clients/jsonrpc-api#getclusternodes
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let cluster_nodes = rpc_client.get_cluster_nodes()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_cluster_nodes(&self) -> ClientResult<Vec<RpcContactInfo>> {
        self.invoke((self.rpc_client.as_ref()).get_cluster_nodes())
    }

    /// Returns identity and transaction information about a confirmed block in the ledger.
    ///
    /// The encodings are returned in [`UiTransactionEncoding::Json`][uite]
    /// format. To return transactions in other encodings, use
    /// [`get_block_with_encoding`].
    ///
    /// [`get_block_with_encoding`]: RpcClient::get_block_with_encoding
    /// [uite]: UiTransactionEncoding::Json
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlock`] RPC
    /// method.
    ///
    /// [`getBlock`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblock
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let slot = rpc_client.get_slot()?;
    /// let block = rpc_client.get_block(slot)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block(&self, slot: Slot) -> ClientResult<EncodedConfirmedBlock> {
        self.invoke((self.rpc_client.as_ref()).get_block(slot))
    }

    /// Returns identity and transaction information about a confirmed block in the ledger.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlock`] RPC method.
    ///
    /// [`getBlock`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblock
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_transaction_status::UiTransactionEncoding;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let slot = rpc_client.get_slot()?;
    /// let encoding = UiTransactionEncoding::Base58;
    /// let block = rpc_client.get_block_with_encoding(
    ///     slot,
    ///     encoding,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_with_encoding(
        &self,
        slot: Slot,
        encoding: UiTransactionEncoding,
    ) -> ClientResult<EncodedConfirmedBlock> {
        self.invoke((self.rpc_client.as_ref()).get_block_with_encoding(slot, encoding))
    }

    /// Returns identity and transaction information about a confirmed block in the ledger.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlock`] RPC method.
    ///
    /// [`getBlock`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblock
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     config::RpcBlockConfig,
    /// #     client_error::Error,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_transaction_status::{
    /// #     TransactionDetails,
    /// #     UiTransactionEncoding,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let slot = rpc_client.get_slot()?;
    /// let config = RpcBlockConfig {
    ///     encoding: Some(UiTransactionEncoding::Base58),
    ///     transaction_details: Some(TransactionDetails::None),
    ///     rewards: Some(true),
    ///     commitment: None,
    ///     max_supported_transaction_version: Some(0),
    /// };
    /// let block = rpc_client.get_block_with_config(
    ///     slot,
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_with_config(
        &self,
        slot: Slot,
        config: RpcBlockConfig,
    ) -> ClientResult<UiConfirmedBlock> {
        self.invoke((self.rpc_client.as_ref()).get_block_with_config(slot, config))
    }

    #[deprecated(since = "1.7.0", note = "Please use RpcClient::get_block() instead")]
    #[allow(deprecated)]
    pub fn get_confirmed_block(&self, slot: Slot) -> ClientResult<EncodedConfirmedBlock> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_block(slot))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_block_with_encoding() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_block_with_encoding(
        &self,
        slot: Slot,
        encoding: UiTransactionEncoding,
    ) -> ClientResult<EncodedConfirmedBlock> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_block_with_encoding(slot, encoding))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_block_with_config() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_block_with_config(
        &self,
        slot: Slot,
        config: RpcConfirmedBlockConfig,
    ) -> ClientResult<UiConfirmedBlock> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_block_with_config(slot, config))
    }

    /// Returns a list of finalized blocks between two slots.
    ///
    /// The range is inclusive, with results including the block for both
    /// `start_slot` and `end_slot`.
    ///
    /// If `end_slot` is not provided, then the end slot is for the latest
    /// finalized block.
    ///
    /// This method may not return blocks for the full range of slots if some
    /// slots do not have corresponding blocks. To simply get a specific number
    /// of sequential blocks, use the [`get_blocks_with_limit`] method.
    ///
    /// This method uses the [`Finalized`] [commitment level][cl].
    ///
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    /// [`get_blocks_with_limit`]: RpcClient::get_blocks_with_limit.
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # Errors
    ///
    /// This method returns an error if the range is greater than 500,000 slots.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlocks`] RPC method, unless
    /// the remote node version is less than 1.7, in which case it maps to the
    /// [`getConfirmedBlocks`] RPC method.
    ///
    /// [`getBlocks`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblocks
    /// [`getConfirmedBlocks`]: https://docs.solana.com/developing/clients/jsonrpc-api#getConfirmedblocks
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Get up to the first 10 blocks
    /// let start_slot = 0;
    /// let end_slot = 9;
    /// let blocks = rpc_client.get_blocks(start_slot, Some(end_slot))?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_blocks(&self, start_slot: Slot, end_slot: Option<Slot>) -> ClientResult<Vec<Slot>> {
        self.invoke((self.rpc_client.as_ref()).get_blocks(start_slot, end_slot))
    }

    /// Returns a list of confirmed blocks between two slots.
    ///
    /// The range is inclusive, with results including the block for both
    /// `start_slot` and `end_slot`.
    ///
    /// If `end_slot` is not provided, then the end slot is for the latest
    /// block with the given [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// This method may not return blocks for the full range of slots if some
    /// slots do not have corresponding blocks. To simply get a specific number
    /// of sequential blocks, use the [`get_blocks_with_limit_and_commitment`]
    /// method.
    ///
    /// [`get_blocks_with_limit_and_commitment`]: RpcClient::get_blocks_with_limit_and_commitment.
    ///
    /// # Errors
    ///
    /// This method returns an error if the range is greater than 500,000 slots.
    ///
    /// This method returns an error if the given commitment level is below
    /// [`Confirmed`].
    ///
    /// [`Confirmed`]: solana_sdk::commitment_config::CommitmentLevel::Confirmed
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlocks`] RPC method, unless
    /// the remote node version is less than 1.7, in which case it maps to the
    /// [`getConfirmedBlocks`] RPC method.
    ///
    /// [`getBlocks`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblocks
    /// [`getConfirmedBlocks`]: https://docs.solana.com/developing/clients/jsonrpc-api#getConfirmedblocks
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Get up to the first 10 blocks
    /// let start_slot = 0;
    /// let end_slot = 9;
    /// // Method does not support commitment below `confirmed`
    /// let commitment_config = CommitmentConfig::confirmed();
    /// let blocks = rpc_client.get_blocks_with_commitment(
    ///     start_slot,
    ///     Some(end_slot),
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_blocks_with_commitment(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Vec<Slot>> {
        self.invoke((self.rpc_client.as_ref()).get_blocks_with_commitment(
            start_slot,
            end_slot,
            commitment_config,
        ))
    }

    /// Returns a list of finalized blocks starting at the given slot.
    ///
    /// This method uses the [`Finalized`] [commitment level][cl].
    ///
    /// [`Finalized`]: CommitmentLevel::Finalized.
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # Errors
    ///
    /// This method returns an error if the limit is greater than 500,000 slots.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlocksWithLimit`] RPC
    /// method, unless the remote node version is less than 1.7, in which case
    /// it maps to the [`getConfirmedBlocksWithLimit`] RPC method.
    ///
    /// [`getBlocksWithLimit`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblockswithlimit
    /// [`getConfirmedBlocksWithLimit`]: https://docs.solana.com/developing/clients/jsonrpc-api#getconfirmedblockswithlimit
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Get the first 10 blocks
    /// let start_slot = 0;
    /// let limit = 10;
    /// let blocks = rpc_client.get_blocks_with_limit(start_slot, limit)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_blocks_with_limit(&self, start_slot: Slot, limit: usize) -> ClientResult<Vec<Slot>> {
        self.invoke((self.rpc_client.as_ref()).get_blocks_with_limit(start_slot, limit))
    }

    /// Returns a list of confirmed blocks starting at the given slot.
    ///
    /// # Errors
    ///
    /// This method returns an error if the limit is greater than 500,000 slots.
    ///
    /// This method returns an error if the given [commitment level][cl] is below
    /// [`Confirmed`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Confirmed`]: solana_sdk::commitment_config::CommitmentLevel::Confirmed
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlocksWithLimit`] RPC
    /// method, unless the remote node version is less than 1.7, in which case
    /// it maps to the `getConfirmedBlocksWithLimit` RPC method.
    ///
    /// [`getBlocksWithLimit`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblockswithlimit
    /// [`getConfirmedBlocksWithLimit`]: https://docs.solana.com/developing/clients/jsonrpc-api#getconfirmedblockswithlimit
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Get the first 10 blocks
    /// let start_slot = 0;
    /// let limit = 10;
    /// let commitment_config = CommitmentConfig::confirmed();
    /// let blocks = rpc_client.get_blocks_with_limit_and_commitment(
    ///     start_slot,
    ///     limit,
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_blocks_with_limit_and_commitment(
        &self,
        start_slot: Slot,
        limit: usize,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Vec<Slot>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_blocks_with_limit_and_commitment(
                start_slot,
                limit,
                commitment_config,
            ),
        )
    }

    #[deprecated(since = "1.7.0", note = "Please use RpcClient::get_blocks() instead")]
    #[allow(deprecated)]
    pub fn get_confirmed_blocks(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> ClientResult<Vec<Slot>> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_blocks(start_slot, end_slot))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_blocks_with_commitment() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_blocks_with_commitment(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Vec<Slot>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_confirmed_blocks_with_commitment(
                start_slot,
                end_slot,
                commitment_config,
            ),
        )
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_blocks_with_limit() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_blocks_with_limit(
        &self,
        start_slot: Slot,
        limit: usize,
    ) -> ClientResult<Vec<Slot>> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_blocks_with_limit(start_slot, limit))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_blocks_with_limit_and_commitment() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_blocks_with_limit_and_commitment(
        &self,
        start_slot: Slot,
        limit: usize,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Vec<Slot>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_confirmed_blocks_with_limit_and_commitment(
                start_slot,
                limit,
                commitment_config,
            ),
        )
    }

    /// Get confirmed signatures for transactions involving an address.
    ///
    /// Returns up to 1000 signatures, ordered from newest to oldest.
    ///
    /// This method uses the [`Finalized`] [commitment level][cl].
    ///
    /// [`Finalized`]: CommitmentLevel::Finalized.
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSignaturesForAddress`] RPC
    /// method, unless the remote node version is less than 1.7, in which case
    /// it maps to the [`getSignaturesForAddress2`] RPC method.
    ///
    /// [`getSignaturesForAddress`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturesforaddress
    /// [`getSignaturesForAddress2`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturesforaddress2
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// let signatures = rpc_client.get_signatures_for_address(
    ///     &alice.pubkey(),
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signatures_for_address(
        &self,
        address: &Pubkey,
    ) -> ClientResult<Vec<RpcConfirmedTransactionStatusWithSignature>> {
        self.invoke((self.rpc_client.as_ref()).get_signatures_for_address(address))
    }

    /// Get confirmed signatures for transactions involving an address.
    ///
    /// # Errors
    ///
    /// This method returns an error if the given [commitment level][cl] is below
    /// [`Confirmed`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Confirmed`]: solana_sdk::commitment_config::CommitmentLevel::Confirmed
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getSignaturesForAddress`] RPC
    /// method, unless the remote node version is less than 1.7, in which case
    /// it maps to the [`getSignaturesForAddress2`] RPC method.
    ///
    /// [`getSignaturesForAddress`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturesforaddress
    /// [`getSignaturesForAddress2`]: https://docs.solana.com/developing/clients/jsonrpc-api#getsignaturesforaddress2
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// # let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// # let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    /// let config = GetConfirmedSignaturesForAddress2Config {
    ///     before: None,
    ///     until: None,
    ///     limit: Some(3),
    ///     commitment: Some(CommitmentConfig::confirmed()),
    /// };
    /// let signatures = rpc_client.get_signatures_for_address_with_config(
    ///     &alice.pubkey(),
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_signatures_for_address_with_config(
        &self,
        address: &Pubkey,
        config: GetConfirmedSignaturesForAddress2Config,
    ) -> ClientResult<Vec<RpcConfirmedTransactionStatusWithSignature>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_signatures_for_address_with_config(address, config),
        )
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_signatures_for_address() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_signatures_for_address2(
        &self,
        address: &Pubkey,
    ) -> ClientResult<Vec<RpcConfirmedTransactionStatusWithSignature>> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_signatures_for_address2(address))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_signatures_for_address_with_config() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_signatures_for_address2_with_config(
        &self,
        address: &Pubkey,
        config: GetConfirmedSignaturesForAddress2Config,
    ) -> ClientResult<Vec<RpcConfirmedTransactionStatusWithSignature>> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_confirmed_signatures_for_address2_with_config(address, config),
        )
    }

    /// Returns transaction details for a confirmed transaction.
    ///
    /// This method uses the [`Finalized`] [commitment level][cl].
    ///
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getTransaction`] RPC method,
    /// unless the remote node version is less than 1.7, in which case it maps
    /// to the [`getConfirmedTransaction`] RPC method.
    ///
    /// [`getTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#gettransaction
    /// [`getConfirmedTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#getconfirmedtransaction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// # };
    /// # use solana_transaction_status::UiTransactionEncoding;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// # let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    /// let transaction = rpc_client.get_transaction(
    ///     &signature,
    ///     UiTransactionEncoding::Json,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_transaction(
        &self,
        signature: &Signature,
        encoding: UiTransactionEncoding,
    ) -> ClientResult<EncodedConfirmedTransactionWithStatusMeta> {
        self.invoke((self.rpc_client.as_ref()).get_transaction(signature, encoding))
    }

    /// Returns transaction details for a confirmed transaction.
    ///
    /// # Errors
    ///
    /// This method returns an error if the given [commitment level][cl] is below
    /// [`Confirmed`].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    /// [`Confirmed`]: solana_sdk::commitment_config::CommitmentLevel::Confirmed
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getTransaction`] RPC method,
    /// unless the remote node version is less than 1.7, in which case it maps
    /// to the [`getConfirmedTransaction`] RPC method.
    ///
    /// [`getTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#gettransaction
    /// [`getConfirmedTransaction`]: https://docs.solana.com/developing/clients/jsonrpc-api#getconfirmedtransaction
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::RpcTransactionConfig,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signature::Signature,
    /// #     signer::keypair::Keypair,
    /// #     system_transaction,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # use solana_transaction_status::UiTransactionEncoding;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// # let lamports = 50;
    /// # let latest_blockhash = rpc_client.get_latest_blockhash()?;
    /// # let tx = system_transaction::transfer(&alice, &bob.pubkey(), lamports, latest_blockhash);
    /// let signature = rpc_client.send_and_confirm_transaction(&tx)?;
    /// let config = RpcTransactionConfig {
    ///     encoding: Some(UiTransactionEncoding::Json),
    ///     commitment: Some(CommitmentConfig::confirmed()),
    ///     max_supported_transaction_version: Some(0),
    /// };
    /// let transaction = rpc_client.get_transaction_with_config(
    ///     &signature,
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_transaction_with_config(
        &self,
        signature: &Signature,
        config: RpcTransactionConfig,
    ) -> ClientResult<EncodedConfirmedTransactionWithStatusMeta> {
        self.invoke((self.rpc_client.as_ref()).get_transaction_with_config(signature, config))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_transaction() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_transaction(
        &self,
        signature: &Signature,
        encoding: UiTransactionEncoding,
    ) -> ClientResult<EncodedConfirmedTransactionWithStatusMeta> {
        self.invoke((self.rpc_client.as_ref()).get_confirmed_transaction(signature, encoding))
    }

    #[deprecated(
        since = "1.7.0",
        note = "Please use RpcClient::get_transaction_with_config() instead"
    )]
    #[allow(deprecated)]
    pub fn get_confirmed_transaction_with_config(
        &self,
        signature: &Signature,
        config: RpcConfirmedTransactionConfig,
    ) -> ClientResult<EncodedConfirmedTransactionWithStatusMeta> {
        self.invoke(
            (self.rpc_client.as_ref()).get_confirmed_transaction_with_config(signature, config),
        )
    }

    /// Returns the estimated production time of a block.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBlockTime`] RPC method.
    ///
    /// [`getBlockTime`]: https://docs.solana.com/developing/clients/jsonrpc-api#getblocktime
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// // Get the time of the most recent finalized block
    /// let slot = rpc_client.get_slot()?;
    /// let block_time = rpc_client.get_block_time(slot)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_block_time(&self, slot: Slot) -> ClientResult<UnixTimestamp> {
        self.invoke((self.rpc_client.as_ref()).get_block_time(slot))
    }

    /// Returns information about the current epoch.
    ///
    /// This method uses the configured default [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getEpochInfo`] RPC method.
    ///
    /// [`getEpochInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getepochinfo
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let epoch_info = rpc_client.get_epoch_info()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_epoch_info(&self) -> ClientResult<EpochInfo> {
        self.invoke((self.rpc_client.as_ref()).get_epoch_info())
    }

    /// Returns information about the current epoch.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getEpochInfo`] RPC method.
    ///
    /// [`getEpochInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getepochinfo
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let commitment_config = CommitmentConfig::confirmed();
    /// let epoch_info = rpc_client.get_epoch_info_with_commitment(
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_epoch_info_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<EpochInfo> {
        self.invoke((self.rpc_client.as_ref()).get_epoch_info_with_commitment(commitment_config))
    }

    /// Returns the leader schedule for an epoch.
    ///
    /// This method uses the configured default [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getLeaderSchedule`] RPC method.
    ///
    /// [`getLeaderSchedule`]: https://docs.solana.com/developing/clients/jsonrpc-api#getleaderschedule
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let slot = rpc_client.get_slot()?;
    /// let leader_schedule = rpc_client.get_leader_schedule(
    ///     Some(slot),
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_leader_schedule(
        &self,
        slot: Option<Slot>,
    ) -> ClientResult<Option<RpcLeaderSchedule>> {
        self.invoke((self.rpc_client.as_ref()).get_leader_schedule(slot))
    }

    /// Returns the leader schedule for an epoch.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getLeaderSchedule`] RPC method.
    ///
    /// [`getLeaderSchedule`]: https://docs.solana.com/developing/clients/jsonrpc-api#getleaderschedule
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let slot = rpc_client.get_slot()?;
    /// let commitment_config = CommitmentConfig::processed();
    /// let leader_schedule = rpc_client.get_leader_schedule_with_commitment(
    ///     Some(slot),
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_leader_schedule_with_commitment(
        &self,
        slot: Option<Slot>,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<Option<RpcLeaderSchedule>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_leader_schedule_with_commitment(slot, commitment_config),
        )
    }

    /// Returns the leader schedule for an epoch.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getLeaderSchedule`] RPC method.
    ///
    /// [`getLeaderSchedule`]: https://docs.solana.com/developing/clients/jsonrpc-api#getleaderschedule
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::RpcLeaderScheduleConfig,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let slot = rpc_client.get_slot()?;
    /// # let validator_pubkey_str = "7AYmEYBBetok8h5L3Eo3vi3bDWnjNnaFbSXfSNYV5ewB".to_string();
    /// let config = RpcLeaderScheduleConfig {
    ///     identity: Some(validator_pubkey_str),
    ///     commitment: Some(CommitmentConfig::processed()),
    /// };
    /// let leader_schedule = rpc_client.get_leader_schedule_with_config(
    ///     Some(slot),
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_leader_schedule_with_config(
        &self,
        slot: Option<Slot>,
        config: RpcLeaderScheduleConfig,
    ) -> ClientResult<Option<RpcLeaderSchedule>> {
        self.invoke((self.rpc_client.as_ref()).get_leader_schedule_with_config(slot, config))
    }

    /// Returns epoch schedule information from this cluster's genesis config.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getEpochSchedule`] RPC method.
    ///
    /// [`getEpochSchedule`]: https://docs.solana.com/developing/clients/jsonrpc-api#getepochschedule
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let epoch_schedule = rpc_client.get_epoch_schedule()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_epoch_schedule(&self) -> ClientResult<EpochSchedule> {
        self.invoke((self.rpc_client.as_ref()).get_epoch_schedule())
    }

    /// Returns a list of recent performance samples, in reverse slot order.
    ///
    /// Performance samples are taken every 60 seconds and include the number of
    /// transactions and slots that occur in a given time window.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getRecentPerformanceSamples`] RPC method.
    ///
    /// [`getRecentPerformanceSamples`]: https://docs.solana.com/developing/clients/jsonrpc-api#getrecentperformancesamples
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let limit = 10;
    /// let performance_samples = rpc_client.get_recent_performance_samples(
    ///     Some(limit),
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_recent_performance_samples(
        &self,
        limit: Option<usize>,
    ) -> ClientResult<Vec<RpcPerfSample>> {
        self.invoke((self.rpc_client.as_ref()).get_recent_performance_samples(limit))
    }

    /// Returns a list of minimum prioritization fees from recent blocks.
    /// Takes an optional vector of addresses; if any addresses are provided, the response will
    /// reflect the minimum prioritization fee to land a transaction locking all of the provided
    /// accounts as writable.
    ///
    /// Currently, a node's prioritization-fee cache stores data from up to 150 blocks.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getRecentPrioritizationFees`] RPC method.
    ///
    /// [`getRecentPrioritizationFees`]: https://docs.solana.com/developing/clients/jsonrpc-api#getrecentprioritizationfees
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::signature::{Keypair, Signer};
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// let addresses = vec![alice.pubkey(), bob.pubkey()];
    /// let prioritization_fees = rpc_client.get_recent_prioritization_fees(
    ///     &addresses,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_recent_prioritization_fees(
        &self,
        addresses: &[Pubkey],
    ) -> ClientResult<Vec<RpcPrioritizationFee>> {
        self.invoke((self.rpc_client.as_ref()).get_recent_prioritization_fees(addresses))
    }

    /// Returns the identity pubkey for the current node.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getIdentity`] RPC method.
    ///
    /// [`getIdentity`]: https://docs.solana.com/developing/clients/jsonrpc-api#getidentity
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let identity = rpc_client.get_identity()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_identity(&self) -> ClientResult<Pubkey> {
        self.invoke((self.rpc_client.as_ref()).get_identity())
    }

    /// Returns the current inflation governor.
    ///
    /// This method uses the [`Finalized`] [commitment level][cl].
    ///
    /// [`Finalized`]: solana_sdk::commitment_config::CommitmentLevel::Finalized
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getInflationGovernor`] RPC
    /// method.
    ///
    /// [`getInflationGovernor`]: https://docs.solana.com/developing/clients/jsonrpc-api#getinflationgovernor
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let inflation_governor = rpc_client.get_inflation_governor()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_inflation_governor(&self) -> ClientResult<RpcInflationGovernor> {
        self.invoke((self.rpc_client.as_ref()).get_inflation_governor())
    }

    /// Returns the specific inflation values for the current epoch.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getInflationRate`] RPC method.
    ///
    /// [`getInflationRate`]: https://docs.solana.com/developing/clients/jsonrpc-api#getinflationrate
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let inflation_rate = rpc_client.get_inflation_rate()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_inflation_rate(&self) -> ClientResult<RpcInflationRate> {
        self.invoke((self.rpc_client.as_ref()).get_inflation_rate())
    }

    /// Returns the inflation reward for a list of addresses for an epoch.
    ///
    /// This method uses the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getInflationReward`] RPC method.
    ///
    /// [`getInflationReward`]: https://docs.solana.com/developing/clients/jsonrpc-api#getinflationreward
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::signature::{Keypair, Signer};
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let epoch_info = rpc_client.get_epoch_info()?;
    /// # let epoch = epoch_info.epoch;
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// let addresses = vec![alice.pubkey(), bob.pubkey()];
    /// let inflation_reward = rpc_client.get_inflation_reward(
    ///     &addresses,
    ///     Some(epoch),
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_inflation_reward(
        &self,
        addresses: &[Pubkey],
        epoch: Option<Epoch>,
    ) -> ClientResult<Vec<Option<RpcInflationReward>>> {
        self.invoke((self.rpc_client.as_ref()).get_inflation_reward(addresses, epoch))
    }

    /// Returns the current solana version running on the node.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getVersion`] RPC method.
    ///
    /// [`getVersion`]: https://docs.solana.com/developing/clients/jsonrpc-api#getversion
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::signature::{Keypair, Signer};
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let expected_version = semver::Version::new(1, 7, 0);
    /// let version = rpc_client.get_version()?;
    /// let version = semver::Version::parse(&version.solana_core)?;
    /// assert!(version >= expected_version);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn get_version(&self) -> ClientResult<RpcVersionInfo> {
        self.invoke((self.rpc_client.as_ref()).get_version())
    }

    /// Returns the lowest slot that the node has information about in its ledger.
    ///
    /// This value may increase over time if the node is configured to purge
    /// older ledger data.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`minimumLedgerSlot`] RPC
    /// method.
    ///
    /// [`minimumLedgerSlot`]: https://docs.solana.com/developing/clients/jsonrpc-api#minimumledgerslot
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let slot = rpc_client.minimum_ledger_slot()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn minimum_ledger_slot(&self) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).minimum_ledger_slot())
    }

    /// Returns all information associated with the account of the provided pubkey.
    ///
    /// This method uses the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// To get multiple accounts at once, use the [`get_multiple_accounts`] method.
    ///
    /// [`get_multiple_accounts`]: RpcClient::get_multiple_accounts
    ///
    /// # Errors
    ///
    /// If the account does not exist, this method returns
    /// [`RpcError::ForUser`]. This is unlike [`get_account_with_commitment`],
    /// which returns `Ok(None)` if the account does not exist.
    ///
    /// [`RpcError::ForUser`]: solana_rpc_client_api::request::RpcError::ForUser
    /// [`get_account_with_commitment`]: RpcClient::get_account_with_commitment
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getAccountInfo`] RPC method.
    ///
    /// [`getAccountInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getaccountinfo
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::{self, RpcClient};
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     pubkey::Pubkey,
    /// # };
    /// # use std::str::FromStr;
    /// # let mocks = rpc_client::create_rpc_client_mocks();
    /// # let rpc_client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
    /// let alice_pubkey = Pubkey::from_str("BgvYtJEfmZYdVKiptmMjxGzv8iQoo4MWjsP3QsTkhhxa").unwrap();
    /// let account = rpc_client.get_account(&alice_pubkey)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_account(&self, pubkey: &Pubkey) -> ClientResult<Account> {
        self.invoke((self.rpc_client.as_ref()).get_account(pubkey))
    }

    /// Returns all information associated with the account of the provided pubkey.
    ///
    /// If the account does not exist, this method returns `Ok(None)`.
    ///
    /// To get multiple accounts at once, use the [`get_multiple_accounts_with_commitment`] method.
    ///
    /// [`get_multiple_accounts_with_commitment`]: RpcClient::get_multiple_accounts_with_commitment
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getAccountInfo`] RPC method.
    ///
    /// [`getAccountInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getaccountinfo
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::{self, RpcClient};
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     pubkey::Pubkey,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # use std::str::FromStr;
    /// # let mocks = rpc_client::create_rpc_client_mocks();
    /// # let rpc_client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
    /// let alice_pubkey = Pubkey::from_str("BgvYtJEfmZYdVKiptmMjxGzv8iQoo4MWjsP3QsTkhhxa").unwrap();
    /// let commitment_config = CommitmentConfig::processed();
    /// let account = rpc_client.get_account_with_commitment(
    ///     &alice_pubkey,
    ///     commitment_config,
    /// )?;
    /// assert!(account.value.is_some());
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Option<Account>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_account_with_commitment(pubkey, commitment_config),
        )
    }

    /// Returns all information associated with the account of the provided pubkey.
    ///
    /// If the account does not exist, this method returns `Ok(None)`.
    ///
    /// To get multiple accounts at once, use the [`get_multiple_accounts_with_config`] method.
    ///
    /// [`get_multiple_accounts_with_config`]: RpcClient::get_multiple_accounts_with_config
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getAccountInfo`] RPC method.
    ///
    /// [`getAccountInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getaccountinfo
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     config::RpcAccountInfoConfig,
    /// #     client_error::Error,
    /// # };
    /// # use solana_rpc_client::rpc_client::{self, RpcClient};
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     pubkey::Pubkey,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # use solana_account_decoder::UiAccountEncoding;
    /// # use std::str::FromStr;
    /// # let mocks = rpc_client::create_rpc_client_mocks();
    /// # let rpc_client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
    /// let alice_pubkey = Pubkey::from_str("BgvYtJEfmZYdVKiptmMjxGzv8iQoo4MWjsP3QsTkhhxa").unwrap();
    /// let commitment_config = CommitmentConfig::processed();
    /// let config = RpcAccountInfoConfig {
    ///     encoding: Some(UiAccountEncoding::Base64),
    ///     commitment: Some(commitment_config),
    ///     .. RpcAccountInfoConfig::default()
    /// };
    /// let account = rpc_client.get_account_with_config(
    ///     &alice_pubkey,
    ///     config,
    /// )?;
    /// assert!(account.value.is_some());
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_account_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcAccountInfoConfig,
    ) -> RpcResult<Option<Account>> {
        self.invoke((self.rpc_client.as_ref()).get_account_with_config(pubkey, config))
    }

    /// Get the max slot seen from retransmit stage.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getMaxRetransmitSlot`] RPC
    /// method.
    ///
    /// [`getMaxRetransmitSlot`]: https://docs.solana.com/developing/clients/jsonrpc-api#getmaxretransmitslot
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let slot = rpc_client.get_max_retransmit_slot()?;
    /// # Ok::<(), Error>(())
    pub fn get_max_retransmit_slot(&self) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).get_max_retransmit_slot())
    }

    /// Get the max slot seen from after [shred](https://docs.solana.com/terminology#shred) insert.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the
    /// [`getMaxShredInsertSlot`] RPC method.
    ///
    /// [`getMaxShredInsertSlot`]: https://docs.solana.com/developing/clients/jsonrpc-api#getmaxshredinsertslot
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let slot = rpc_client.get_max_shred_insert_slot()?;
    /// # Ok::<(), Error>(())
    pub fn get_max_shred_insert_slot(&self) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).get_max_shred_insert_slot())
    }

    /// Returns the account information for a list of pubkeys.
    ///
    /// This method uses the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getMultipleAccounts`] RPC method.
    ///
    /// [`getMultipleAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getmultipleaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// let pubkeys = vec![alice.pubkey(), bob.pubkey()];
    /// let accounts = rpc_client.get_multiple_accounts(&pubkeys)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> ClientResult<Vec<Option<Account>>> {
        self.invoke((self.rpc_client.as_ref()).get_multiple_accounts(pubkeys))
    }

    /// Returns the account information for a list of pubkeys.
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getMultipleAccounts`] RPC method.
    ///
    /// [`getMultipleAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getmultipleaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// let pubkeys = vec![alice.pubkey(), bob.pubkey()];
    /// let commitment_config = CommitmentConfig::processed();
    /// let accounts = rpc_client.get_multiple_accounts_with_commitment(
    ///     &pubkeys,
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_multiple_accounts_with_commitment(
        &self,
        pubkeys: &[Pubkey],
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Vec<Option<Account>>> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_multiple_accounts_with_commitment(pubkeys, commitment_config),
        )
    }

    /// Returns the account information for a list of pubkeys.
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getMultipleAccounts`] RPC method.
    ///
    /// [`getMultipleAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getmultipleaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     config::RpcAccountInfoConfig,
    /// #     client_error::Error,
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # use solana_account_decoder::UiAccountEncoding;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let bob = Keypair::new();
    /// let pubkeys = vec![alice.pubkey(), bob.pubkey()];
    /// let commitment_config = CommitmentConfig::processed();
    /// let config = RpcAccountInfoConfig {
    ///     encoding: Some(UiAccountEncoding::Base64),
    ///     commitment: Some(commitment_config),
    ///     .. RpcAccountInfoConfig::default()
    /// };
    /// let accounts = rpc_client.get_multiple_accounts_with_config(
    ///     &pubkeys,
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_multiple_accounts_with_config(
        &self,
        pubkeys: &[Pubkey],
        config: RpcAccountInfoConfig,
    ) -> RpcResult<Vec<Option<Account>>> {
        self.invoke((self.rpc_client.as_ref()).get_multiple_accounts_with_config(pubkeys, config))
    }

    /// Gets the raw data associated with an account.
    ///
    /// This is equivalent to calling [`get_account`] and then accessing the
    /// [`data`] field of the returned [`Account`].
    ///
    /// [`get_account`]: RpcClient::get_account
    /// [`data`]: Account::data
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getAccountInfo`] RPC method.
    ///
    /// [`getAccountInfo`]: https://docs.solana.com/developing/clients/jsonrpc-api#getaccountinfo
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::{self, RpcClient};
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     pubkey::Pubkey,
    /// # };
    /// # use std::str::FromStr;
    /// # let mocks = rpc_client::create_rpc_client_mocks();
    /// # let rpc_client = RpcClient::new_mock_with_mocks("succeeds".to_string(), mocks);
    /// let alice_pubkey = Pubkey::from_str("BgvYtJEfmZYdVKiptmMjxGzv8iQoo4MWjsP3QsTkhhxa").unwrap();
    /// let account_data = rpc_client.get_account_data(&alice_pubkey)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_account_data(&self, pubkey: &Pubkey) -> ClientResult<Vec<u8>> {
        self.invoke((self.rpc_client.as_ref()).get_account_data(pubkey))
    }

    /// Returns minimum balance required to make an account with specified data length rent exempt.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the
    /// [`getMinimumBalanceForRentExemption`] RPC method.
    ///
    /// [`getMinimumBalanceForRentExemption`]: https://docs.solana.com/developing/clients/jsonrpc-api#getminimumbalanceforrentexemption
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let data_len = 300;
    /// let balance = rpc_client.get_minimum_balance_for_rent_exemption(data_len)?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_minimum_balance_for_rent_exemption(data_len))
    }

    /// Request the balance of the provided account pubkey.
    ///
    /// This method uses the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBalance`] RPC method.
    ///
    /// [`getBalance`]: https://docs.solana.com/developing/clients/jsonrpc-api#getbalance
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// let balance = rpc_client.get_balance(&alice.pubkey())?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_balance(&self, pubkey: &Pubkey) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_balance(pubkey))
    }

    /// Request the balance of the provided account pubkey.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getBalance`] RPC method.
    ///
    /// [`getBalance`]: https://docs.solana.com/developing/clients/jsonrpc-api#getbalance
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// let commitment_config = CommitmentConfig::processed();
    /// let balance = rpc_client.get_balance_with_commitment(
    ///     &alice.pubkey(),
    ///     commitment_config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<u64> {
        self.invoke(
            (self.rpc_client.as_ref()).get_balance_with_commitment(pubkey, commitment_config),
        )
    }

    /// Returns all accounts owned by the provided program pubkey.
    ///
    /// This method uses the configured [commitment level][cl].
    ///
    /// [cl]: https://docs.solana.com/developing/clients/jsonrpc-api#configuring-state-commitment
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getProgramAccounts`] RPC
    /// method.
    ///
    /// [`getProgramAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getprogramaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// # };
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// let accounts = rpc_client.get_program_accounts(&alice.pubkey())?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_program_accounts(&self, pubkey: &Pubkey) -> ClientResult<Vec<(Pubkey, Account)>> {
        self.invoke((self.rpc_client.as_ref()).get_program_accounts(pubkey))
    }

    /// Returns all accounts owned by the provided program pubkey.
    ///
    /// # RPC Reference
    ///
    /// This method is built on the [`getProgramAccounts`] RPC method.
    ///
    /// [`getProgramAccounts`]: https://docs.solana.com/developing/clients/jsonrpc-api#getprogramaccounts
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::{
    /// #     client_error::Error,
    /// #     config::{RpcAccountInfoConfig, RpcProgramAccountsConfig},
    /// #     filter::{MemcmpEncodedBytes, RpcFilterType, Memcmp},
    /// # };
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::{
    /// #     signature::Signer,
    /// #     signer::keypair::Keypair,
    /// #     commitment_config::CommitmentConfig,
    /// # };
    /// # use solana_account_decoder::{UiDataSliceConfig, UiAccountEncoding};
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// # let alice = Keypair::new();
    /// # let base64_bytes = "\
    /// #     AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\
    /// #     AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\
    /// #     AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    /// let memcmp = RpcFilterType::Memcmp(Memcmp::new(
    ///     0,                                                    // offset
    ///     MemcmpEncodedBytes::Base64(base64_bytes.to_string()), // encoded bytes
    /// ));
    /// let config = RpcProgramAccountsConfig {
    ///     filters: Some(vec![
    ///         RpcFilterType::DataSize(128),
    ///         memcmp,
    ///     ]),
    ///     account_config: RpcAccountInfoConfig {
    ///         encoding: Some(UiAccountEncoding::Base64),
    ///         data_slice: Some(UiDataSliceConfig {
    ///             offset: 0,
    ///             length: 5,
    ///         }),
    ///         commitment: Some(CommitmentConfig::processed()),
    ///         min_context_slot: Some(1234),
    ///     },
    ///     with_context: Some(false),
    /// };
    /// let accounts = rpc_client.get_program_accounts_with_config(
    ///     &alice.pubkey(),
    ///     config,
    /// )?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_program_accounts_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcProgramAccountsConfig,
    ) -> ClientResult<Vec<(Pubkey, Account)>> {
        self.invoke((self.rpc_client.as_ref()).get_program_accounts_with_config(pubkey, config))
    }

    /// Returns the stake minimum delegation, in lamports.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getStakeMinimumDelegation`] RPC method.
    ///
    /// [`getStakeMinimumDelegation`]: https://docs.solana.com/developing/clients/jsonrpc-api#getstakeminimumdelegation
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let stake_minimum_delegation = rpc_client.get_stake_minimum_delegation()?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_stake_minimum_delegation(&self) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_stake_minimum_delegation())
    }

    /// Returns the stake minimum delegation, in lamports, based on the commitment level.
    ///
    /// # RPC Reference
    ///
    /// This method corresponds directly to the [`getStakeMinimumDelegation`] RPC method.
    ///
    /// [`getStakeMinimumDelegation`]: https://docs.solana.com/developing/clients/jsonrpc-api#getstakeminimumdelegation
    ///
    /// # Examples
    ///
    /// ```
    /// # use solana_rpc_client_api::client_error::Error;
    /// # use solana_rpc_client::rpc_client::RpcClient;
    /// # use solana_sdk::commitment_config::CommitmentConfig;
    /// # let rpc_client = RpcClient::new_mock("succeeds".to_string());
    /// let stake_minimum_delegation =
    /// rpc_client.get_stake_minimum_delegation_with_commitment(CommitmentConfig::confirmed())?;
    /// # Ok::<(), Error>(())
    /// ```
    pub fn get_stake_minimum_delegation_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<u64> {
        self.invoke(
            self.rpc_client
                .get_stake_minimum_delegation_with_commitment(commitment_config),
        )
    }

    /// Request the transaction count.
    pub fn get_transaction_count(&self) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_transaction_count())
    }

    pub fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<u64> {
        self.invoke(
            (self.rpc_client.as_ref()).get_transaction_count_with_commitment(commitment_config),
        )
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please use `get_latest_blockhash` and `get_fee_for_message` instead"
    )]
    #[allow(deprecated)]
    pub fn get_fees(&self) -> ClientResult<Fees> {
        self.invoke((self.rpc_client.as_ref()).get_fees())
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please use `get_latest_blockhash_with_commitment` and `get_fee_for_message` instead"
    )]
    #[allow(deprecated)]
    pub fn get_fees_with_commitment(&self, commitment_config: CommitmentConfig) -> RpcResult<Fees> {
        self.invoke((self.rpc_client.as_ref()).get_fees_with_commitment(commitment_config))
    }

    #[deprecated(since = "1.9.0", note = "Please use `get_latest_blockhash` instead")]
    #[allow(deprecated)]
    pub fn get_recent_blockhash(&self) -> ClientResult<(Hash, FeeCalculator)> {
        self.invoke((self.rpc_client.as_ref()).get_recent_blockhash())
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please use `get_latest_blockhash_with_commitment` instead"
    )]
    #[allow(deprecated)]
    pub fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<(Hash, FeeCalculator, Slot)> {
        self.invoke(
            (self.rpc_client.as_ref()).get_recent_blockhash_with_commitment(commitment_config),
        )
    }

    #[deprecated(since = "1.9.0", note = "Please `get_fee_for_message` instead")]
    #[allow(deprecated)]
    pub fn get_fee_calculator_for_blockhash(
        &self,
        blockhash: &Hash,
    ) -> ClientResult<Option<FeeCalculator>> {
        self.invoke((self.rpc_client.as_ref()).get_fee_calculator_for_blockhash(blockhash))
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please `get_latest_blockhash_with_commitment` and `get_fee_for_message` instead"
    )]
    #[allow(deprecated)]
    pub fn get_fee_calculator_for_blockhash_with_commitment(
        &self,
        blockhash: &Hash,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Option<FeeCalculator>> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_fee_calculator_for_blockhash_with_commitment(blockhash, commitment_config),
        )
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please do not use, will no longer be available in the future"
    )]
    #[allow(deprecated)]
    pub fn get_fee_rate_governor(&self) -> RpcResult<FeeRateGovernor> {
        self.invoke((self.rpc_client.as_ref()).get_fee_rate_governor())
    }

    #[deprecated(
        since = "1.9.0",
        note = "Please do not use, will no longer be available in the future"
    )]
    #[allow(deprecated)]
    pub fn get_new_blockhash(&self, blockhash: &Hash) -> ClientResult<(Hash, FeeCalculator)> {
        self.invoke((self.rpc_client.as_ref()).get_new_blockhash(blockhash))
    }

    pub fn get_first_available_block(&self) -> ClientResult<Slot> {
        self.invoke((self.rpc_client.as_ref()).get_first_available_block())
    }

    pub fn get_genesis_hash(&self) -> ClientResult<Hash> {
        self.invoke((self.rpc_client.as_ref()).get_genesis_hash())
    }

    pub fn get_health(&self) -> ClientResult<()> {
        self.invoke((self.rpc_client.as_ref()).get_health())
    }

    pub fn get_token_account(&self, pubkey: &Pubkey) -> ClientResult<Option<UiTokenAccount>> {
        self.invoke((self.rpc_client.as_ref()).get_token_account(pubkey))
    }

    pub fn get_token_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Option<UiTokenAccount>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_token_account_with_commitment(pubkey, commitment_config),
        )
    }

    pub fn get_token_account_balance(&self, pubkey: &Pubkey) -> ClientResult<UiTokenAmount> {
        self.invoke((self.rpc_client.as_ref()).get_token_account_balance(pubkey))
    }

    pub fn get_token_account_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<UiTokenAmount> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_token_account_balance_with_commitment(pubkey, commitment_config),
        )
    }

    pub fn get_token_accounts_by_delegate(
        &self,
        delegate: &Pubkey,
        token_account_filter: TokenAccountsFilter,
    ) -> ClientResult<Vec<RpcKeyedAccount>> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_token_accounts_by_delegate(delegate, token_account_filter),
        )
    }

    pub fn get_token_accounts_by_delegate_with_commitment(
        &self,
        delegate: &Pubkey,
        token_account_filter: TokenAccountsFilter,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Vec<RpcKeyedAccount>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_token_accounts_by_delegate_with_commitment(
                delegate,
                token_account_filter,
                commitment_config,
            ),
        )
    }

    pub fn get_token_accounts_by_owner(
        &self,
        owner: &Pubkey,
        token_account_filter: TokenAccountsFilter,
    ) -> ClientResult<Vec<RpcKeyedAccount>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_token_accounts_by_owner(owner, token_account_filter),
        )
    }

    pub fn get_token_accounts_by_owner_with_commitment(
        &self,
        owner: &Pubkey,
        token_account_filter: TokenAccountsFilter,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Vec<RpcKeyedAccount>> {
        self.invoke(
            (self.rpc_client.as_ref()).get_token_accounts_by_owner_with_commitment(
                owner,
                token_account_filter,
                commitment_config,
            ),
        )
    }

    pub fn get_token_largest_accounts(
        &self,
        mint: &Pubkey,
    ) -> ClientResult<Vec<RpcTokenAccountBalance>> {
        self.invoke((self.rpc_client.as_ref()).get_token_largest_accounts(mint))
    }

    pub fn get_token_largest_accounts_with_commitment(
        &self,
        mint: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<Vec<RpcTokenAccountBalance>> {
        self.invoke(
            (self.rpc_client.as_ref())
                .get_token_largest_accounts_with_commitment(mint, commitment_config),
        )
    }

    pub fn get_token_supply(&self, mint: &Pubkey) -> ClientResult<UiTokenAmount> {
        self.invoke((self.rpc_client.as_ref()).get_token_supply(mint))
    }

    pub fn get_token_supply_with_commitment(
        &self,
        mint: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResult<UiTokenAmount> {
        self.invoke(
            (self.rpc_client.as_ref()).get_token_supply_with_commitment(mint, commitment_config),
        )
    }

    pub fn request_airdrop(&self, pubkey: &Pubkey, lamports: u64) -> ClientResult<Signature> {
        self.invoke((self.rpc_client.as_ref()).request_airdrop(pubkey, lamports))
    }

    pub fn request_airdrop_with_blockhash(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
        recent_blockhash: &Hash,
    ) -> ClientResult<Signature> {
        self.invoke((self.rpc_client.as_ref()).request_airdrop_with_blockhash(
            pubkey,
            lamports,
            recent_blockhash,
        ))
    }

    pub fn request_airdrop_with_config(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
        config: RpcRequestAirdropConfig,
    ) -> ClientResult<Signature> {
        self.invoke(
            (self.rpc_client.as_ref()).request_airdrop_with_config(pubkey, lamports, config),
        )
    }

    pub fn poll_get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<u64> {
        self.invoke(
            (self.rpc_client.as_ref()).poll_get_balance_with_commitment(pubkey, commitment_config),
        )
    }

    pub fn wait_for_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        expected_balance: Option<u64>,
        commitment_config: CommitmentConfig,
    ) -> Option<u64> {
        self.invoke((self.rpc_client.as_ref()).wait_for_balance_with_commitment(
            pubkey,
            expected_balance,
            commitment_config,
        ))
        .ok()
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature(&self, signature: &Signature) -> ClientResult<()> {
        self.invoke((self.rpc_client.as_ref()).poll_for_signature(signature))
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> ClientResult<()> {
        self.invoke(
            (self.rpc_client.as_ref())
                .poll_for_signature_with_commitment(signature, commitment_config),
        )
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> ClientResult<usize> {
        self.invoke(
            (self.rpc_client.as_ref())
                .poll_for_signature_confirmation(signature, min_confirmed_blocks),
        )
    }

    pub fn get_num_blocks_since_signature_confirmation(
        &self,
        signature: &Signature,
    ) -> ClientResult<usize> {
        self.invoke(
            (self.rpc_client.as_ref()).get_num_blocks_since_signature_confirmation(signature),
        )
    }

    pub fn get_latest_blockhash(&self) -> ClientResult<Hash> {
        self.invoke((self.rpc_client.as_ref()).get_latest_blockhash())
    }

    #[allow(deprecated)]
    pub fn get_latest_blockhash_with_commitment(
        &self,
        commitment: CommitmentConfig,
    ) -> ClientResult<(Hash, u64)> {
        self.invoke((self.rpc_client.as_ref()).get_latest_blockhash_with_commitment(commitment))
    }

    #[allow(deprecated)]
    pub fn is_blockhash_valid(
        &self,
        blockhash: &Hash,
        commitment: CommitmentConfig,
    ) -> ClientResult<bool> {
        self.invoke((self.rpc_client.as_ref()).is_blockhash_valid(blockhash, commitment))
    }

    #[allow(deprecated)]
    pub fn get_fee_for_message(&self, message: &impl SerializableMessage) -> ClientResult<u64> {
        self.invoke((self.rpc_client.as_ref()).get_fee_for_message(message))
    }

    pub fn get_new_latest_blockhash(&self, blockhash: &Hash) -> ClientResult<Hash> {
        self.invoke((self.rpc_client.as_ref()).get_new_latest_blockhash(blockhash))
    }

    pub fn get_transport_stats(&self) -> RpcTransportStats {
        (self.rpc_client.as_ref()).get_transport_stats()
    }

    pub fn get_feature_activation_slot(&self, feature_id: &Pubkey) -> ClientResult<Option<Slot>> {
        self.get_account_with_commitment(feature_id, self.commitment())
            .and_then(|maybe_feature_account| {
                maybe_feature_account
                    .value
                    .map(|feature_account| {
                        bincode::deserialize(feature_account.data()).map_err(|_| {
                            ClientError::from(ErrorKind::Custom(
                                "Failed to deserialize feature account".to_string(),
                            ))
                        })
                    })
                    .transpose()
            })
            .map(|maybe_feature: Option<Feature>| {
                maybe_feature.and_then(|feature| feature.activated_at)
            })
    }

    fn invoke<T, F: std::future::Future<Output = ClientResult<T>>>(&self, f: F) -> ClientResult<T> {
        // `block_on()` panics if called within an asynchronous execution context. Whereas
        // `block_in_place()` only panics if called from a current_thread runtime, which is the
        // lesser evil.
        tokio::task::block_in_place(move || self.runtime.as_ref().expect("runtime").block_on(f))
    }

    pub fn get_inner_client(&self) -> &Arc<nonblocking::rpc_client::RpcClient> {
        &self.rpc_client
    }

    pub fn runtime(&self) -> &tokio::runtime::Runtime {
        self.runtime.as_ref().expect("runtime")
    }
}

/// Mocks for documentation examples
#[doc(hidden)]
pub fn create_rpc_client_mocks() -> crate::mock_sender::Mocks {
    let mut mocks = std::collections::HashMap::new();

    let get_account_request = RpcRequest::GetAccountInfo;
    let get_account_response = serde_json::to_value(Response {
        context: RpcResponseContext {
            slot: 1,
            api_version: None,
        },
        value: {
            let pubkey = Pubkey::from_str("BgvYtJEfmZYdVKiptmMjxGzv8iQoo4MWjsP3QsTkhhxa").unwrap();
            let account = Account {
                lamports: 1_000_000,
                data: vec![],
                owner: pubkey,
                executable: false,
                rent_epoch: 0,
            };
            UiAccount::encode(&pubkey, &account, UiAccountEncoding::Base64, None, None)
        },
    })
    .unwrap();

    mocks.insert(get_account_request, get_account_response);

    mocks
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::mock_sender::PUBKEY,
        assert_matches::assert_matches,
        crossbeam_channel::unbounded,
        jsonrpc_core::{futures::prelude::*, Error, IoHandler, Params},
        jsonrpc_http_server::{AccessControlAllowOrigin, DomainsValidation, ServerBuilder},
        serde_json::{json, Number},
        solana_rpc_client_api::client_error::ErrorKind,
        solana_sdk::{
            instruction::InstructionError,
            signature::{Keypair, Signer},
            system_transaction,
            transaction::TransactionError,
        },
        std::{io, thread},
    };

    #[test]
    fn test_send() {
        _test_send();
    }

    #[tokio::test(flavor = "current_thread")]
    #[should_panic(expected = "can call blocking only when running on the multi-threaded runtime")]
    async fn test_send_async_current_thread() {
        _test_send();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_send_async_multi_thread() {
        _test_send();
    }

    fn _test_send() {
        let (sender, receiver) = unbounded();
        thread::spawn(move || {
            let rpc_addr = "0.0.0.0:0".parse().unwrap();
            let mut io = IoHandler::default();
            // Successful request
            io.add_method("getBalance", |_params: Params| {
                future::ok(Value::Number(Number::from(50)))
            });
            // Failed request
            io.add_method("getRecentBlockhash", |params: Params| {
                if params != Params::None {
                    future::err(Error::invalid_request())
                } else {
                    future::ok(Value::String(
                        "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx".to_string(),
                    ))
                }
            });

            let server = ServerBuilder::new(io)
                .threads(1)
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .start_http(&rpc_addr)
                .expect("Unable to start RPC server");
            sender.send(*server.address()).unwrap();
            server.wait();
        });

        let rpc_addr = receiver.recv().unwrap();
        let rpc_client = RpcClient::new_socket(rpc_addr);

        let balance: u64 = rpc_client
            .send(
                RpcRequest::GetBalance,
                json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"]),
            )
            .unwrap();
        assert_eq!(balance, 50);

        #[allow(deprecated)]
        let blockhash: String = rpc_client
            .send(RpcRequest::GetRecentBlockhash, Value::Null)
            .unwrap();
        assert_eq!(blockhash, "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx");

        // Send erroneous parameter
        #[allow(deprecated)]
        let blockhash: ClientResult<String> =
            rpc_client.send(RpcRequest::GetRecentBlockhash, json!(["parameter"]));
        assert!(blockhash.is_err());
    }

    #[test]
    fn test_send_transaction() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let key = Keypair::new();
        let to = solana_sdk::pubkey::new_rand();
        let blockhash = Hash::default();
        let tx = system_transaction::transfer(&key, &to, 50, blockhash);

        let signature = rpc_client.send_transaction(&tx);
        assert_eq!(signature.unwrap(), tx.signatures[0]);

        let rpc_client = RpcClient::new_mock("fails".to_string());

        let signature = rpc_client.send_transaction(&tx);
        assert!(signature.is_err());

        // Test bad signature returned from rpc node
        let rpc_client = RpcClient::new_mock("malicious".to_string());
        let signature = rpc_client.send_transaction(&tx);
        assert!(signature.is_err());
    }

    #[test]
    fn test_get_recent_blockhash() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let expected_blockhash: Hash = PUBKEY.parse().unwrap();

        let blockhash = rpc_client.get_latest_blockhash().expect("blockhash ok");
        assert_eq!(blockhash, expected_blockhash);

        let rpc_client = RpcClient::new_mock("fails".to_string());

        #[allow(deprecated)]
        let result = rpc_client.get_recent_blockhash();
        assert!(result.is_err());
    }

    #[test]
    fn test_custom_request() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let slot = rpc_client.get_slot().unwrap();
        assert_eq!(slot, 0);

        let custom_slot = rpc_client
            .send::<Slot>(RpcRequest::Custom { method: "getSlot" }, Value::Null)
            .unwrap();

        assert_eq!(slot, custom_slot);
    }

    #[test]
    fn test_get_signature_status() {
        let signature = Signature::default();

        let rpc_client = RpcClient::new_mock("succeeds".to_string());
        let status = rpc_client.get_signature_status(&signature).unwrap();
        assert_eq!(status, Some(Ok(())));

        let rpc_client = RpcClient::new_mock("sig_not_found".to_string());
        let status = rpc_client.get_signature_status(&signature).unwrap();
        assert_eq!(status, None);

        let rpc_client = RpcClient::new_mock("account_in_use".to_string());
        let status = rpc_client.get_signature_status(&signature).unwrap();
        assert_eq!(status, Some(Err(TransactionError::AccountInUse)));
    }

    #[test]
    fn test_send_and_confirm_transaction() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let key = Keypair::new();
        let to = solana_sdk::pubkey::new_rand();
        let blockhash = Hash::default();
        let tx = system_transaction::transfer(&key, &to, 50, blockhash);
        let result = rpc_client.send_and_confirm_transaction(&tx);
        result.unwrap();

        let rpc_client = RpcClient::new_mock("account_in_use".to_string());
        let result = rpc_client.send_and_confirm_transaction(&tx);
        assert!(result.is_err());

        let rpc_client = RpcClient::new_mock("instruction_error".to_string());
        let result = rpc_client.send_and_confirm_transaction(&tx);
        assert_matches!(
            result.unwrap_err().kind(),
            ErrorKind::TransactionError(TransactionError::InstructionError(
                0,
                InstructionError::UninitializedAccount
            ))
        );

        let rpc_client = RpcClient::new_mock("sig_not_found".to_string());
        let result = rpc_client.send_and_confirm_transaction(&tx);
        if let ErrorKind::Io(err) = result.unwrap_err().kind() {
            assert_eq!(err.kind(), io::ErrorKind::Other);
        }
    }

    #[test]
    fn test_rpc_client_thread() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());
        thread::spawn(move || rpc_client);
    }

    // Regression test that the get_block_production_with_config
    // method internally creates the json params array correctly.
    #[test]
    fn get_block_production_with_config_no_error() -> ClientResult<()> {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let config = RpcBlockProductionConfig {
            identity: Some(Keypair::new().pubkey().to_string()),
            range: None,
            commitment: None,
        };

        let prod = rpc_client.get_block_production_with_config(config)?.value;

        assert!(!prod.by_identity.is_empty());

        Ok(())
    }

    #[test]
    fn test_get_latest_blockhash() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let expected_blockhash: Hash = PUBKEY.parse().unwrap();

        let blockhash = rpc_client.get_latest_blockhash().expect("blockhash ok");
        assert_eq!(blockhash, expected_blockhash);

        let rpc_client = RpcClient::new_mock("fails".to_string());

        #[allow(deprecated)]
        let is_err = rpc_client.get_latest_blockhash().is_err();
        assert!(is_err);
    }

    #[test]
    fn test_get_stake_minimum_delegation() {
        let expected_minimum_delegation: u64 = 123_456_789;
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        // Test: without commitment
        {
            let actual_minimum_delegation = rpc_client.get_stake_minimum_delegation().unwrap();
            assert_eq!(expected_minimum_delegation, actual_minimum_delegation);
        }

        // Test: with commitment
        {
            let actual_minimum_delegation = rpc_client
                .get_stake_minimum_delegation_with_commitment(CommitmentConfig::confirmed())
                .unwrap();
            assert_eq!(expected_minimum_delegation, actual_minimum_delegation);
        }
    }

    #[test]
    fn test_get_program_accounts_with_config() {
        let program_id = Pubkey::new_unique();
        let pubkey = Pubkey::new_unique();
        let account = Account {
            lamports: 1_000_000,
            data: vec![],
            owner: program_id,
            executable: false,
            rent_epoch: 0,
        };
        let keyed_account = RpcKeyedAccount {
            pubkey: pubkey.to_string(),
            account: UiAccount::encode(&pubkey, &account, UiAccountEncoding::Base64, None, None),
        };
        let expected_result = vec![(pubkey, account)];
        // Test: without context
        {
            let mocks: Mocks = [(
                RpcRequest::GetProgramAccounts,
                serde_json::to_value(OptionalContext::NoContext(vec![keyed_account.clone()]))
                    .unwrap(),
            )]
            .into_iter()
            .collect();
            let rpc_client = RpcClient::new_mock_with_mocks("mock_client".to_string(), mocks);
            let result = rpc_client
                .get_program_accounts_with_config(
                    &program_id,
                    RpcProgramAccountsConfig {
                        filters: None,
                        account_config: RpcAccountInfoConfig {
                            encoding: Some(UiAccountEncoding::Base64),
                            data_slice: None,
                            commitment: None,
                            min_context_slot: None,
                        },
                        with_context: None,
                    },
                )
                .unwrap();
            assert_eq!(expected_result, result);
        }

        // Test: with context
        {
            let mocks: Mocks = [(
                RpcRequest::GetProgramAccounts,
                serde_json::to_value(OptionalContext::Context(Response {
                    context: RpcResponseContext {
                        slot: 1,
                        api_version: None,
                    },
                    value: vec![keyed_account],
                }))
                .unwrap(),
            )]
            .into_iter()
            .collect();
            let rpc_client = RpcClient::new_mock_with_mocks("mock_client".to_string(), mocks);
            let result = rpc_client
                .get_program_accounts_with_config(
                    &program_id,
                    RpcProgramAccountsConfig {
                        filters: None,
                        account_config: RpcAccountInfoConfig {
                            encoding: Some(UiAccountEncoding::Base64),
                            data_slice: None,
                            commitment: None,
                            min_context_slot: None,
                        },
                        with_context: Some(true),
                    },
                )
                .unwrap();
            assert_eq!(expected_result, result);
        }
    }
}
