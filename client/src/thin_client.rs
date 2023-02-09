//! The `thin_client` module is a client-side object that interfaces with
//! a server-side TPU.  Client code should use this object instead of writing
//! messages to the network directly. The binary encoding of its messages are
//! unstable and may change in future releases.

use {
    crate::connection_cache::ConnectionCache,
    solana_quic_client::{QuicConfig, QuicConnectionManager, QuicPool},
    solana_rpc_client::rpc_client::RpcClient,
    solana_rpc_client_api::config::RpcProgramAccountsConfig,
    solana_sdk::{
        account::Account,
        client::{AsyncClient, Client, SyncClient},
        clock::Slot,
        commitment_config::CommitmentConfig,
        epoch_info::EpochInfo,
        fee_calculator::{FeeCalculator, FeeRateGovernor},
        hash::Hash,
        instruction::Instruction,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signature},
        signers::Signers,
        transaction::{self, Transaction, VersionedTransaction},
        transport::Result as TransportResult,
    },
    solana_thin_client::thin_client::ThinClient as BackendThinClient,
    solana_udp_client::{UdpConfig, UdpConnectionManager, UdpPool},
    std::{net::SocketAddr, sync::Arc, time::Duration},
};

/// A thin wrapper over thin-client/ThinClient to ease
/// construction of the ThinClient for code dealing both with udp and quic.
/// For the scenario only using udp or quic, use thin-client/ThinClient directly.
pub enum ThinClient {
    Quic(BackendThinClient<QuicPool, QuicConnectionManager, QuicConfig>),
    Udp(BackendThinClient<UdpPool, UdpConnectionManager, UdpConfig>),
}

/// Macros easing the forwarding calls to the BackendThinClient
macro_rules! dispatch {
    /*  Regular version */
    ($vis:vis fn $name:ident(&self $(, $arg:ident : $ty:ty)*) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&self $(, $arg:$ty)*) $(-> $out)? {
            match self {
                Self::Quic(this) => this.$name($($arg),*),
                Self::Udp(this) => this.$name($($arg),*),
            }
        }
    };

    /* The self is a mut */
    ($vis:vis fn $name:ident(&mut self $(, $arg:ident : $ty:ty)*) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&mut self $(, $arg:$ty)*) $(-> $out)? {
            match self {
                Self::Quic(this) => this.$name($($arg),*),
                Self::Udp(this) => this.$name($($arg),*),
            }
        }
    };

    /* There is a type parameter */
    ($vis:vis fn $name:ident<T: Signers>(&self $(, $arg:ident : $ty:ty)*) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name<T: Signers>(&self $(, $arg:$ty)*) $(-> $out)? {
            match self {
                Self::Quic(this) => this.$name($($arg),*),
                Self::Udp(this) => this.$name($($arg),*),
            }
        }
    };
}

/// Macro forwarding calls to BackendThinClient with deprecated functions
macro_rules! dispatch_allow_deprecated {
    ($vis:vis fn $name:ident(&self $(, $arg:ident : $ty:ty)*) $(-> $out:ty)?) => {
        #[inline]
        $vis fn $name(&self $(, $arg:$ty)*) $(-> $out)? {
            match self {
                Self::Quic(this) => {
                    #[allow(deprecated)]
                    this.$name($($arg),*)
                }
                Self::Udp(this) => {
                    #[allow(deprecated)]
                    this.$name($($arg),*)
                }
            }
        }
    };
}

impl ThinClient {
    /// Create a new ThinClient that will interface with the Rpc at `rpc_addr` using TCP
    /// and the Tpu at `tpu_addr` over `transactions_socket` using Quic or UDP
    /// (currently hardcoded to UDP)
    pub fn new(
        rpc_addr: SocketAddr,
        tpu_addr: SocketAddr,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        match &*connection_cache {
            ConnectionCache::Quic(connection_cache) => {
                let thin_client =
                    BackendThinClient::new(rpc_addr, tpu_addr, connection_cache.clone());
                ThinClient::Quic(thin_client)
            }
            ConnectionCache::Udp(connection_cache) => {
                let thin_client =
                    BackendThinClient::new(rpc_addr, tpu_addr, connection_cache.clone());
                ThinClient::Udp(thin_client)
            }
        }
    }

    pub fn new_socket_with_timeout(
        rpc_addr: SocketAddr,
        tpu_addr: SocketAddr,
        timeout: Duration,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        match &*connection_cache {
            ConnectionCache::Quic(connection_cache) => {
                let thin_client = BackendThinClient::new_socket_with_timeout(
                    rpc_addr,
                    tpu_addr,
                    timeout,
                    connection_cache.clone(),
                );
                ThinClient::Quic(thin_client)
            }
            ConnectionCache::Udp(connection_cache) => {
                let thin_client = BackendThinClient::new_socket_with_timeout(
                    rpc_addr,
                    tpu_addr,
                    timeout,
                    connection_cache.clone(),
                );
                ThinClient::Udp(thin_client)
            }
        }
    }

    pub fn new_from_addrs(
        rpc_addrs: Vec<SocketAddr>,
        tpu_addrs: Vec<SocketAddr>,
        connection_cache: Arc<ConnectionCache>,
    ) -> Self {
        match &*connection_cache {
            ConnectionCache::Quic(connection_cache) => {
                let thin_client = BackendThinClient::new_from_addrs(
                    rpc_addrs,
                    tpu_addrs,
                    connection_cache.clone(),
                );
                ThinClient::Quic(thin_client)
            }
            ConnectionCache::Udp(connection_cache) => {
                let thin_client = BackendThinClient::new_from_addrs(
                    rpc_addrs,
                    tpu_addrs,
                    connection_cache.clone(),
                );
                ThinClient::Udp(thin_client)
            }
        }
    }

