use {
    crate::svm_bridge::{
        create_executable_environment, LoadAndExecuteTransactionsOutput, MockBankCallback,
        MockForkGraph, TransactionBatch,
    },
    base64::{prelude::BASE64_STANDARD, Engine},
    bincode::config::Options,
    jsonrpc_core::{types::error, Error, Metadata, Result},
    jsonrpc_derive::rpc,
    log::*,
    serde_json,
    solana_account_decoder::{
        encode_ui_account,
        parse_account_data::{AccountAdditionalDataV2, SplTokenAdditionalData},
        parse_token::{get_token_account_mint, is_known_spl_token_id},
        UiAccount, UiAccountEncoding, UiDataSliceConfig, MAX_BASE58_BYTES,
    },
    solana_accounts_db::blockhash_queue::BlockhashQueue,
    solana_compute_budget::compute_budget::ComputeBudget,
    solana_perf::packet::PACKET_DATA_SIZE,
    solana_program_runtime::loaded_programs::ProgramCacheEntry,
    solana_rpc_client_api::{
        config::*,
        response::{Response as RpcResponse, *},
    },
    solana_sdk::{
        account::{from_account, Account, AccountSharedData, ReadableAccount},
        clock::{Epoch, Slot, MAX_PROCESSING_AGE, MAX_TRANSACTION_FORWARDING_DELAY},
        commitment_config::CommitmentConfig,
        exit::Exit,
        hash::Hash,
        inner_instruction::InnerInstructions,
        message::{
            v0::{LoadedAddresses, MessageAddressTableLookup},
            AddressLoaderError,
        },
        nonce::state::DurableNonce,
        pubkey::Pubkey,
        reserved_account_keys::ReservedAccountKeys,
        signature::Signature,
        system_instruction, sysvar,
        transaction::{
            AddressLoader, MessageHash, SanitizedTransaction, TransactionError,
            VersionedTransaction,
        },
        transaction_context::{TransactionAccount, TransactionReturnData},
    },
    solana_svm::{
        account_loader::{CheckedTransactionDetails, TransactionCheckResult},
        account_overrides::AccountOverrides,
        transaction_error_metrics::TransactionErrorMetrics,
        transaction_processing_result::{
            ProcessedTransaction, TransactionProcessingResultExtensions,
        },
        transaction_processor::{
            ExecutionRecordingConfig, TransactionBatchProcessor, TransactionLogMessages,
            TransactionProcessingConfig, TransactionProcessingEnvironment,
        },
    },
    solana_system_program::system_processor,
    solana_transaction_status::{
        map_inner_instructions, parse_ui_inner_instructions, TransactionBinaryEncoding,
        UiTransactionEncoding,
    },
    solana_vote::vote_account::VoteAccountsHashMap,
    spl_token_2022::{
        extension::{
            interest_bearing_mint::InterestBearingConfig, BaseStateWithExtensions,
            StateWithExtensions,
        },
        state::Mint,
    },
    std::{
        any::type_name,
        cmp::min,
        collections::{HashMap, HashSet},
        fs,
        path::PathBuf,
        str::FromStr,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, RwLock,
        },
    },
};

pub const MAX_REQUEST_BODY_SIZE: usize = 50 * (1 << 10); // 50kB

const EXECUTION_SLOT: u64 = 5; // The execution slot must be greater than the deployment slot
const EXECUTION_EPOCH: u64 = 2; // The execution epoch must be greater than the deployment epoch
const MAX_BASE58_SIZE: usize = 1683; // Golden, bump if PACKET_DATA_SIZE changes
const MAX_BASE64_SIZE: usize = 1644; // Golden, bump if PACKET_DATA_SIZE changes

fn new_response<T>(slot: Slot, value: T) -> RpcResponse<T> {
    RpcResponse {
        context: RpcResponseContext::new(slot),
        value,
    }
}

#[derive(Debug, Default, Clone)]
pub struct JsonRpcConfig {
    pub accounts_path: PathBuf,
    pub ledger_path: PathBuf,
    pub rpc_threads: usize,
    pub rpc_niceness_adj: i8,
    pub max_request_body_size: Option<usize>,
}

#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    account_map: Vec<(Pubkey, AccountSharedData)>,
    #[allow(dead_code)]
    exit: Arc<RwLock<Exit>>,
    transaction_processor: Arc<RwLock<TransactionBatchProcessor<MockForkGraph>>>,
}

struct TransactionSimulationResult {
    pub result: solana_sdk::transaction::Result<()>,
    pub logs: TransactionLogMessages,
    pub post_simulation_accounts: Vec<TransactionAccount>,
    pub units_consumed: u64,
    pub return_data: Option<TransactionReturnData>,
    pub inner_instructions: Option<Vec<InnerInstructions>>,
}

#[derive(Debug, Default, PartialEq)]
pub struct ProcessedTransactionCounts {
    pub processed_transactions_count: u64,
    pub processed_non_vote_transactions_count: u64,
    pub processed_with_successful_result_count: u64,
    pub signature_count: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TransactionLogCollectorFilter {
    All,
    AllWithVotes,
    None,
    OnlyMentionedAddresses,
}

impl Default for TransactionLogCollectorFilter {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Default)]
pub struct TransactionLogCollectorConfig {
    pub mentioned_addresses: HashSet<Pubkey>,
    pub filter: TransactionLogCollectorFilter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionLogInfo {
    pub signature: Signature,
    pub result: solana_sdk::transaction::Result<()>,
    pub is_vote: bool,
    pub log_messages: TransactionLogMessages,
}

#[derive(Default, Debug)]
pub struct TransactionLogCollector {
    // All the logs collected for from this Bank.  Exact contents depend on the
    // active `TransactionLogCollectorFilter`
    pub logs: Vec<TransactionLogInfo>,

    // For each `mentioned_addresses`, maintain a list of indices into `logs` to easily
    // locate the logs from transactions that included the mentioned addresses.
    pub mentioned_address_map: HashMap<Pubkey, Vec<usize>>,
}

impl AddressLoader for JsonRpcRequestProcessor {
    fn load_addresses(
        self,
        _lookups: &[MessageAddressTableLookup],
    ) -> core::result::Result<LoadedAddresses, AddressLoaderError> {
        Ok(LoadedAddresses {
            writable: vec![],
            readonly: vec![],
        })
    }
}

impl Metadata for JsonRpcRequestProcessor {}

impl JsonRpcRequestProcessor {
    pub fn new(config: JsonRpcConfig, exit: Arc<RwLock<Exit>>) -> Self {
        let accounts_json_path = config.accounts_path.clone();
        let accounts_data: String = fs::read_to_string(accounts_json_path).unwrap();
        let accounts_data: serde_json::Value = serde_json::from_str(&accounts_data).unwrap();
        let accounts_slice: Vec<(Pubkey, AccountSharedData)> = accounts_data["accounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|acc| {
                let pubkey = Pubkey::from_str(acc["pubkey"].as_str().unwrap()).unwrap();
                let account = acc["account"].as_object().unwrap();
                let owner = account["owner"].as_str().unwrap();
                let data = account["data"].as_array().unwrap()[0].as_str().unwrap();
                let acc_data = AccountSharedData::from(Account {
                    lamports: account["lamports"].as_u64().unwrap(),
                    data: BASE64_STANDARD.decode(data).unwrap(),
                    owner: Pubkey::from_str(owner).unwrap(),
                    executable: account["executable"].as_bool().unwrap(),
                    rent_epoch: account["rentEpoch"].as_u64().unwrap(),
                });
                (pubkey, acc_data)
            })
            .collect();
        let batch_processor = TransactionBatchProcessor::<MockForkGraph>::new_uninitialized(
            EXECUTION_SLOT,
            EXECUTION_EPOCH,
        );

        Self {
            account_map: accounts_slice,
            exit,
            transaction_processor: Arc::new(RwLock::new(batch_processor)),
        }
    }

    fn get_account_info(
        &self,
        pubkey: &Pubkey,
        config: Option<RpcAccountInfoConfig>,
    ) -> Result<RpcResponse<Option<UiAccount>>> {
        let RpcAccountInfoConfig {
            encoding,
            data_slice,
            commitment: _,
            min_context_slot: _,
        } = config.unwrap_or_default();
        let encoding = encoding.unwrap_or(UiAccountEncoding::Binary);
        Ok(new_response(
            0,
            match self.get_account(pubkey) {
                Some(account) => {
                    debug!("Found account {pubkey:?}");
                    Some(encode_account(&account, pubkey, encoding, data_slice)?)
                }
                None => {
                    debug!("Did not find account {pubkey:?}");
                    None
                }
            },
        ))
    }