    dispatch!(pub fn rpc_client(&self) -> &RpcClient);

    dispatch!(pub fn retry_transfer_until_confirmed(&self, keypair: &Keypair, transaction: &mut Transaction, tries: usize, min_confirmed_blocks: usize) -> TransportResult<Signature>);

    dispatch!(pub fn retry_transfer(
        &self,
        keypair: &Keypair,
        transaction: &mut Transaction,
        tries: usize
    ) -> TransportResult<Signature>);

    dispatch!(pub fn send_and_confirm_transaction<T: Signers>(
        &self,
        keypairs: &T,
        transaction: &mut Transaction,
        tries: usize,
        pending_confirmations: usize
    ) -> TransportResult<Signature>);

    dispatch!(pub fn poll_get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64>);

    dispatch!(pub fn poll_get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig
    ) -> TransportResult<u64>);

    dispatch!(pub fn wait_for_balance(&self, pubkey: &Pubkey, expected_balance: Option<u64>) -> Option<u64>);

    dispatch!(pub fn get_program_accounts_with_config(
        &self,
        pubkey: &Pubkey,
        config: RpcProgramAccountsConfig
    ) -> TransportResult<Vec<(Pubkey, Account)>>);

    dispatch!(pub fn wait_for_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        expected_balance: Option<u64>,
        commitment_config: CommitmentConfig
    ) -> Option<u64>);

    dispatch!(pub fn poll_for_signature_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig
    ) -> TransportResult<()>);

    dispatch!(pub fn get_num_blocks_since_signature_confirmation(
        &mut self,
        sig: &Signature
    ) -> TransportResult<usize>);
}

impl Client for ThinClient {
    dispatch!(fn tpu_addr(&self) -> String);
}

impl SyncClient for ThinClient {
    dispatch!(fn send_and_confirm_message<T: Signers>(
        &self,
        keypairs: &T,
        message: Message
    ) -> TransportResult<Signature>);

    dispatch!(fn send_and_confirm_instruction(
        &self,
        keypair: &Keypair,
        instruction: Instruction
    ) -> TransportResult<Signature>);

    dispatch!(fn transfer_and_confirm(
        &self,
        lamports: u64,
        keypair: &Keypair,
        pubkey: &Pubkey
    ) -> TransportResult<Signature>);

    dispatch!(fn get_account_data(&self, pubkey: &Pubkey) -> TransportResult<Option<Vec<u8>>>);

    dispatch!(fn get_account(&self, pubkey: &Pubkey) -> TransportResult<Option<Account>>);

    dispatch!(fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig
    ) -> TransportResult<Option<Account>>);

    dispatch!(fn get_balance(&self, pubkey: &Pubkey) -> TransportResult<u64>);

    dispatch!(fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig
    ) -> TransportResult<u64>);

    dispatch!(fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> TransportResult<u64>);

    dispatch_allow_deprecated!(fn get_recent_blockhash(&self) -> TransportResult<(Hash, FeeCalculator)>);

    dispatch_allow_deprecated!(fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig
    ) -> TransportResult<(Hash, FeeCalculator, Slot)>);

    dispatch_allow_deprecated!(fn get_fee_calculator_for_blockhash(
        &self,
        blockhash: &Hash
    ) -> TransportResult<Option<FeeCalculator>>);

    dispatch_allow_deprecated!(fn get_fee_rate_governor(&self) -> TransportResult<FeeRateGovernor>);

    dispatch!(fn get_signature_status(
        &self,
        signature: &Signature
    ) -> TransportResult<Option<transaction::Result<()>>>);

    dispatch!(fn get_signature_status_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig
    ) -> TransportResult<Option<transaction::Result<()>>>);

    dispatch!(fn get_slot(&self) -> TransportResult<u64>);

    dispatch!(fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig
    ) -> TransportResult<u64>);

    dispatch!(fn get_epoch_info(&self) -> TransportResult<EpochInfo>);

    dispatch!(fn get_transaction_count(&self) -> TransportResult<u64>);

    dispatch!(fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig
    ) -> TransportResult<u64>);

    dispatch!(fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize
    ) -> TransportResult<usize>);

    dispatch!(fn poll_for_signature(&self, signature: &Signature) -> TransportResult<()>);

    dispatch_allow_deprecated!(fn get_new_blockhash(&self, blockhash: &Hash) -> TransportResult<(Hash, FeeCalculator)>);

    dispatch!(fn get_latest_blockhash(&self) -> TransportResult<Hash>);

    dispatch!(fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig
    ) -> TransportResult<(Hash, u64)>);

    dispatch!(fn is_blockhash_valid(
        &self,
        blockhash: &Hash,
        commitment_config: CommitmentConfig
    ) -> TransportResult<bool>);

    dispatch!(fn get_fee_for_message(&self, message: &Message) -> TransportResult<u64>);
}

impl AsyncClient for ThinClient {
    dispatch!(fn async_send_versioned_transaction(
        &self,
        transaction: VersionedTransaction
    ) -> TransportResult<Signature>);

    dispatch!(fn async_send_versioned_transaction_batch(
        &self,
        batch: Vec<VersionedTransaction>
    ) -> TransportResult<()>);
}