    fn get_latest_blockhash(&self, _config: RpcContextConfig) -> Result<RpcResponse<RpcBlockhash>> {
        let blockhash = Hash::default();
        let last_valid_block_height = 0u64;
        Ok(new_response(
            0,
            RpcBlockhash {
                blockhash: blockhash.to_string(),
                last_valid_block_height,
            },
        ))
    }

    fn get_minimum_balance_for_rent_exemption(
        &self,
        _data_len: usize,
        _commitment: Option<CommitmentConfig>,
    ) -> u64 {
        0u64
    }

    fn simulate_transaction_unchecked(
        &self,
        transaction: &SanitizedTransaction,
        enable_cpi_recording: bool,
    ) -> TransactionSimulationResult {
        let mut mock_bank = MockBankCallback::new(self.account_map.clone());
        let transaction_processor = self.transaction_processor.read().unwrap();

        let account_keys = transaction.message().account_keys();
        let number_of_accounts = account_keys.len();
        let account_overrides = AccountOverrides::default();

        let fork_graph = Arc::new(RwLock::new(MockForkGraph {}));

        create_executable_environment(
            fork_graph.clone(),
            &account_keys,
            &mut mock_bank,
            &transaction_processor,
        );

        // Add the system program builtin.
        transaction_processor.add_builtin(
            &mock_bank,
            solana_system_program::id(),
            "system_program",
            ProgramCacheEntry::new_builtin(
                0,
                b"system_program".len(),
                system_processor::Entrypoint::vm,
            ),
        );
        // Add the BPF Loader v2 builtin, for the SPL Token program.
        transaction_processor.add_builtin(
            &mock_bank,
            solana_sdk::bpf_loader_upgradeable::id(),
            "solana_bpf_loader_upgradeable_program",
            ProgramCacheEntry::new_builtin(
                0,
                b"solana_bpf_loader_upgradeable_program".len(),
                solana_bpf_loader_program::Entrypoint::vm,
            ),
        );

        let batch = self.prepare_unlocked_batch_from_single_tx(transaction);
        let LoadAndExecuteTransactionsOutput {
            mut processing_results,
            ..
        } = self.load_and_execute_transactions(
            &mock_bank,
            &batch,
            // After simulation, transactions will need to be forwarded to the leader
            // for processing. During forwarding, the transaction could expire if the
            // delay is not accounted for.
            MAX_PROCESSING_AGE - MAX_TRANSACTION_FORWARDING_DELAY,
            TransactionProcessingConfig {
                account_overrides: Some(&account_overrides),
                check_program_modification_slot: false,
                compute_budget: Some(ComputeBudget::default()),
                log_messages_bytes_limit: None,
                limit_to_load_programs: true,
                recording_config: ExecutionRecordingConfig {
                    enable_cpi_recording,
                    enable_log_recording: true,
                    enable_return_data_recording: true,
                },
                transaction_account_lock_limit: Some(64),
            },
        );

        let processing_result = processing_results
            .pop()
            .unwrap_or(Err(TransactionError::InvalidProgramForExecution));
        let flattened_result = processing_result.flattened_result();
        let (post_simulation_accounts, logs, return_data, inner_instructions) =
            match processing_result {
                Ok(processed_tx) => match processed_tx {
                    ProcessedTransaction::Executed(executed_tx) => {
                        let details = executed_tx.execution_details;
                        let post_simulation_accounts = executed_tx
                            .loaded_transaction
                            .accounts
                            .into_iter()
                            .take(number_of_accounts)
                            .collect::<Vec<_>>();
                        (
                            post_simulation_accounts,
                            details.log_messages,
                            details.return_data,
                            details.inner_instructions,
                        )
                    }
                    ProcessedTransaction::FeesOnly(_) => (vec![], None, None, None),
                },
                Err(_) => (vec![], None, None, None),
            };
        let logs = logs.unwrap_or_default();
        let units_consumed: u64 = 0;

        TransactionSimulationResult {
            result: flattened_result,
            logs,
            post_simulation_accounts,
            units_consumed,
            return_data,
            inner_instructions,
        }
    }

    fn prepare_unlocked_batch_from_single_tx<'a>(
        &'a self,
        transaction: &'a SanitizedTransaction,
    ) -> TransactionBatch<'_> {
        let tx_account_lock_limit = solana_sdk::transaction::MAX_TX_ACCOUNT_LOCKS;
        let lock_result = transaction
            .get_account_locks(tx_account_lock_limit)
            .map(|_| ());
        let batch = TransactionBatch::new(
            vec![lock_result],
            std::borrow::Cow::Borrowed(std::slice::from_ref(transaction)),
        );
        batch
    }

    fn check_age(
        &self,
        sanitized_txs: &[impl core::borrow::Borrow<SanitizedTransaction>],
        lock_results: &[solana_sdk::transaction::Result<()>],
        max_age: usize,
        error_counters: &mut TransactionErrorMetrics,
    ) -> Vec<TransactionCheckResult> {
        let last_blockhash = Hash::default();
        let blockhash_queue = BlockhashQueue::default();
        let next_durable_nonce = DurableNonce::from_blockhash(&last_blockhash);

        sanitized_txs
            .iter()
            .zip(lock_results)
            .map(|(tx, lock_res)| match lock_res {
                Ok(()) => self.check_transaction_age(
                    tx.borrow(),
                    max_age,
                    &next_durable_nonce,
                    &blockhash_queue,
                    error_counters,
                ),
                Err(e) => Err(e.clone()),
            })
            .collect()
    }

    fn check_transaction_age(
        &self,
        _tx: &SanitizedTransaction,
        _max_age: usize,
        _next_durable_nonce: &DurableNonce,
        _hash_queue: &BlockhashQueue,
        _error_counters: &mut TransactionErrorMetrics,
    ) -> TransactionCheckResult {
        /* for now just return defaults */
        Ok(CheckedTransactionDetails {
            nonce: None,
            lamports_per_signature: u64::default(),
        })
    }

    fn clock(&self) -> sysvar::clock::Clock {
        from_account(&self.get_account(&sysvar::clock::id()).unwrap_or_default())
            .unwrap_or_default()
    }

    fn epoch_total_stake(&self, _epoch: Epoch) -> Option<u64> {
        Some(u64::default())
    }

    fn epoch_vote_accounts(&self, _epoch: Epoch) -> Option<&VoteAccountsHashMap> {
        None
    }

    fn get_account(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        let account_map: HashMap<Pubkey, AccountSharedData> =
            HashMap::from_iter(self.account_map.clone());
        account_map.get(pubkey).cloned()
    }

    fn get_additional_mint_data(&self, data: &[u8]) -> Result<SplTokenAdditionalData> {
        StateWithExtensions::<Mint>::unpack(data)
            .map_err(|_| {
                Error::invalid_params("Invalid param: Token mint could not be unpacked".to_string())
            })
            .map(|mint| {
                let interest_bearing_config = mint
                    .get_extension::<InterestBearingConfig>()
                    .map(|x| (*x, self.clock().unix_timestamp))
                    .ok();
                SplTokenAdditionalData {
                    decimals: mint.base.decimals,
                    interest_bearing_config,
                }
            })
    }

    fn get_encoded_account(
        &self,
        pubkey: &Pubkey,
        encoding: UiAccountEncoding,
        data_slice: Option<UiDataSliceConfig>,
        // only used for simulation results
        overwrite_accounts: Option<&HashMap<Pubkey, AccountSharedData>>,
    ) -> Result<Option<UiAccount>> {
        match overwrite_accounts
            .and_then(|accounts| accounts.get(pubkey).cloned())
            .or_else(|| self.get_account(pubkey))
        {
            Some(account) => {
                let response = if is_known_spl_token_id(account.owner())
                    && encoding == UiAccountEncoding::JsonParsed
                {
                    self.get_parsed_token_account(pubkey, account, overwrite_accounts)
                } else {
                    encode_account(&account, pubkey, encoding, data_slice)?
                };
                Ok(Some(response))
            }
            None => Ok(None),
        }
    }

    fn get_parsed_token_account(
        &self,
        pubkey: &Pubkey,
        account: AccountSharedData,
        // only used for simulation results
        overwrite_accounts: Option<&HashMap<Pubkey, AccountSharedData>>,
    ) -> UiAccount {
        let additional_data = get_token_account_mint(account.data())
            .and_then(|mint_pubkey| {
                overwrite_accounts
                    .and_then(|accounts| accounts.get(&mint_pubkey).cloned())
                    .or_else(|| self.get_account(&mint_pubkey))
            })
            .and_then(|mint_account| self.get_additional_mint_data(mint_account.data()).ok())
            .map(|data| AccountAdditionalDataV2 {
                spl_token_additional_data: Some(data),
            });

        encode_ui_account(
            pubkey,
            &account,
            UiAccountEncoding::JsonParsed,
            additional_data,
            None,
        )
    }

    fn last_blockhash_and_lamports_per_signature(&self) -> (Hash, u64) {
        let last_hash = Hash::default();
        let last_lamports_per_signature = u64::default();
        (last_hash, last_lamports_per_signature)
    }

    fn load_and_execute_transactions(
        &self,
        bank: &MockBankCallback,
        batch: &TransactionBatch,
        max_age: usize,
        processing_config: TransactionProcessingConfig,
    ) -> LoadAndExecuteTransactionsOutput {
        let sanitized_txs = batch.sanitized_transactions();
        debug!("processing transactions: {}", sanitized_txs.len());
        let mut error_counters = TransactionErrorMetrics::default();

        let check_results = self.check_age(
            sanitized_txs,
            batch.lock_results(),
            max_age,
            &mut error_counters,
        );

        let (blockhash, lamports_per_signature) = self.last_blockhash_and_lamports_per_signature();
        let processing_environment = TransactionProcessingEnvironment {
            blockhash,
            epoch_total_stake: self.epoch_total_stake(Epoch::default()),
            epoch_vote_accounts: self.epoch_vote_accounts(Epoch::default()),
            feature_set: Arc::clone(&bank.feature_set),
            fee_structure: None,
            lamports_per_signature,
            rent_collector: None,
        };

        let sanitized_output = self
            .transaction_processor
            .read()
            .unwrap()
            .load_and_execute_sanitized_transactions(
                bank,
                sanitized_txs,
                check_results,
                &processing_environment,
                &processing_config,
            );

        let err_count = &mut error_counters.total;

        let mut processed_counts = ProcessedTransactionCounts::default();
        for (processing_result, tx) in sanitized_output
            .processing_results
            .iter()
            .zip(sanitized_txs)
        {
            if processing_result.was_processed() {
                // Signature count must be accumulated only if the transaction
                // is processed, otherwise a mismatched count between banking
                // and replay could occur
                processed_counts.signature_count +=
                    u64::from(tx.message().header().num_required_signatures);
                processed_counts.processed_transactions_count += 1;

                if !tx.is_simple_vote_transaction() {
                    processed_counts.processed_non_vote_transactions_count += 1;
                }
            }

            match processing_result.flattened_result() {
                Ok(()) => {
                    processed_counts.processed_with_successful_result_count += 1;
                }
                Err(err) => {
                    if *err_count == 0 {
                        debug!("tx error: {:?} {:?}", err, tx);
                    }
                    *err_count += 1;
                }
            }
        }

        LoadAndExecuteTransactionsOutput {
            processing_results: sanitized_output.processing_results,
        }
    }
}

/// RPC interface that an API node is expected to provide
pub mod rpc {
    use super::*;
    #[rpc]
    pub trait Rpc {
        type Metadata;

        #[rpc(meta, name = "getAccountInfo")]
        fn get_account_info(
            &self,
            meta: Self::Metadata,
            pubkey_str: String,
            config: Option<RpcAccountInfoConfig>,
        ) -> Result<RpcResponse<Option<UiAccount>>>;

        #[rpc(meta, name = "getLatestBlockhash")]
        fn get_latest_blockhash(
            &self,
            meta: Self::Metadata,
            config: Option<RpcContextConfig>,
        ) -> Result<RpcResponse<RpcBlockhash>>;

        #[rpc(meta, name = "getMinimumBalanceForRentExemption")]
        fn get_minimum_balance_for_rent_exemption(
            &self,
            meta: Self::Metadata,
            data_len: usize,
            commitment: Option<CommitmentConfig>,
        ) -> Result<u64>;

        #[rpc(meta, name = "getVersion")]
        fn get_version(&self, meta: Self::Metadata) -> Result<RpcVersionInfo>;

        #[rpc(meta, name = "simulateTransaction")]
        fn simulate_transaction(
            &self,
            meta: Self::Metadata,
            data: String,
            config: Option<RpcSimulateTransactionConfig>,
        ) -> Result<RpcResponse<RpcSimulateTransactionResult>>;
    }

    pub struct RpcImpl;

    impl Rpc for RpcImpl {
        type Metadata = JsonRpcRequestProcessor;

        fn simulate_transaction(
            &self,
            meta: Self::Metadata,
            data: String,
            config: Option<RpcSimulateTransactionConfig>,
        ) -> Result<RpcResponse<RpcSimulateTransactionResult>> {
            debug!("simulate_transaction rpc request received");

            let RpcSimulateTransactionConfig {
                sig_verify: _,
                replace_recent_blockhash: _,
                commitment: _,
                encoding,
                accounts: config_accounts,
                min_context_slot: _,
                inner_instructions: enable_cpi_recording,
            } = config.unwrap_or_default();
            let tx_encoding = encoding.unwrap_or(UiTransactionEncoding::Base58);
            let binary_encoding = tx_encoding.into_binary_encoding().ok_or_else(|| {
                Error::invalid_params(format!(
                    "unsupported encoding: {tx_encoding}. Supported encodings: base58, base64"
                ))
            })?;
            let (_, unsanitized_tx) =
                decode_and_deserialize::<VersionedTransaction>(data, binary_encoding)?;
            debug!("unsanitized transaction decoded {:?}", unsanitized_tx);

            let transaction = sanitize_transaction(
                unsanitized_tx,
                meta.clone(),
                &ReservedAccountKeys::default().active,
            )?;

            let TransactionSimulationResult {
                result,
                logs,
                post_simulation_accounts,
                units_consumed,
                return_data,
                inner_instructions,
            } = meta.simulate_transaction_unchecked(&transaction, enable_cpi_recording);

            let account_keys = transaction.message().account_keys();
            let number_of_accounts = account_keys.len();

            let accounts = if let Some(config_accounts) = config_accounts {
                let accounts_encoding = config_accounts
                    .encoding
                    .unwrap_or(UiAccountEncoding::Base64);

                if accounts_encoding == UiAccountEncoding::Binary
                    || accounts_encoding == UiAccountEncoding::Base58
                {
                    return Err(Error::invalid_params("base58 encoding not supported"));
                }

                if config_accounts.addresses.len() > number_of_accounts {
                    return Err(Error::invalid_params(format!(
                        "Too many accounts provided; max {number_of_accounts}"
                    )));
                }

                if result.is_err() {
                    Some(vec![None; config_accounts.addresses.len()])
                } else {
                    let mut post_simulation_accounts_map = HashMap::new();
                    for (pubkey, data) in post_simulation_accounts {
                        post_simulation_accounts_map.insert(pubkey, data);
                    }

                    Some(
                        config_accounts
                            .addresses
                            .iter()
                            .map(|address_str| {
                                let pubkey = verify_pubkey(address_str)?;
                                meta.get_encoded_account(
                                    &pubkey,
                                    accounts_encoding,
                                    None,
                                    Some(&post_simulation_accounts_map),
                                )
                            })
                            .collect::<Result<Vec<_>>>()?,
                    )
                }
            } else {
                None
            };

            let inner_instructions = inner_instructions.map(|info| {
                map_inner_instructions(info)
                    .map(|converted| parse_ui_inner_instructions(converted, &account_keys))
                    .collect()
            });

            Ok(new_response(
                0,
                RpcSimulateTransactionResult {
                    err: result.err(),
                    logs: Some(logs),
                    accounts,
                    units_consumed: Some(units_consumed),
                    return_data: return_data.map(|return_data| return_data.into()),
                    inner_instructions,
                    replacement_blockhash: None,
                },
            ))
        }

        fn get_account_info(
            &self,
            meta: Self::Metadata,
            pubkey_str: String,
            config: Option<RpcAccountInfoConfig>,
        ) -> Result<RpcResponse<Option<UiAccount>>> {
            debug!("get_account_info rpc request received: {:?}", pubkey_str);
            let pubkey = verify_pubkey(&pubkey_str)?;
            debug!("pubkey {pubkey:?} verified.");
            meta.get_account_info(&pubkey, config)
        }

        fn get_latest_blockhash(
            &self,
            meta: Self::Metadata,
            config: Option<RpcContextConfig>,
        ) -> Result<RpcResponse<RpcBlockhash>> {
            debug!("get_latest_blockhash rpc request received");
            meta.get_latest_blockhash(config.unwrap_or_default())
        }

        fn get_minimum_balance_for_rent_exemption(
            &self,
            meta: Self::Metadata,
            data_len: usize,
            commitment: Option<CommitmentConfig>,
        ) -> Result<u64> {
            debug!(
                "get_minimum_balance_for_rent_exemption rpc request received: {:?}",
                data_len
            );
            if data_len as u64 > system_instruction::MAX_PERMITTED_DATA_LENGTH {
                return Err(Error::invalid_request());
            }
            Ok(meta.get_minimum_balance_for_rent_exemption(data_len, commitment))
        }

        fn get_version(&self, _: Self::Metadata) -> Result<RpcVersionInfo> {
            debug!("get_version rpc request received");
            let version = solana_version::Version::default();
            Ok(RpcVersionInfo {
                solana_core: version.to_string(),
                feature_set: Some(version.feature_set),
            })
        }
    }
}

pub fn create_exit(exit: Arc<AtomicBool>) -> Arc<RwLock<Exit>> {
    let mut exit_handler = Exit::default();
    exit_handler.register_exit(Box::new(move || exit.store(true, Ordering::Relaxed)));
    Arc::new(RwLock::new(exit_handler))
}

fn decode_and_deserialize<T>(
    encoded: String,
    encoding: TransactionBinaryEncoding,
) -> Result<(Vec<u8>, T)>
where
    T: serde::de::DeserializeOwned,
{
    let wire_output = match encoding {
        TransactionBinaryEncoding::Base58 => {
            if encoded.len() > MAX_BASE58_SIZE {
                return Err(Error::invalid_params(format!(
                    "base58 encoded {} too large: {} bytes (max: encoded/raw {}/{})",
                    type_name::<T>(),
                    encoded.len(),
                    MAX_BASE58_SIZE,
                    PACKET_DATA_SIZE,
                )));
            }
            bs58::decode(encoded)
                .into_vec()
                .map_err(|e| Error::invalid_params(format!("invalid base58 encoding: {e:?}")))?
        }
        TransactionBinaryEncoding::Base64 => {
            if encoded.len() > MAX_BASE64_SIZE {
                return Err(Error::invalid_params(format!(
                    "base64 encoded {} too large: {} bytes (max: encoded/raw {}/{})",
                    type_name::<T>(),
                    encoded.len(),
                    MAX_BASE64_SIZE,
                    PACKET_DATA_SIZE,
                )));
            }
            BASE64_STANDARD
                .decode(encoded)
                .map_err(|e| Error::invalid_params(format!("invalid base64 encoding: {e:?}")))?
        }
    };
    if wire_output.len() > PACKET_DATA_SIZE {
        return Err(Error::invalid_params(format!(
            "decoded {} too large: {} bytes (max: {} bytes)",
            type_name::<T>(),
            wire_output.len(),
            PACKET_DATA_SIZE
        )));
    }
    bincode::options()
        .with_limit(PACKET_DATA_SIZE as u64)
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .deserialize_from(&wire_output[..])
        .map_err(|err| {
            Error::invalid_params(format!(
                "failed to deserialize {}: {}",
                type_name::<T>(),
                &err.to_string()
            ))
        })
        .map(|output| (wire_output, output))
}

fn encode_account<T: ReadableAccount>(
    account: &T,
    pubkey: &Pubkey,
    encoding: UiAccountEncoding,
    data_slice: Option<UiDataSliceConfig>,
) -> Result<UiAccount> {
    if (encoding == UiAccountEncoding::Binary || encoding == UiAccountEncoding::Base58)
        && data_slice
            .map(|s| min(s.length, account.data().len().saturating_sub(s.offset)))
            .unwrap_or(account.data().len())
            > MAX_BASE58_BYTES
    {
        let message = format!("Encoded binary (base 58) data should be less than {MAX_BASE58_BYTES} bytes, please use Base64 encoding.");
        Err(error::Error {
            code: error::ErrorCode::InvalidRequest,
            message,
            data: None,
        })
    } else {
        Ok(encode_ui_account(
            pubkey, account, encoding, None, data_slice,
        ))
    }
}

fn sanitize_transaction(
    transaction: VersionedTransaction,
    address_loader: impl AddressLoader,
    reserved_account_keys: &HashSet<Pubkey>,
) -> Result<SanitizedTransaction> {
    SanitizedTransaction::try_create(
        transaction,
        MessageHash::Compute,
        None,
        address_loader,
        reserved_account_keys,
    )
    .map_err(|err| Error::invalid_params(format!("invalid transaction: {err}")))
}

fn verify_pubkey(input: &str) -> Result<Pubkey> {
    input
        .parse()
        .map_err(|e| Error::invalid_params(format!("Invalid param: {e:?}")))
}
