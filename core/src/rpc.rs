//! The `rpc` module implements the Solana RPC interface.

use crate::{
    cluster_info::ClusterInfo,
    commitment::{BlockCommitmentArray, BlockCommitmentCache},
    contact_info::ContactInfo,
    non_circulating_supply::calculate_non_circulating_supply,
    rpc_error::RpcCustomError,
    validator::ValidatorExit,
};
use bincode::serialize;
use jsonrpc_core::{Error, Metadata, Result};
use jsonrpc_derive::rpc;
use solana_client::{
    rpc_config::*,
    rpc_request::{
        MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS_SLOT_RANGE,
        MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS, NUM_LARGEST_ACCOUNTS,
    },
    rpc_response::*,
};
use solana_faucet::faucet::request_airdrop_transaction;
use solana_ledger::{
    bank_forks::BankForks, blockstore::Blockstore, blockstore_db::BlockstoreError,
};
use solana_perf::packet::PACKET_DATA_SIZE;
use solana_runtime::{accounts::AccountAddressFilter, bank::Bank};
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    commitment_config::{CommitmentConfig, CommitmentLevel},
    epoch_info::EpochInfo,
    epoch_schedule::EpochSchedule,
    hash::Hash,
    inflation::Inflation,
    pubkey::Pubkey,
    signature::Signature,
    timing::slot_duration_from_slots_per_year,
    transaction::{self, Transaction},
};
use solana_transaction_status::{
    ConfirmedBlock, ConfirmedTransaction, TransactionEncoding, TransactionStatus,
};
use solana_vote_program::vote_state::{VoteState, MAX_LOCKOUT_HISTORY};
use std::{
    cmp::{max, min},
    collections::{HashMap, HashSet},
    net::{SocketAddr, UdpSocket},
    str::FromStr,
    sync::{Arc, RwLock},
    thread::sleep,
    time::{Duration, Instant},
};

type RpcResponse<T> = Result<Response<T>>;

fn new_response<T>(bank: &Bank, value: T) -> RpcResponse<T> {
    let context = RpcResponseContext { slot: bank.slot() };
    Ok(Response { context, value })
}

#[derive(Debug, Default, Clone)]
pub struct JsonRpcConfig {
    pub enable_validator_exit: bool,
    pub enable_set_log_filter: bool,
    pub enable_rpc_transaction_history: bool,
    pub identity_pubkey: Pubkey,
    pub faucet_addr: Option<SocketAddr>,
}

#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    blockstore: Arc<Blockstore>,
    config: JsonRpcConfig,
    validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
}

impl JsonRpcRequestProcessor {
    fn bank(&self, commitment: Option<CommitmentConfig>) -> Result<Arc<Bank>> {
        debug!("RPC commitment_config: {:?}", commitment);
        let r_bank_forks = self.bank_forks.read().unwrap();

        match commitment {
            Some(commitment_config) if commitment_config.commitment == CommitmentLevel::Recent => {
                let bank = r_bank_forks.working_bank();
                debug!("RPC using working_bank: {:?}", bank.slot());
                Ok(bank)
            }
            Some(commitment_config) if commitment_config.commitment == CommitmentLevel::Root => {
                let slot = r_bank_forks.root();
                debug!("RPC using node root: {:?}", slot);
                Ok(r_bank_forks.get(slot).cloned().unwrap())
            }
            Some(commitment_config) if commitment_config.commitment == CommitmentLevel::Single => {
                let slot = self
                    .block_commitment_cache
                    .read()
                    .unwrap()
                    .highest_confirmed_slot();
                debug!("RPC using confirmed slot: {:?}", slot);
                Ok(r_bank_forks.get(slot).cloned().unwrap())
            }
            _ => {
                let cluster_root = self
                    .block_commitment_cache
                    .read()
                    .unwrap()
                    .largest_confirmed_root();
                debug!("RPC using block: {:?}", cluster_root);
                r_bank_forks.get(cluster_root).cloned().ok_or_else(|| {
                    RpcCustomError::NonexistentClusterRoot {
                        cluster_root,
                        node_root: r_bank_forks.root(),
                    }
                    .into()
                })
            }
        }
    }

    pub fn new(
        config: JsonRpcConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        blockstore: Arc<Blockstore>,
        validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
    ) -> Self {
        JsonRpcRequestProcessor {
            config,
            bank_forks,
            block_commitment_cache,
            blockstore,
            validator_exit,
        }
    }

    pub fn get_account_info(
        &self,
        pubkey: Result<Pubkey>,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<Option<RpcAccount>> {
        let bank = &*self.bank(commitment)?;
        pubkey.and_then(|key| new_response(bank, bank.get_account(&key).map(RpcAccount::encode)))
    }

    pub fn get_minimum_balance_for_rent_exemption(
        &self,
        data_len: usize,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64> {
        Ok(self
            .bank(commitment)?
            .get_minimum_balance_for_rent_exemption(data_len))
    }

    pub fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Vec<RpcKeyedAccount>> {
        Ok(self
            .bank(commitment)?
            .get_program_accounts(Some(&program_id))
            .into_iter()
            .map(|(pubkey, account)| RpcKeyedAccount {
                pubkey: pubkey.to_string(),
                account: RpcAccount::encode(account),
            })
            .collect())
    }

    pub fn get_inflation(&self, commitment: Option<CommitmentConfig>) -> Result<Inflation> {
        Ok(self.bank(commitment)?.inflation())
    }

    pub fn get_epoch_schedule(&self) -> Result<EpochSchedule> {
        // Since epoch schedule data comes from the genesis config, any commitment level should be
        // fine
        Ok(*self.bank(Some(CommitmentConfig::root()))?.epoch_schedule())
    }

    pub fn get_balance(
        &self,
        pubkey: Result<Pubkey>,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<u64> {
        let bank = &*self.bank(commitment)?;
        pubkey.and_then(|key| new_response(bank, bank.get_balance(&key)))
    }

    fn get_recent_blockhash(
        &self,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<RpcBlockhashFeeCalculator> {
        let bank = &*self.bank(commitment)?;
        let (blockhash, fee_calculator) = bank.confirmed_last_blockhash();
        new_response(
            bank,
            RpcBlockhashFeeCalculator {
                blockhash: blockhash.to_string(),
                fee_calculator,
            },
        )
    }

    fn get_fee_calculator_for_blockhash(
        &self,
        blockhash: &Hash,
    ) -> RpcResponse<Option<RpcFeeCalculator>> {
        let bank = &*self.bank(None)?;
        let fee_calculator = bank.get_fee_calculator(blockhash);
        new_response(
            bank,
            fee_calculator.map(|fee_calculator| RpcFeeCalculator { fee_calculator }),
        )
    }

    fn get_fee_rate_governor(&self) -> RpcResponse<RpcFeeRateGovernor> {
        let bank = &*self.bank(None)?;
        let fee_rate_governor = bank.get_fee_rate_governor();
        new_response(
            bank,
            RpcFeeRateGovernor {
                fee_rate_governor: fee_rate_governor.clone(),
            },
        )
    }

    pub fn confirm_transaction(
        &self,
        signature: Result<Signature>,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<bool> {
        let bank = &*self.bank(commitment)?;
        match signature {
            Err(e) => Err(e),
            Ok(sig) => {
                let status = bank.get_signature_status(&sig);
                match status {
                    Some(status) => new_response(bank, status.is_ok()),
                    None => new_response(bank, false),
                }
            }
        }
    }

    fn get_block_commitment(&self, block: Slot) -> RpcBlockCommitment<BlockCommitmentArray> {
        let r_block_commitment = self.block_commitment_cache.read().unwrap();
        RpcBlockCommitment {
            commitment: r_block_commitment
                .get_block_commitment(block)
                .map(|block_commitment| block_commitment.commitment),
            total_stake: r_block_commitment.total_stake(),
        }
    }

    fn get_slot(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment)?.slot())
    }

    fn get_slot_leader(&self, commitment: Option<CommitmentConfig>) -> Result<String> {
        Ok(self.bank(commitment)?.collector_id().to_string())
    }

    fn minimum_ledger_slot(&self) -> Result<Slot> {
        match self.blockstore.slot_meta_iterator(0) {
            Ok(mut metas) => match metas.next() {
                Some((slot, _meta)) => Ok(slot),
                None => Err(Error::invalid_request()),
            },
            Err(err) => {
                warn!("slot_meta_iterator failed: {:?}", err);
                Err(Error::invalid_request())
            }
        }
    }

    fn get_transaction_count(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment)?.transaction_count() as u64)
    }

    fn get_total_supply(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment)?.capitalization())
    }

    fn get_largest_accounts(
        &self,
        config: Option<RpcLargestAccountsConfig>,
    ) -> RpcResponse<Vec<RpcAccountBalance>> {
        let config = config.unwrap_or_default();
        let bank = self.bank(config.commitment)?;
        let (addresses, address_filter) = if let Some(filter) = config.filter {
            let non_circulating_supply = calculate_non_circulating_supply(bank.clone());
            let addresses = non_circulating_supply.accounts.into_iter().collect();
            let address_filter = match filter {
                RpcLargestAccountsFilter::Circulating => AccountAddressFilter::Exclude,
                RpcLargestAccountsFilter::NonCirculating => AccountAddressFilter::Include,
            };
            (addresses, address_filter)
        } else {
            (HashSet::new(), AccountAddressFilter::Exclude)
        };
        new_response(
            &bank,
            bank.get_largest_accounts(NUM_LARGEST_ACCOUNTS, &addresses, address_filter)
                .into_iter()
                .map(|(address, lamports)| RpcAccountBalance {
                    address: address.to_string(),
                    lamports,
                })
                .collect(),
        )
    }

    fn get_supply(&self, commitment: Option<CommitmentConfig>) -> RpcResponse<RpcSupply> {
        let bank = self.bank(commitment)?;
        let non_circulating_supply = calculate_non_circulating_supply(bank.clone());
        let total_supply = bank.capitalization();
        new_response(
            &bank,
            RpcSupply {
                total: total_supply,
                circulating: total_supply - non_circulating_supply.lamports,
                non_circulating: non_circulating_supply.lamports,
                non_circulating_accounts: non_circulating_supply
                    .accounts
                    .iter()
                    .map(|pubkey| pubkey.to_string())
                    .collect(),
            },
        )
    }

    fn get_vote_accounts(
        &self,
        commitment: Option<CommitmentConfig>,
    ) -> Result<RpcVoteAccountStatus> {
        let bank = self.bank(commitment)?;
        let vote_accounts = bank.vote_accounts();
        let epoch_vote_accounts = bank
            .epoch_vote_accounts(bank.get_epoch_and_slot_index(bank.slot()).0)
            .ok_or_else(Error::invalid_request)?;
        let (current_vote_accounts, delinquent_vote_accounts): (
            Vec<RpcVoteAccountInfo>,
            Vec<RpcVoteAccountInfo>,
        ) = vote_accounts
            .iter()
            .map(|(pubkey, (activated_stake, account))| {
                let vote_state = VoteState::from(&account).unwrap_or_default();
                let last_vote = if let Some(vote) = vote_state.votes.iter().last() {
                    vote.slot
                } else {
                    0
                };
                let epoch_vote_account = epoch_vote_accounts
                    .iter()
                    .any(|(epoch_vote_pubkey, _)| epoch_vote_pubkey == pubkey);
                RpcVoteAccountInfo {
                    vote_pubkey: (pubkey).to_string(),
                    node_pubkey: vote_state.node_pubkey.to_string(),
                    activated_stake: *activated_stake,
                    commission: vote_state.commission,
                    root_slot: vote_state.root_slot.unwrap_or(0),
                    epoch_credits: vote_state.epoch_credits().clone(),
                    epoch_vote_account,
                    last_vote,
                }
            })
            .partition(|vote_account_info| {
                if bank.slot() >= MAX_LOCKOUT_HISTORY as u64 {
                    vote_account_info.last_vote > bank.slot() - MAX_LOCKOUT_HISTORY as u64
                } else {
                    vote_account_info.last_vote > 0
                }
            });

        let delinquent_staked_vote_accounts = delinquent_vote_accounts
            .into_iter()
            .filter(|vote_account_info| vote_account_info.activated_stake > 0)
            .collect::<Vec<_>>();

        Ok(RpcVoteAccountStatus {
            current: current_vote_accounts,
            delinquent: delinquent_staked_vote_accounts,
        })
    }

    pub fn set_log_filter(&self, filter: String) -> Result<()> {
        if self.config.enable_set_log_filter {
            solana_logger::setup_with(&filter);
        }
        Ok(())
    }

    pub fn validator_exit(&self) -> Result<bool> {
        if self.config.enable_validator_exit {
            warn!("validator_exit request...");
            if let Some(x) = self.validator_exit.write().unwrap().take() {
                x.exit()
            }
            Ok(true)
        } else {
            debug!("validator_exit ignored");
            Ok(false)
        }
    }

    fn check_slot_cleaned_up<T>(
        &self,
        result: &std::result::Result<T, BlockstoreError>,
        slot: Slot,
    ) -> Result<()>
    where
        T: std::fmt::Debug,
    {
        if result.is_err() {
            if let BlockstoreError::SlotCleanedUp = result.as_ref().unwrap_err() {
                return Err(RpcCustomError::BlockCleanedUp {
                    slot,
                    first_available_block: self
                        .blockstore
                        .get_first_available_block()
                        .unwrap_or_default(),
                }
                .into());
            }
        }
        Ok(())
    }

    pub fn get_confirmed_block(
        &self,
        slot: Slot,
        encoding: Option<TransactionEncoding>,
    ) -> Result<Option<ConfirmedBlock>> {
        if self.config.enable_rpc_transaction_history
            && slot
                <= self
                    .block_commitment_cache
                    .read()
                    .unwrap()
                    .largest_confirmed_root()
        {
            let result = self.blockstore.get_confirmed_block(slot, encoding);
            self.check_slot_cleaned_up(&result, slot)?;
            Ok(result.ok())
        } else {
            Ok(None)
        }
    }

    pub fn get_confirmed_blocks(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> Result<Vec<Slot>> {
        let end_slot = min(
            end_slot.unwrap_or(std::u64::MAX),
            self.block_commitment_cache
                .read()
                .unwrap()
                .largest_confirmed_root(),
        );
        if end_slot < start_slot {
            return Ok(vec![]);
        }
        Ok(self
            .blockstore
            .rooted_slot_iterator(max(start_slot, self.blockstore.lowest_slot()))
            .map_err(|_| Error::internal_error())?
            .filter(|&slot| slot <= end_slot)
            .collect())
    }

    pub fn get_block_time(&self, slot: Slot) -> Result<Option<UnixTimestamp>> {
        if slot
            <= self
                .block_commitment_cache
                .read()
                .unwrap()
                .largest_confirmed_root()
        {
            // This calculation currently assumes that bank.slots_per_year will remain unchanged after
            // genesis (ie. that this bank's slot_per_year will be applicable to any rooted slot being
            // queried). If these values will be variable in the future, those timing parameters will
            // need to be stored persistently, and the slot_duration calculation will likely need to be
            // moved upstream into blockstore. Also, an explicit commitment level will need to be set.
            let bank = self.bank(None)?;
            let slot_duration = slot_duration_from_slots_per_year(bank.slots_per_year());
            let epoch = bank.epoch_schedule().get_epoch(slot);
            let stakes = HashMap::new();
            let stakes = bank.epoch_vote_accounts(epoch).unwrap_or(&stakes);

            let result = self.blockstore.get_block_time(slot, slot_duration, stakes);
            self.check_slot_cleaned_up(&result, slot)?;
            Ok(result.ok().unwrap_or(None))
        } else {
            Ok(None)
        }
    }

    pub fn get_signature_confirmation_status(
        &self,
        signature: Signature,
        commitment: Option<CommitmentConfig>,
    ) -> Option<RpcSignatureConfirmation> {
        self.get_transaction_status(signature, &self.bank(commitment).ok()?)
            .map(
                |TransactionStatus {
                     status,
                     confirmations,
                     ..
                 }| RpcSignatureConfirmation {
                    confirmations: confirmations.unwrap_or(MAX_LOCKOUT_HISTORY + 1),
                    status,
                },
            )
    }

    pub fn get_signature_status(
        &self,
        signature: Signature,
        commitment: Option<CommitmentConfig>,
    ) -> Option<transaction::Result<()>> {
        self.bank(commitment)
            .ok()?
            .get_signature_status_slot(&signature)
            .map(|(_, status)| status)
    }

    pub fn get_signature_statuses(
        &self,
        signatures: Vec<Signature>,
        config: Option<RpcSignatureStatusConfig>,
    ) -> RpcResponse<Vec<Option<TransactionStatus>>> {
        let mut statuses: Vec<Option<TransactionStatus>> = vec![];

        let search_transaction_history = config
            .map(|x| x.search_transaction_history)
            .unwrap_or(false);
        let bank = self.bank(Some(CommitmentConfig::recent()))?;

        for signature in signatures {
            let status = if let Some(status) = self.get_transaction_status(signature, &bank) {
                Some(status)
            } else if self.config.enable_rpc_transaction_history && search_transaction_history {
                self.blockstore
                    .get_transaction_status(signature)
                    .map_err(|_| Error::internal_error())?
                    .filter(|(slot, _status_meta)| {
                        slot <= &self
                            .block_commitment_cache
                            .read()
                            .unwrap()
                            .largest_confirmed_root()
                    })
                    .map(|(slot, status_meta)| {
                        let err = status_meta.status.clone().err();
                        TransactionStatus {
                            slot,
                            status: status_meta.status,
                            confirmations: None,
                            err,
                        }
                    })
            } else {
                None
            };
            statuses.push(status);
        }
        Ok(Response {
            context: RpcResponseContext { slot: bank.slot() },
            value: statuses,
        })
    }

    fn get_transaction_status(
        &self,
        signature: Signature,
        bank: &Arc<Bank>,
    ) -> Option<TransactionStatus> {
        bank.get_signature_status_slot(&signature)
            .map(|(slot, status)| {
                let r_block_commitment_cache = self.block_commitment_cache.read().unwrap();

                let confirmations = if r_block_commitment_cache.root() >= slot
                    && r_block_commitment_cache.is_confirmed_rooted(slot)
                {
                    None
                } else {
                    r_block_commitment_cache
                        .get_confirmation_count(slot)
                        .or(Some(0))
                };
                let err = status.clone().err();
                TransactionStatus {
                    slot,
                    status,
                    confirmations,
                    err,
                }
            })
    }

    pub fn get_confirmed_transaction(
        &self,
        signature: Signature,
        encoding: Option<TransactionEncoding>,
    ) -> Result<Option<ConfirmedTransaction>> {
        if self.config.enable_rpc_transaction_history {
            Ok(self
                .blockstore
                .get_confirmed_transaction(signature, encoding)
                .unwrap_or(None)
                .filter(|confirmed_transaction| {
                    confirmed_transaction.slot
                        <= self
                            .block_commitment_cache
                            .read()
                            .unwrap()
                            .largest_confirmed_root()
                }))
        } else {
            Ok(None)
        }
    }

    pub fn get_confirmed_signatures_for_address(
        &self,
        pubkey: Pubkey,
        start_slot: Slot,
        end_slot: Slot,
    ) -> Result<Vec<Signature>> {
        if self.config.enable_rpc_transaction_history {
            let end_slot = min(
                end_slot,
                self.block_commitment_cache
                    .read()
                    .unwrap()
                    .largest_confirmed_root(),
            );
            Ok(self
                .blockstore
                .get_confirmed_signatures_for_address(pubkey, start_slot, end_slot)
                .unwrap_or_else(|_| vec![]))
        } else {
            Ok(vec![])
        }
    }

    pub fn get_first_available_block(&self) -> Result<Slot> {
        Ok(self
            .blockstore
            .get_first_available_block()
            .unwrap_or_default())
    }
}

fn get_tpu_addr(cluster_info: &ClusterInfo) -> Result<SocketAddr> {
    let contact_info = cluster_info.my_contact_info();
    Ok(contact_info.tpu)
}

fn verify_pubkey(input: String) -> Result<Pubkey> {
    input.parse().map_err(|_e| Error::invalid_request())
}

fn verify_signature(input: &str) -> Result<Signature> {
    input.parse().map_err(|_e| Error::invalid_request())
}

#[derive(Clone)]
pub struct Meta {
    pub request_processor: Arc<RwLock<JsonRpcRequestProcessor>>,
    pub cluster_info: Arc<ClusterInfo>,
    pub genesis_hash: Hash,
}
impl Metadata for Meta {}

#[rpc(server)]
pub trait RpcSol {
    type Metadata;

    // DEPRECATED
    #[rpc(meta, name = "confirmTransaction")]
    fn confirm_transaction(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<bool>;

    // DEPRECATED
    #[rpc(meta, name = "getSignatureStatus")]
    fn get_signature_status(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<transaction::Result<()>>>;

    // DEPRECATED (used by Trust Wallet)
    #[rpc(meta, name = "getSignatureConfirmation")]
    fn get_signature_confirmation(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<RpcSignatureConfirmation>>;

    #[rpc(meta, name = "getAccountInfo")]
    fn get_account_info(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<Option<RpcAccount>>;

    #[rpc(meta, name = "getProgramAccounts")]
    fn get_program_accounts(
        &self,
        meta: Self::Metadata,
        program_id_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Vec<RpcKeyedAccount>>;

    #[rpc(meta, name = "getMinimumBalanceForRentExemption")]
    fn get_minimum_balance_for_rent_exemption(
        &self,
        meta: Self::Metadata,
        data_len: usize,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64>;

    #[rpc(meta, name = "getInflation")]
    fn get_inflation(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Inflation>;

    #[rpc(meta, name = "getEpochSchedule")]
    fn get_epoch_schedule(&self, meta: Self::Metadata) -> Result<EpochSchedule>;

    #[rpc(meta, name = "getBalance")]
    fn get_balance(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<u64>;

    #[rpc(meta, name = "getClusterNodes")]
    fn get_cluster_nodes(&self, meta: Self::Metadata) -> Result<Vec<RpcContactInfo>>;

    #[rpc(meta, name = "getEpochInfo")]
    fn get_epoch_info(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<EpochInfo>;

    #[rpc(meta, name = "getBlockCommitment")]
    fn get_block_commitment(
        &self,
        meta: Self::Metadata,
        block: Slot,
    ) -> Result<RpcBlockCommitment<BlockCommitmentArray>>;

    #[rpc(meta, name = "getGenesisHash")]
    fn get_genesis_hash(&self, meta: Self::Metadata) -> Result<String>;

    #[rpc(meta, name = "getLeaderSchedule")]
    fn get_leader_schedule(
        &self,
        meta: Self::Metadata,
        slot: Option<Slot>,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<RpcLeaderSchedule>>;

    #[rpc(meta, name = "getRecentBlockhash")]
    fn get_recent_blockhash(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<RpcBlockhashFeeCalculator>;

    #[rpc(meta, name = "getFeeCalculatorForBlockhash")]
    fn get_fee_calculator_for_blockhash(
        &self,
        meta: Self::Metadata,
        blockhash: String,
    ) -> RpcResponse<Option<RpcFeeCalculator>>;

    #[rpc(meta, name = "getFeeRateGovernor")]
    fn get_fee_rate_governor(&self, meta: Self::Metadata) -> RpcResponse<RpcFeeRateGovernor>;

    #[rpc(meta, name = "getSignatureStatuses")]
    fn get_signature_statuses(
        &self,
        meta: Self::Metadata,
        signature_strs: Vec<String>,
        config: Option<RpcSignatureStatusConfig>,
    ) -> RpcResponse<Vec<Option<TransactionStatus>>>;

    #[rpc(meta, name = "getSlot")]
    fn get_slot(&self, meta: Self::Metadata, commitment: Option<CommitmentConfig>) -> Result<u64>;

    #[rpc(meta, name = "getTransactionCount")]
    fn get_transaction_count(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64>;

    // DEPRECATED
    #[rpc(meta, name = "getTotalSupply")]
    fn get_total_supply(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64>;

    #[rpc(meta, name = "getLargestAccounts")]
    fn get_largest_accounts(
        &self,
        meta: Self::Metadata,
        config: Option<RpcLargestAccountsConfig>,
    ) -> RpcResponse<Vec<RpcAccountBalance>>;

    #[rpc(meta, name = "getSupply")]
    fn get_supply(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<RpcSupply>;

    #[rpc(meta, name = "requestAirdrop")]
    fn request_airdrop(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        lamports: u64,
        commitment: Option<CommitmentConfig>,
    ) -> Result<String>;

    #[rpc(meta, name = "sendTransaction")]
    fn send_transaction(&self, meta: Self::Metadata, data: String) -> Result<String>;

    #[rpc(meta, name = "simulateTransaction")]
    fn simulate_transaction(
        &self,
        meta: Self::Metadata,
        data: String,
        config: Option<RpcSimulateTransactionConfig>,
    ) -> RpcResponse<TransactionStatus>;

    #[rpc(meta, name = "getSlotLeader")]
    fn get_slot_leader(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<String>;

    #[rpc(meta, name = "minimumLedgerSlot")]
    fn minimum_ledger_slot(&self, meta: Self::Metadata) -> Result<Slot>;

    #[rpc(meta, name = "getVoteAccounts")]
    fn get_vote_accounts(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<RpcVoteAccountStatus>;

    #[rpc(meta, name = "validatorExit")]
    fn validator_exit(&self, meta: Self::Metadata) -> Result<bool>;

    #[rpc(meta, name = "getIdentity")]
    fn get_identity(&self, meta: Self::Metadata) -> Result<RpcIdentity>;

    #[rpc(meta, name = "getVersion")]
    fn get_version(&self, meta: Self::Metadata) -> Result<RpcVersionInfo>;

    #[rpc(meta, name = "setLogFilter")]
    fn set_log_filter(&self, _meta: Self::Metadata, filter: String) -> Result<()>;

    #[rpc(meta, name = "getConfirmedBlock")]
    fn get_confirmed_block(
        &self,
        meta: Self::Metadata,
        slot: Slot,
        encoding: Option<TransactionEncoding>,
    ) -> Result<Option<ConfirmedBlock>>;

    #[rpc(meta, name = "getBlockTime")]
    fn get_block_time(&self, meta: Self::Metadata, slot: Slot) -> Result<Option<UnixTimestamp>>;

    #[rpc(meta, name = "getConfirmedBlocks")]
    fn get_confirmed_blocks(
        &self,
        meta: Self::Metadata,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> Result<Vec<Slot>>;

    #[rpc(meta, name = "getConfirmedTransaction")]
    fn get_confirmed_transaction(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        encoding: Option<TransactionEncoding>,
    ) -> Result<Option<ConfirmedTransaction>>;

    #[rpc(meta, name = "getConfirmedSignaturesForAddress")]
    fn get_confirmed_signatures_for_address(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        start_slot: Slot,
        end_slot: Slot,
    ) -> Result<Vec<String>>;

    #[rpc(meta, name = "getFirstAvailableBlock")]
    fn get_first_available_block(&self, meta: Self::Metadata) -> Result<Slot>;
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(
        &self,
        meta: Self::Metadata,
        id: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<bool> {
        debug!("confirm_transaction rpc request received: {:?}", id);
        let signature = verify_signature(&id);
        meta.request_processor
            .read()
            .unwrap()
            .confirm_transaction(signature, commitment)
    }

    fn get_account_info(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<Option<RpcAccount>> {
        debug!("get_account_info rpc request received: {:?}", pubkey_str);
        let pubkey = verify_pubkey(pubkey_str);
        meta.request_processor
            .read()
            .unwrap()
            .get_account_info(pubkey, commitment)
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
        meta.request_processor
            .read()
            .unwrap()
            .get_minimum_balance_for_rent_exemption(data_len, commitment)
    }

    fn get_program_accounts(
        &self,
        meta: Self::Metadata,
        program_id_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Vec<RpcKeyedAccount>> {
        debug!(
            "get_program_accounts rpc request received: {:?}",
            program_id_str
        );
        let program_id = verify_pubkey(program_id_str)?;
        meta.request_processor
            .read()
            .unwrap()
            .get_program_accounts(&program_id, commitment)
    }

    fn get_inflation(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Inflation> {
        debug!("get_inflation rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_inflation(commitment)
    }

    fn get_epoch_schedule(&self, meta: Self::Metadata) -> Result<EpochSchedule> {
        debug!("get_epoch_schedule rpc request received");
        meta.request_processor.read().unwrap().get_epoch_schedule()
    }

    fn get_balance(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<u64> {
        debug!("get_balance rpc request received: {:?}", pubkey_str);
        let pubkey = verify_pubkey(pubkey_str);
        meta.request_processor
            .read()
            .unwrap()
            .get_balance(pubkey, commitment)
    }

    fn get_cluster_nodes(&self, meta: Self::Metadata) -> Result<Vec<RpcContactInfo>> {
        let cluster_info = &meta.cluster_info;
        fn valid_address_or_none(addr: &SocketAddr) -> Option<SocketAddr> {
            if ContactInfo::is_valid_address(addr) {
                Some(*addr)
            } else {
                None
            }
        }
        let my_shred_version = cluster_info.my_shred_version();
        Ok(cluster_info
            .all_peers()
            .iter()
            .filter_map(|(contact_info, _)| {
                if my_shred_version == contact_info.shred_version
                    && ContactInfo::is_valid_address(&contact_info.gossip)
                {
                    Some(RpcContactInfo {
                        pubkey: contact_info.id.to_string(),
                        gossip: Some(contact_info.gossip),
                        tpu: valid_address_or_none(&contact_info.tpu),
                        rpc: valid_address_or_none(&contact_info.rpc),
                        version: cluster_info
                            .get_node_version(&contact_info.id)
                            .map(|v| v.to_string()),
                    })
                } else {
                    None // Exclude spy nodes
                }
            })
            .collect())
    }

    fn get_epoch_info(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<EpochInfo> {
        let bank = meta.request_processor.read().unwrap().bank(commitment)?;
        Ok(bank.get_epoch_info())
    }

    fn get_block_commitment(
        &self,
        meta: Self::Metadata,
        block: Slot,
    ) -> Result<RpcBlockCommitment<BlockCommitmentArray>> {
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_block_commitment(block))
    }

    fn get_genesis_hash(&self, meta: Self::Metadata) -> Result<String> {
        debug!("get_genesis_hash rpc request received");
        Ok(meta.genesis_hash.to_string())
    }

    fn get_leader_schedule(
        &self,
        meta: Self::Metadata,
        slot: Option<Slot>,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<RpcLeaderSchedule>> {
        let bank = meta.request_processor.read().unwrap().bank(commitment)?;
        let slot = slot.unwrap_or_else(|| bank.slot());
        let epoch = bank.epoch_schedule().get_epoch(slot);

        Ok(
            solana_ledger::leader_schedule_utils::leader_schedule(epoch, &bank).map(
                |leader_schedule| {
                    let mut map = HashMap::new();

                    for (slot_index, pubkey) in
                        leader_schedule.get_slot_leaders().iter().enumerate()
                    {
                        let pubkey = pubkey.to_string();
                        map.entry(pubkey).or_insert_with(|| vec![]).push(slot_index);
                    }
                    map
                },
            ),
        )
    }

    fn get_recent_blockhash(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<RpcBlockhashFeeCalculator> {
        debug!("get_recent_blockhash rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_recent_blockhash(commitment)
    }

    fn get_fee_calculator_for_blockhash(
        &self,
        meta: Self::Metadata,
        blockhash: String,
    ) -> RpcResponse<Option<RpcFeeCalculator>> {
        debug!("get_fee_calculator_for_blockhash rpc request received");
        let blockhash =
            Hash::from_str(&blockhash).map_err(|e| Error::invalid_params(format!("{:?}", e)))?;
        meta.request_processor
            .read()
            .unwrap()
            .get_fee_calculator_for_blockhash(&blockhash)
    }

    fn get_fee_rate_governor(&self, meta: Self::Metadata) -> RpcResponse<RpcFeeRateGovernor> {
        debug!("get_fee_rate_governor rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_fee_rate_governor()
    }

    fn get_signature_confirmation(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<RpcSignatureConfirmation>> {
        debug!(
            "get_signature_confirmation rpc request received: {:?}",
            signature_str
        );
        let signature = verify_signature(&signature_str)?;
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_signature_confirmation_status(signature, commitment))
    }

    fn get_signature_status(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<transaction::Result<()>>> {
        let signature = verify_signature(&signature_str)?;
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_signature_status(signature, commitment))
    }

    fn get_signature_statuses(
        &self,
        meta: Self::Metadata,
        signature_strs: Vec<String>,
        config: Option<RpcSignatureStatusConfig>,
    ) -> RpcResponse<Vec<Option<TransactionStatus>>> {
        if signature_strs.len() > MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS {
            return Err(Error::invalid_params(format!(
                "Too many inputs provided; max {}",
                MAX_GET_SIGNATURE_STATUSES_QUERY_ITEMS
            )));
        }
        let mut signatures: Vec<Signature> = vec![];
        for signature_str in signature_strs {
            signatures.push(verify_signature(&signature_str)?);
        }
        meta.request_processor
            .read()
            .unwrap()
            .get_signature_statuses(signatures, config)
    }

    fn get_slot(&self, meta: Self::Metadata, commitment: Option<CommitmentConfig>) -> Result<u64> {
        meta.request_processor.read().unwrap().get_slot(commitment)
    }

    fn get_transaction_count(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64> {
        debug!("get_transaction_count rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_transaction_count(commitment)
    }

    fn get_total_supply(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64> {
        debug!("get_total_supply rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_total_supply(commitment)
    }

    fn get_largest_accounts(
        &self,
        meta: Self::Metadata,
        config: Option<RpcLargestAccountsConfig>,
    ) -> RpcResponse<Vec<RpcAccountBalance>> {
        debug!("get_largest_accounts rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_largest_accounts(config)
    }

    fn get_supply(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<RpcSupply> {
        debug!("get_supply rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_supply(commitment)
    }

    fn request_airdrop(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        lamports: u64,
        commitment: Option<CommitmentConfig>,
    ) -> Result<String> {
        trace!(
            "request_airdrop id={} lamports={} commitment: {:?}",
            pubkey_str,
            lamports,
            &commitment
        );

        let faucet_addr = meta
            .request_processor
            .read()
            .unwrap()
            .config
            .faucet_addr
            .ok_or_else(Error::invalid_request)?;
        let pubkey = verify_pubkey(pubkey_str)?;

        let blockhash = meta
            .request_processor
            .read()
            .unwrap()
            .bank(commitment.clone())?
            .confirmed_last_blockhash()
            .0;
        let transaction = request_airdrop_transaction(&faucet_addr, &pubkey, lamports, blockhash)
            .map_err(|err| {
            info!("request_airdrop_transaction failed: {:?}", err);
            Error::internal_error()
        })?;

        let data = serialize(&transaction).map_err(|err| {
            info!("request_airdrop: serialize error: {:?}", err);
            Error::internal_error()
        })?;

        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let tpu_addr = get_tpu_addr(&meta.cluster_info)?;
        transactions_socket
            .send_to(&data, tpu_addr)
            .map_err(|err| {
                info!("request_airdrop: send_to error: {:?}", err);
                Error::internal_error()
            })?;

        let signature = transaction.signatures[0];
        let now = Instant::now();
        let mut signature_status;
        let signature_timeout = match &commitment {
            Some(config) if config.commitment == CommitmentLevel::Recent => 5,
            _ => 30,
        };
        loop {
            signature_status = meta
                .request_processor
                .read()
                .unwrap()
                .get_signature_statuses(vec![signature], None)?
                .value[0]
                .clone()
                .filter(|result| result.satisfies_commitment(commitment.unwrap_or_default()))
                .map(|x| x.status);

            if signature_status == Some(Ok(())) {
                info!("airdrop signature ok");
                return Ok(signature.to_string());
            } else if now.elapsed().as_secs() > signature_timeout {
                info!("airdrop signature timeout");
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }

    fn send_transaction(&self, meta: Self::Metadata, data: String) -> Result<String> {
        let (wire_transaction, transaction) = deserialize_bs58_transaction(data)?;
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let tpu_addr = get_tpu_addr(&meta.cluster_info)?;
        transactions_socket
            .send_to(&wire_transaction, tpu_addr)
            .map_err(|err| {
                info!("send_transaction: send_to error: {:?}", err);
                Error::internal_error()
            })?;
        let signature = transaction.signatures[0].to_string();
        trace!(
            "send_transaction: sent {} bytes, signature={}",
            wire_transaction.len(),
            signature
        );
        Ok(signature)
    }

    fn simulate_transaction(
        &self,
        meta: Self::Metadata,
        data: String,
        config: Option<RpcSimulateTransactionConfig>,
    ) -> RpcResponse<TransactionStatus> {
        let (_, transaction) = deserialize_bs58_transaction(data)?;
        let config = config.unwrap_or(RpcSimulateTransactionConfig { sig_verify: false });

        let bank = &*meta.request_processor.read().unwrap().bank(None)?;
        assert!(bank.is_frozen());

        let mut result = if config.sig_verify {
            transaction.verify()
        } else {
            Ok(())
        };

        if result.is_ok() {
            let transactions = [transaction];
            let batch = bank.prepare_batch(&transactions, None);
            let (
                _loaded_accounts,
                executed,
                _retryable_transactions,
                _transaction_count,
                _signature_count,
            ) = bank.load_and_execute_transactions(&batch, solana_sdk::clock::MAX_PROCESSING_AGE);
            result = executed[0].0.clone();
        }

        new_response(
            &bank,
            TransactionStatus {
                slot: bank.slot(),
                confirmations: Some(0),
                status: result.clone(),
                err: result.err(),
            },
        )
    }

    fn get_slot_leader(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<String> {
        meta.request_processor
            .read()
            .unwrap()
            .get_slot_leader(commitment)
    }

    fn minimum_ledger_slot(&self, meta: Self::Metadata) -> Result<Slot> {
        meta.request_processor.read().unwrap().minimum_ledger_slot()
    }

    fn get_vote_accounts(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<RpcVoteAccountStatus> {
        meta.request_processor
            .read()
            .unwrap()
            .get_vote_accounts(commitment)
    }

    fn validator_exit(&self, meta: Self::Metadata) -> Result<bool> {
        meta.request_processor.read().unwrap().validator_exit()
    }

    fn get_identity(&self, meta: Self::Metadata) -> Result<RpcIdentity> {
        Ok(RpcIdentity {
            identity: meta
                .request_processor
                .read()
                .unwrap()
                .config
                .identity_pubkey
                .to_string(),
        })
    }

    fn get_version(&self, _: Self::Metadata) -> Result<RpcVersionInfo> {
        Ok(RpcVersionInfo {
            solana_core: solana_version::Version::default().to_string(),
        })
    }

    fn set_log_filter(&self, meta: Self::Metadata, filter: String) -> Result<()> {
        meta.request_processor
            .read()
            .unwrap()
            .set_log_filter(filter)
    }

    fn get_confirmed_block(
        &self,
        meta: Self::Metadata,
        slot: Slot,
        encoding: Option<TransactionEncoding>,
    ) -> Result<Option<ConfirmedBlock>> {
        meta.request_processor
            .read()
            .unwrap()
            .get_confirmed_block(slot, encoding)
    }

    fn get_confirmed_blocks(
        &self,
        meta: Self::Metadata,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> Result<Vec<Slot>> {
        meta.request_processor
            .read()
            .unwrap()
            .get_confirmed_blocks(start_slot, end_slot)
    }

    fn get_block_time(&self, meta: Self::Metadata, slot: Slot) -> Result<Option<UnixTimestamp>> {
        meta.request_processor.read().unwrap().get_block_time(slot)
    }

    fn get_confirmed_transaction(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        encoding: Option<TransactionEncoding>,
    ) -> Result<Option<ConfirmedTransaction>> {
        let signature = verify_signature(&signature_str)?;
        meta.request_processor
            .read()
            .unwrap()
            .get_confirmed_transaction(signature, encoding)
    }

    fn get_confirmed_signatures_for_address(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        start_slot: Slot,
        end_slot: Slot,
    ) -> Result<Vec<String>> {
        let pubkey = verify_pubkey(pubkey_str)?;
        if end_slot < start_slot {
            return Err(Error::invalid_params(format!(
                "start_slot {} must be less than or equal to end_slot {}",
                start_slot, end_slot
            )));
        }
        if end_slot - start_slot > MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS_SLOT_RANGE {
            return Err(Error::invalid_params(format!(
                "Slot range too large; max {}",
                MAX_GET_CONFIRMED_SIGNATURES_FOR_ADDRESS_SLOT_RANGE
            )));
        }
        meta.request_processor
            .read()
            .unwrap()
            .get_confirmed_signatures_for_address(pubkey, start_slot, end_slot)
            .map(|signatures| {
                signatures
                    .iter()
                    .map(|signature| signature.to_string())
                    .collect()
            })
    }

    fn get_first_available_block(&self, meta: Self::Metadata) -> Result<Slot> {
        meta.request_processor
            .read()
            .unwrap()
            .get_first_available_block()
    }
}

fn deserialize_bs58_transaction(bs58_transaction: String) -> Result<(Vec<u8>, Transaction)> {
    let wire_transaction = bs58::decode(bs58_transaction).into_vec().unwrap();
    if wire_transaction.len() >= PACKET_DATA_SIZE {
        info!(
            "transaction too large: {} bytes (max: {} bytes)",
            wire_transaction.len(),
            PACKET_DATA_SIZE
        );
        return Err(Error::invalid_request());
    }
    bincode::config()
        .limit(PACKET_DATA_SIZE as u64)
        .deserialize(&wire_transaction)
        .map_err(|err| {
            info!("transaction deserialize error: {:?}", err);
            Error::invalid_request()
        })
        .map(|transaction| (wire_transaction, transaction))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        commitment::BlockCommitment, contact_info::ContactInfo,
        non_circulating_supply::non_circulating_accounts,
        replay_stage::tests::create_test_transactions_and_populate_blockstore,
    };
    use bincode::deserialize;
    use jsonrpc_core::{MetaIoHandler, Output, Response, Value};
    use solana_ledger::{
        blockstore::entries_to_test_shreds,
        blockstore_processor::fill_blockstore_slot_with_ticks,
        entry::next_entry_mut,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        get_tmp_ledger_path,
    };
    use solana_sdk::{
        fee_calculator::DEFAULT_BURN_PERCENT,
        hash::{hash, Hash},
        instruction::InstructionError,
        rpc_port,
        signature::{Keypair, Signer},
        system_transaction,
        transaction::{self, TransactionError},
    };
    use solana_transaction_status::{EncodedTransaction, TransactionWithStatusMeta};
    use solana_vote_program::{
        vote_instruction,
        vote_state::{Vote, VoteInit, MAX_LOCKOUT_HISTORY},
    };
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };

    const TEST_MINT_LAMPORTS: u64 = 1_000_000;
    const TEST_SLOTS_PER_EPOCH: u64 = 50;

    struct RpcHandler {
        io: MetaIoHandler<Meta>,
        meta: Meta,
        bank: Arc<Bank>,
        bank_forks: Arc<RwLock<BankForks>>,
        blockhash: Hash,
        alice: Keypair,
        leader_pubkey: Pubkey,
        leader_vote_keypair: Keypair,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        confirmed_block_signatures: Vec<Signature>,
    }

    fn start_rpc_handler_with_tx(pubkey: &Pubkey) -> RpcHandler {
        start_rpc_handler_with_tx_and_blockstore(pubkey, vec![], 0)
    }

    fn start_rpc_handler_with_tx_and_blockstore(
        pubkey: &Pubkey,
        blockstore_roots: Vec<Slot>,
        default_timestamp: i64,
    ) -> RpcHandler {
        let (bank_forks, alice, leader_vote_keypair) = new_bank_forks();
        let bank = bank_forks.read().unwrap().working_bank();
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&ledger_path).unwrap();
        let blockstore = Arc::new(blockstore);

        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        bank.transfer(4, &alice, &keypair2.pubkey()).unwrap();
        let confirmed_block_signatures = create_test_transactions_and_populate_blockstore(
            vec![&alice, &keypair1, &keypair2, &keypair3],
            0,
            bank.clone(),
            blockstore.clone(),
        );

        let mut commitment_slot0 = BlockCommitment::default();
        commitment_slot0.increase_confirmation_stake(2, 9);
        let mut commitment_slot1 = BlockCommitment::default();
        commitment_slot1.increase_confirmation_stake(1, 9);
        let mut block_commitment: HashMap<u64, BlockCommitment> = HashMap::new();
        block_commitment.entry(0).or_insert(commitment_slot0);
        block_commitment.entry(1).or_insert(commitment_slot1);
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new(
            block_commitment,
            0,
            10,
            bank.clone(),
            blockstore.clone(),
            0,
            0,
        )));

        // Add timestamp vote to blockstore
        let vote = Vote {
            slots: vec![1],
            hash: Hash::default(),
            timestamp: Some(default_timestamp),
        };
        let vote_ix = vote_instruction::vote(
            &leader_vote_keypair.pubkey(),
            &leader_vote_keypair.pubkey(),
            vote,
        );
        let vote_tx = Transaction::new_signed_instructions(
            &[&leader_vote_keypair],
            &[vote_ix],
            Hash::default(),
        );
        let shreds = entries_to_test_shreds(
            vec![next_entry_mut(&mut Hash::default(), 0, vec![vote_tx])],
            1,
            0,
            true,
            0,
        );
        blockstore.insert_shreds(shreds, None, false).unwrap();
        blockstore.set_roots(&[1]).unwrap();

        let mut roots = blockstore_roots;
        if !roots.is_empty() {
            roots.retain(|&x| x > 0);
            let mut parent_bank = bank;
            for (i, root) in roots.iter().enumerate() {
                let new_bank =
                    Bank::new_from_parent(&parent_bank, parent_bank.collector_id(), *root);
                parent_bank = bank_forks.write().unwrap().insert(new_bank);
                let parent = if i > 0 { roots[i - 1] } else { 0 };
                fill_blockstore_slot_with_ticks(&blockstore, 5, *root, parent, Hash::default());
            }
            blockstore.set_roots(&roots).unwrap();
            let new_bank = Bank::new_from_parent(
                &parent_bank,
                parent_bank.collector_id(),
                roots.iter().max().unwrap() + 1,
            );
            bank_forks.write().unwrap().insert(new_bank);

            for root in roots.iter() {
                bank_forks.write().unwrap().set_root(*root, &None, Some(0));
            }
        }

        let bank = bank_forks.read().unwrap().working_bank();

        let leader_pubkey = *bank.collector_id();
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);

        let blockhash = bank.confirmed_last_blockhash().0;
        let tx = system_transaction::transfer(&alice, pubkey, 20, blockhash);
        bank.process_transaction(&tx).expect("process transaction");
        let tx =
            system_transaction::transfer(&alice, &non_circulating_accounts()[0], 20, blockhash);
        bank.process_transaction(&tx).expect("process transaction");

        let tx = system_transaction::transfer(&alice, pubkey, std::u64::MAX, blockhash);
        let _ = bank.process_transaction(&tx);

        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            JsonRpcConfig {
                enable_rpc_transaction_history: true,
                identity_pubkey: *pubkey,
                ..JsonRpcConfig::default()
            },
            bank_forks.clone(),
            block_commitment_cache.clone(),
            blockstore,
            validator_exit,
        )));
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default()));

        cluster_info.insert_info(ContactInfo::new_with_pubkey_socketaddr(
            &leader_pubkey,
            &socketaddr!("127.0.0.1:1234"),
        ));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            cluster_info,
            genesis_hash: Hash::default(),
        };
        RpcHandler {
            io,
            meta,
            bank,
            bank_forks,
            blockhash,
            alice,
            leader_pubkey,
            leader_vote_keypair,
            block_commitment_cache,
            confirmed_block_signatures,
        }
    }

    #[test]
    fn test_rpc_request_processor_new() {
        let bob_pubkey = Pubkey::new_rand();
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let (bank_forks, alice, _) = new_bank_forks();
        let bank = bank_forks.read().unwrap().working_bank();
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore.clone()),
        ));
        let request_processor = JsonRpcRequestProcessor::new(
            JsonRpcConfig::default(),
            bank_forks,
            block_commitment_cache,
            blockstore,
            validator_exit,
        );
        thread::spawn(move || {
            let blockhash = bank.confirmed_last_blockhash().0;
            let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
            bank.process_transaction(&tx).expect("process transaction");
        })
        .join()
        .unwrap();
        assert_eq!(request_processor.get_transaction_count(None).unwrap(), 1);
    }

    #[test]
    fn test_rpc_get_balance() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":20,
                },
            "id": 1,
        });
        let result = serde_json::from_str::<Value>(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_cluster_nodes() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            leader_pubkey,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getClusterNodes"}"#;

        let res = io.handle_request_sync(&req, meta);
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");

        let expected = format!(
            r#"{{"jsonrpc":"2.0","result":[{{"pubkey": "{}", "gossip": "127.0.0.1:1235", "tpu": "127.0.0.1:1234", "rpc": "127.0.0.1:{}", "version": null}}],"id":1}}"#,
            leader_pubkey,
            rpc_port::DEFAULT_RPC_PORT
        );

        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_slot_leader() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            leader_pubkey,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getSlotLeader"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":"{}","id":1}}"#, leader_pubkey);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_tx_count() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getTransactionCount"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = r#"{"jsonrpc":"2.0","result":4,"id":1}"#;
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_minimum_ledger_slot() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"minimumLedgerSlot"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = r#"{"jsonrpc":"2.0","result":0,"id":1}"#;
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_total_supply() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getTotalSupply"}"#;
        let rep = io.handle_request_sync(&req, meta);
        let res: Response = serde_json::from_str(&rep.expect("actual response"))
            .expect("actual response deserialization");
        let supply: u64 = if let Response::Single(res) = res {
            if let Output::Success(res) = res {
                if let Value::Number(num) = res.result {
                    num.as_u64().unwrap()
                } else {
                    panic!("Expected number");
                }
            } else {
                panic!("Expected success");
            }
        } else {
            panic!("Expected single response");
        };
        assert!(supply >= TEST_MINT_LAMPORTS);
    }

    #[test]
    fn test_get_supply() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getSupply"}"#;
        let res = io.handle_request_sync(&req, meta);
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let supply: RpcSupply = serde_json::from_value(json["result"]["value"].clone())
            .expect("actual response deserialization");
        assert_eq!(supply.non_circulating, 20);
        assert!(supply.circulating >= TEST_MINT_LAMPORTS);
        assert!(supply.total >= TEST_MINT_LAMPORTS + 20);
        let expected_accounts: Vec<String> = non_circulating_accounts()
            .iter()
            .map(|pubkey| pubkey.to_string())
            .collect();
        assert_eq!(
            supply.non_circulating_accounts.len(),
            expected_accounts.len()
        );
        for address in supply.non_circulating_accounts {
            assert!(expected_accounts.contains(&address));
        }
    }

    #[test]
    fn test_get_largest_accounts() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io, meta, alice, ..
        } = start_rpc_handler_with_tx(&bob_pubkey);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getLargestAccounts"}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let largest_accounts: Vec<RpcAccountBalance> =
            serde_json::from_value(json["result"]["value"].clone())
                .expect("actual response deserialization");
        assert_eq!(largest_accounts.len(), 19);

        // Get Alice balance
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
            alice.pubkey()
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let alice_balance: u64 = serde_json::from_value(json["result"]["value"].clone())
            .expect("actual response deserialization");
        assert!(largest_accounts.contains(&RpcAccountBalance {
            address: alice.pubkey().to_string(),
            lamports: alice_balance,
        }));

        // Get Bob balance
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBalance","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let bob_balance: u64 = serde_json::from_value(json["result"]["value"].clone())
            .expect("actual response deserialization");
        assert!(largest_accounts.contains(&RpcAccountBalance {
            address: bob_pubkey.to_string(),
            lamports: bob_balance,
        }));

        // Test Circulating/NonCirculating Filter
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getLargestAccounts","params":[{"filter":"circulating"}]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let largest_accounts: Vec<RpcAccountBalance> =
            serde_json::from_value(json["result"]["value"].clone())
                .expect("actual response deserialization");
        assert_eq!(largest_accounts.len(), 18);
        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getLargestAccounts","params":[{"filter":"nonCirculating"}]}"#;
        let res = io.handle_request_sync(&req, meta);
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let largest_accounts: Vec<RpcAccountBalance> =
            serde_json::from_value(json["result"]["value"].clone())
                .expect("actual response deserialization");
        assert_eq!(largest_accounts.len(), 1);
    }

    #[test]
    fn test_rpc_get_minimum_balance_for_rent_exemption() {
        let bob_pubkey = Pubkey::new_rand();
        let data_len = 50;
        let RpcHandler { io, meta, bank, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getMinimumBalanceForRentExemption","params":[{}]}}"#,
            data_len
        );
        let rep = io.handle_request_sync(&req, meta);
        let res: Response = serde_json::from_str(&rep.expect("actual response"))
            .expect("actual response deserialization");
        let minimum_balance: u64 = if let Response::Single(res) = res {
            if let Output::Success(res) = res {
                if let Value::Number(num) = res.result {
                    num.as_u64().unwrap()
                } else {
                    panic!("Expected number");
                }
            } else {
                panic!("Expected success");
            }
        } else {
            panic!("Expected single response");
        };
        assert_eq!(
            minimum_balance,
            bank.get_minimum_balance_for_rent_exemption(data_len)
        );
    }

    #[test]
    fn test_rpc_get_inflation() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, bank, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getInflation"}"#;
        let rep = io.handle_request_sync(&req, meta);
        let res: Response = serde_json::from_str(&rep.expect("actual response"))
            .expect("actual response deserialization");
        let inflation: Inflation = if let Response::Single(res) = res {
            if let Output::Success(res) = res {
                serde_json::from_value(res.result).unwrap()
            } else {
                panic!("Expected success");
            }
        } else {
            panic!("Expected single response");
        };
        assert_eq!(inflation, bank.inflation());
    }

    #[test]
    fn test_rpc_get_epoch_schedule() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, bank, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getEpochSchedule"}"#;
        let rep = io.handle_request_sync(&req, meta);
        let res: Response = serde_json::from_str(&rep.expect("actual response"))
            .expect("actual response deserialization");

        let epoch_schedule: EpochSchedule = if let Response::Single(res) = res {
            if let Output::Success(res) = res {
                serde_json::from_value(res.result).unwrap()
            } else {
                panic!("Expected success");
            }
        } else {
            panic!("Expected single response");
        };
        assert_eq!(epoch_schedule, *bank.epoch_schedule());
    }

    #[test]
    fn test_rpc_get_leader_schedule() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, bank, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        for req in [
            r#"{"jsonrpc":"2.0","id":1,"method":"getLeaderSchedule", "params": [0]}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"getLeaderSchedule"}"#,
        ]
        .iter()
        {
            let rep = io.handle_request_sync(&req, meta.clone());
            let res: Response = serde_json::from_str(&rep.expect("actual response"))
                .expect("actual response deserialization");

            let schedule: Option<RpcLeaderSchedule> = if let Response::Single(res) = res {
                if let Output::Success(res) = res {
                    serde_json::from_value(res.result).unwrap()
                } else {
                    panic!("Expected success for {}", req);
                }
            } else {
                panic!("Expected single response");
            };
            let schedule = schedule.expect("leader schedule");

            let bob_schedule = schedule
                .get(&bank.collector_id().to_string())
                .expect("leader not in the leader schedule");

            assert_eq!(
                bob_schedule.len(),
                solana_ledger::leader_schedule_utils::leader_schedule(bank.epoch(), &bank)
                    .unwrap()
                    .get_slot_leaders()
                    .len()
            );
        }

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getLeaderSchedule", "params": [42424242]}"#;
        let rep = io.handle_request_sync(&req, meta);
        let res: Response = serde_json::from_str(&rep.expect("actual response"))
            .expect("actual response deserialization");

        let schedule: Option<RpcLeaderSchedule> = if let Response::Single(res) = res {
            if let Output::Success(res) = res {
                serde_json::from_value(res.result).unwrap()
            } else {
                panic!("Expected success");
            }
        } else {
            panic!("Expected single response");
        };
        assert_eq!(schedule, None);
    }

    #[test]
    fn test_rpc_get_account_info() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":{
                "owner": "11111111111111111111111111111111",
                "lamports": 20,
                "data": "",
                "executable": false,
                "rentEpoch": 0
            },
                },
            "id": 1,
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_program_accounts() {
        let bob = Keypair::new();
        let RpcHandler {
            io,
            meta,
            bank,
            blockhash,
            ..
        } = start_rpc_handler_with_tx(&bob.pubkey());

        let new_program_id = Pubkey::new_rand();
        let tx = system_transaction::assign(&bob, blockhash, &new_program_id);
        bank.process_transaction(&tx).unwrap();
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getProgramAccounts","params":["{}"]}}"#,
            new_program_id
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(
            r#"{{
                "jsonrpc":"2.0",
                "result":[
                    {{
                        "pubkey": "{}",
                        "account": {{
                            "owner": "{}",
                            "lamports": 20,
                            "data": "",
                            "executable": false,
                            "rentEpoch": 0
                        }}
                    }}
                ],
                "id":1}}
            "#,
            bob.pubkey(),
            new_program_id
        );
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_simulate_transaction() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            alice,
            bank,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let mut tx = system_transaction::transfer(&alice, &bob_pubkey, 1234, blockhash);
        let tx_serialized_encoded = bs58::encode(serialize(&tx).unwrap()).into_string();
        tx.signatures[0] = Signature::default();
        let tx_badsig_serialized_encoded = bs58::encode(serialize(&tx).unwrap()).into_string();

        bank.freeze(); // Ensure the root bank is frozen, `start_rpc_handler_with_tx()` doesn't do this

        // Good signature with sigVerify=true
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"simulateTransaction","params":["{}", {{"sigVerify": true}}]}}"#,
            tx_serialized_encoded,
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":{"confirmations":0,"slot": 0,"status":{"Ok":null},"err":null}
            },
            "id": 1,
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Bad signature with sigVerify=true
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"simulateTransaction","params":["{}", {{"sigVerify": true}}]}}"#,
            tx_badsig_serialized_encoded,
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":{"confirmations":0,"slot":0,"status":{"Err":"SignatureFailure"},"err":"SignatureFailure"}
            },
            "id": 1,
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Bad signature with sigVerify=false
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"simulateTransaction","params":["{}", {{"sigVerify": false}}]}}"#,
            tx_serialized_encoded,
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":{"confirmations":0,"slot": 0,"status":{"Ok":null},"err":null}
            },
            "id": 1,
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Bad signature with default sigVerify setting (false)
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"simulateTransaction","params":["{}"]}}"#,
            tx_serialized_encoded,
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":{"confirmations":0,"slot": 0,"status":{"Ok":null},"err":null}
            },
            "id": 1,
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    #[should_panic]
    fn test_rpc_simulate_transaction_panic_on_unfrozen_bank() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            alice,
            bank,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let tx = system_transaction::transfer(&alice, &bob_pubkey, 1234, blockhash);
        let tx_serialized_encoded = bs58::encode(serialize(&tx).unwrap()).into_string();

        assert!(!bank.is_frozen());

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"simulateTransaction","params":["{}", {{"sigVerify": true}}]}}"#,
            tx_serialized_encoded,
        );

        // should panic because `bank` is not frozen
        let _ = io.handle_request_sync(&req, meta.clone());
    }

    #[test]
    fn test_rpc_confirm_tx() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            alice,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);
        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"confirmTransaction","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":true,
                },
            "id": 1,
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_signature_status() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            alice,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected_res: Option<transaction::Result<()>> = Some(Ok(()));
        let expected = json!({
            "jsonrpc": "2.0",
            "result": expected_res,
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test getSignatureStatus request on unprocessed tx
        let tx = system_transaction::transfer(&alice, &bob_pubkey, 10, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected_res: Option<String> = None;
        let expected = json!({
            "jsonrpc": "2.0",
            "result": expected_res,
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Test getSignatureStatus request on a TransactionError
        let tx = system_transaction::transfer(&alice, &bob_pubkey, std::u64::MAX, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected_res: Option<transaction::Result<()>> = Some(Err(
            TransactionError::InstructionError(0, InstructionError::Custom(1)),
        ));
        let expected = json!({
            "jsonrpc": "2.0",
            "result": expected_res,
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_signature_statuses() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            alice,
            confirmed_block_signatures,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatuses","params":[["{}"]]}}"#,
            confirmed_block_signatures[0]
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected_res: transaction::Result<()> = Ok(());
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let result: Option<TransactionStatus> =
            serde_json::from_value(json["result"]["value"][0].clone())
                .expect("actual response deserialization");
        let result = result.as_ref().unwrap();
        assert_eq!(expected_res, result.status);
        assert_eq!(None, result.confirmations);

        // Test getSignatureStatus request on unprocessed tx
        let tx = system_transaction::transfer(&alice, &bob_pubkey, 10, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatuses","params":[["{}"]]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let result: Option<TransactionStatus> =
            serde_json::from_value(json["result"]["value"][0].clone())
                .expect("actual response deserialization");
        assert!(result.is_none());

        // Test getSignatureStatus request on a TransactionError
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatuses","params":[["{}"]]}}"#,
            confirmed_block_signatures[1]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected_res: transaction::Result<()> = Err(TransactionError::InstructionError(
            0,
            InstructionError::Custom(1),
        ));
        let json: Value = serde_json::from_str(&res.unwrap()).unwrap();
        let result: Option<TransactionStatus> =
            serde_json::from_value(json["result"]["value"][0].clone())
                .expect("actual response deserialization");
        assert_eq!(expected_res, result.as_ref().unwrap().status);
    }

    #[test]
    fn test_rpc_get_recent_blockhash() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getRecentBlockhash"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
            "context":{"slot":0},
            "value":{
                "blockhash": blockhash.to_string(),
                "feeCalculator": {
                    "lamportsPerSignature": 0,
                }
            }},
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_fee_calculator_for_blockhash() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, bank, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let (blockhash, fee_calculator) = bank.last_blockhash_with_fee_calculator();
        let fee_calculator = RpcFeeCalculator { fee_calculator };

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getFeeCalculatorForBlockhash","params":["{:?}"]}}"#,
            blockhash
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":fee_calculator,
            },
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        // Expired (non-existent) blockhash
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getFeeCalculatorForBlockhash","params":["{:?}"]}}"#,
            Hash::default()
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "context":{"slot":0},
                "value":Value::Null,
            },
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_fee_rate_governor() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getFeeRateGovernor"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
            "context":{"slot":0},
            "value":{
                "feeRateGovernor": {
                    "burnPercent": DEFAULT_BURN_PERCENT,
                    "maxLamportsPerSignature": 0,
                    "minLamportsPerSignature": 0,
                    "targetLamportsPerSignature": 0,
                    "targetSignaturesPerSlot": 0
                }
            }},
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_fail_request_airdrop() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        // Expect internal error because no faucet is available
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"requestAirdrop","params":["{}", 50]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid request"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_send_bad_tx() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore.clone()),
        ));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: {
                let request_processor = JsonRpcRequestProcessor::new(
                    JsonRpcConfig::default(),
                    new_bank_forks().0,
                    block_commitment_cache,
                    blockstore,
                    validator_exit,
                );
                Arc::new(RwLock::new(request_processor))
            },
            cluster_info: Arc::new(ClusterInfo::new_with_invalid_keypair(ContactInfo::default())),
            genesis_hash: Hash::default(),
        };

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["37u9WtQpcm6ULa3Vmu7ySnANv"]}"#;
        let res = io.handle_request_sync(req, meta);
        let expected =
            r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"Invalid request"},"id":1}"#;
        let expected: Response =
            serde_json::from_str(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_tpu_addr() {
        let cluster_info = Arc::new(ClusterInfo::new_with_invalid_keypair(
            ContactInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234")),
        ));
        assert_eq!(
            get_tpu_addr(&cluster_info),
            Ok(socketaddr!("127.0.0.1:1234"))
        );
    }

    #[test]
    fn test_rpc_verify_pubkey() {
        let pubkey = Pubkey::new_rand();
        assert_eq!(verify_pubkey(pubkey.to_string()).unwrap(), pubkey);
        let bad_pubkey = "a1b2c3d4";
        assert_eq!(
            verify_pubkey(bad_pubkey.to_string()),
            Err(Error::invalid_request())
        );
    }

    #[test]
    fn test_rpc_verify_signature() {
        let tx = system_transaction::transfer(&Keypair::new(), &Pubkey::new_rand(), 20, hash(&[0]));
        assert_eq!(
            verify_signature(&tx.signatures[0].to_string()).unwrap(),
            tx.signatures[0]
        );
        let bad_signature = "a1b2c3d4";
        assert_eq!(
            verify_signature(&bad_signature.to_string()),
            Err(Error::invalid_request())
        );
    }

    fn new_bank_forks() -> (Arc<RwLock<BankForks>>, Keypair, Keypair) {
        let GenesisConfigInfo {
            mut genesis_config,
            mint_keypair,
            voting_keypair,
        } = create_genesis_config(TEST_MINT_LAMPORTS);

        genesis_config.rent.lamports_per_byte_year = 50;
        genesis_config.rent.exemption_threshold = 2.0;
        genesis_config.epoch_schedule =
            EpochSchedule::custom(TEST_SLOTS_PER_EPOCH, TEST_SLOTS_PER_EPOCH, false);

        let bank = Bank::new(&genesis_config);
        (
            Arc::new(RwLock::new(BankForks::new(bank.slot(), bank))),
            mint_keypair,
            voting_keypair,
        )
    }

    pub fn create_validator_exit(exit: &Arc<AtomicBool>) -> Arc<RwLock<Option<ValidatorExit>>> {
        let mut validator_exit = ValidatorExit::default();
        let exit_ = exit.clone();
        validator_exit.register_exit(Box::new(move || exit_.store(true, Ordering::Relaxed)));
        Arc::new(RwLock::new(Some(validator_exit)))
    }

    #[test]
    fn test_rpc_request_processor_config_default_trait_validator_exit_fails() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore.clone()),
        ));
        let request_processor = JsonRpcRequestProcessor::new(
            JsonRpcConfig::default(),
            new_bank_forks().0,
            block_commitment_cache,
            blockstore,
            validator_exit,
        );
        assert_eq!(request_processor.validator_exit(), Ok(false));
        assert_eq!(exit.load(Ordering::Relaxed), false);
    }

    #[test]
    fn test_rpc_request_processor_allow_validator_exit_config() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::default_with_blockstore(blockstore.clone()),
        ));
        let mut config = JsonRpcConfig::default();
        config.enable_validator_exit = true;
        let request_processor = JsonRpcRequestProcessor::new(
            config,
            new_bank_forks().0,
            block_commitment_cache,
            blockstore,
            validator_exit,
        );
        assert_eq!(request_processor.validator_exit(), Ok(true));
        assert_eq!(exit.load(Ordering::Relaxed), true);
    }

    #[test]
    fn test_rpc_get_identity() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getIdentity"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "identity": bob_pubkey.to_string()
            },
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_get_version() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getVersion"}"#;
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "solana-core": solana_version::version!().to_string()
            },
            "id": 1
        });
        let expected: Response =
            serde_json::from_value(expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_rpc_processor_get_block_commitment() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let bank_forks = new_bank_forks().0;
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(Blockstore::open(&ledger_path).unwrap());

        let commitment_slot0 = BlockCommitment::new([8; MAX_LOCKOUT_HISTORY + 1]);
        let commitment_slot1 = BlockCommitment::new([9; MAX_LOCKOUT_HISTORY + 1]);
        let mut block_commitment: HashMap<u64, BlockCommitment> = HashMap::new();
        block_commitment
            .entry(0)
            .or_insert_with(|| commitment_slot0.clone());
        block_commitment
            .entry(1)
            .or_insert_with(|| commitment_slot1.clone());
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::new(
            block_commitment,
            0,
            42,
            bank_forks.read().unwrap().working_bank(),
            blockstore.clone(),
            0,
            0,
        )));

        let mut config = JsonRpcConfig::default();
        config.enable_validator_exit = true;
        let request_processor = JsonRpcRequestProcessor::new(
            config,
            bank_forks,
            block_commitment_cache,
            blockstore,
            validator_exit,
        );
        assert_eq!(
            request_processor.get_block_commitment(0),
            RpcBlockCommitment {
                commitment: Some(commitment_slot0.commitment),
                total_stake: 42,
            }
        );
        assert_eq!(
            request_processor.get_block_commitment(1),
            RpcBlockCommitment {
                commitment: Some(commitment_slot1.commitment),
                total_stake: 42,
            }
        );
        assert_eq!(
            request_processor.get_block_commitment(2),
            RpcBlockCommitment {
                commitment: None,
                total_stake: 42,
            }
        );
    }

    #[test]
    fn test_rpc_get_block_commitment() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            block_commitment_cache,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getBlockCommitment","params":[0]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let RpcBlockCommitment {
            commitment,
            total_stake,
        } = if let Response::Single(res) = result {
            if let Output::Success(res) = res {
                serde_json::from_value(res.result).unwrap()
            } else {
                panic!("Expected success");
            }
        } else {
            panic!("Expected single response");
        };
        assert_eq!(
            commitment,
            block_commitment_cache
                .read()
                .unwrap()
                .get_block_commitment(0)
                .map(|block_commitment| block_commitment.commitment)
        );
        assert_eq!(total_stake, 10);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getBlockCommitment","params":[2]}"#;
        let res = io.handle_request_sync(&req, meta);
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let commitment_response: RpcBlockCommitment<BlockCommitmentArray> =
            if let Response::Single(res) = result {
                if let Output::Success(res) = res {
                    serde_json::from_value(res.result).unwrap()
                } else {
                    panic!("Expected success");
                }
            } else {
                panic!("Expected single response");
            };
        assert_eq!(commitment_response.commitment, None);
        assert_eq!(commitment_response.total_stake, 10);
    }

    #[test]
    fn test_get_confirmed_block() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            confirmed_block_signatures,
            blockhash,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlock","params":[0]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_block: Option<ConfirmedBlock> =
            serde_json::from_value(result["result"].clone()).unwrap();
        let confirmed_block = confirmed_block.unwrap();
        assert_eq!(confirmed_block.transactions.len(), 3);

        for TransactionWithStatusMeta { transaction, meta } in
            confirmed_block.transactions.into_iter()
        {
            if let EncodedTransaction::Json(transaction) = transaction {
                if transaction.signatures[0] == confirmed_block_signatures[0].to_string() {
                    let meta = meta.unwrap();
                    assert_eq!(transaction.message.recent_blockhash, blockhash.to_string());
                    assert_eq!(meta.status, Ok(()));
                    assert_eq!(meta.err, None);
                } else if transaction.signatures[0] == confirmed_block_signatures[1].to_string() {
                    let meta = meta.unwrap();
                    assert_eq!(
                        meta.err,
                        Some(TransactionError::InstructionError(
                            0,
                            InstructionError::Custom(1)
                        ))
                    );
                    assert_eq!(
                        meta.status,
                        Err(TransactionError::InstructionError(
                            0,
                            InstructionError::Custom(1)
                        ))
                    );
                } else {
                    assert_eq!(meta, None);
                }
            }
        }

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlock","params":[0,"binary"]}"#;
        let res = io.handle_request_sync(&req, meta);
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_block: Option<ConfirmedBlock> =
            serde_json::from_value(result["result"].clone()).unwrap();
        let confirmed_block = confirmed_block.unwrap();
        assert_eq!(confirmed_block.transactions.len(), 3);

        for TransactionWithStatusMeta { transaction, meta } in
            confirmed_block.transactions.into_iter()
        {
            if let EncodedTransaction::Binary(transaction) = transaction {
                let decoded_transaction: Transaction =
                    deserialize(&bs58::decode(&transaction).into_vec().unwrap()).unwrap();
                if decoded_transaction.signatures[0] == confirmed_block_signatures[0] {
                    let meta = meta.unwrap();
                    assert_eq!(decoded_transaction.message.recent_blockhash, blockhash);
                    assert_eq!(meta.status, Ok(()));
                    assert_eq!(meta.err, None);
                } else if decoded_transaction.signatures[0] == confirmed_block_signatures[1] {
                    let meta = meta.unwrap();
                    assert_eq!(
                        meta.err,
                        Some(TransactionError::InstructionError(
                            0,
                            InstructionError::Custom(1)
                        ))
                    );
                    assert_eq!(
                        meta.status,
                        Err(TransactionError::InstructionError(
                            0,
                            InstructionError::Custom(1)
                        ))
                    );
                } else {
                    assert_eq!(meta, None);
                }
            }
        }
    }

    #[test]
    fn test_get_confirmed_blocks() {
        let bob_pubkey = Pubkey::new_rand();
        let roots = vec![0, 1, 3, 4, 8];
        let RpcHandler {
            io,
            meta,
            block_commitment_cache,
            ..
        } = start_rpc_handler_with_tx_and_blockstore(&bob_pubkey, roots.clone(), 0);
        block_commitment_cache
            .write()
            .unwrap()
            .set_largest_confirmed_root(8);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[0]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, roots[1..].to_vec());

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[2]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, vec![3, 4, 8]);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[0,4]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, vec![1, 3, 4]);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[0,7]}"#;
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, vec![1, 3, 4]);

        let req = r#"{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[9,11]}"#;
        let res = io.handle_request_sync(&req, meta);
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, Vec::<Slot>::new());
    }

    #[test]
    fn test_get_block_time() {
        let bob_pubkey = Pubkey::new_rand();
        let base_timestamp = 1_576_183_541;
        let RpcHandler {
            io,
            meta,
            bank,
            block_commitment_cache,
            ..
        } = start_rpc_handler_with_tx_and_blockstore(
            &bob_pubkey,
            vec![1, 2, 3, 4, 5, 6, 7],
            base_timestamp,
        );
        block_commitment_cache
            .write()
            .unwrap()
            .set_largest_confirmed_root(7);

        let slot_duration = slot_duration_from_slots_per_year(bank.slots_per_year());

        let slot = 2;
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockTime","params":[{}]}}"#,
            slot
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(r#"{{"jsonrpc":"2.0","result":{},"id":1}}"#, base_timestamp);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let slot = 7;
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockTime","params":[{}]}}"#,
            slot
        );
        let res = io.handle_request_sync(&req, meta.clone());
        let expected = format!(
            r#"{{"jsonrpc":"2.0","result":{},"id":1}}"#,
            base_timestamp + (5 * slot_duration).as_secs() as i64
        );
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);

        let slot = 12345;
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockTime","params":[{}]}}"#,
            slot
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = r#"{"jsonrpc":"2.0","result":null,"id":1}"#;
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
    }

    #[test]
    fn test_get_vote_accounts() {
        let RpcHandler {
            io,
            meta,
            mut bank,
            bank_forks,
            alice,
            leader_vote_keypair,
            ..
        } = start_rpc_handler_with_tx(&Pubkey::new_rand());

        assert_eq!(bank.vote_accounts().len(), 1);

        // Create a vote account with no stake.
        let alice_vote_keypair = Keypair::new();
        let instructions = vote_instruction::create_account(
            &alice.pubkey(),
            &alice_vote_keypair.pubkey(),
            &VoteInit {
                node_pubkey: alice.pubkey(),
                authorized_voter: alice_vote_keypair.pubkey(),
                authorized_withdrawer: alice_vote_keypair.pubkey(),
                commission: 0,
            },
            bank.get_minimum_balance_for_rent_exemption(VoteState::size_of()),
        );

        let transaction = Transaction::new_signed_instructions(
            &[&alice, &alice_vote_keypair],
            &instructions,
            bank.last_blockhash(),
        );
        bank.process_transaction(&transaction)
            .expect("process transaction");
        assert_eq!(bank.vote_accounts().len(), 2);

        // Check getVoteAccounts: the bootstrap validator vote account will be delinquent as it has
        // stake but has never voted, and the vote account with no stake should not be present.
        {
            let req = r#"{"jsonrpc":"2.0","id":1,"method":"getVoteAccounts"}"#;
            let res = io.handle_request_sync(&req, meta.clone());
            let result: Value = serde_json::from_str(&res.expect("actual response"))
                .expect("actual response deserialization");

            let vote_account_status: RpcVoteAccountStatus =
                serde_json::from_value(result["result"].clone()).unwrap();

            assert!(vote_account_status.current.is_empty());
            assert_eq!(vote_account_status.delinquent.len(), 1);
            for vote_account_info in vote_account_status.delinquent {
                assert_ne!(vote_account_info.activated_stake, 0);
            }
        }

        // Advance bank to the next epoch
        for _ in 0..TEST_SLOTS_PER_EPOCH {
            bank.freeze();

            // Votes
            let instructions = [
                vote_instruction::vote(
                    &leader_vote_keypair.pubkey(),
                    &leader_vote_keypair.pubkey(),
                    Vote {
                        slots: vec![bank.slot()],
                        hash: bank.hash(),
                        timestamp: None,
                    },
                ),
                vote_instruction::vote(
                    &alice_vote_keypair.pubkey(),
                    &alice_vote_keypair.pubkey(),
                    Vote {
                        slots: vec![bank.slot()],
                        hash: bank.hash(),
                        timestamp: None,
                    },
                ),
            ];

            bank = bank_forks.write().unwrap().insert(Bank::new_from_parent(
                &bank,
                &Pubkey::default(),
                bank.slot() + 1,
            ));

            let transaction = Transaction::new_signed_with_payer(
                &instructions,
                Some(&alice.pubkey()),
                &[&alice, &leader_vote_keypair, &alice_vote_keypair],
                bank.last_blockhash(),
            );

            bank.process_transaction(&transaction)
                .expect("process transaction");
        }

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getVoteAccounts","params":{}}}"#,
            json!([CommitmentConfig::recent()])
        );

        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");

        let vote_account_status: RpcVoteAccountStatus =
            serde_json::from_value(result["result"].clone()).unwrap();

        // The vote account with no stake should not be present.
        assert!(vote_account_status.delinquent.is_empty());

        // Both accounts should be active and have voting history.
        assert_eq!(vote_account_status.current.len(), 2);
        //let leader_info = &vote_account_status.current[0];
        let leader_info = vote_account_status
            .current
            .iter()
            .find(|x| x.vote_pubkey == leader_vote_keypair.pubkey().to_string())
            .unwrap();
        assert_ne!(leader_info.activated_stake, 0);
        // Subtract one because the last vote always carries over to the next epoch
        let expected_credits = TEST_SLOTS_PER_EPOCH - MAX_LOCKOUT_HISTORY as u64 - 1;
        assert_eq!(
            leader_info.epoch_credits,
            vec![
                (0, expected_credits, 0),
                (1, expected_credits + 1, expected_credits) // one vote in current epoch
            ]
        );

        // Advance bank with no voting
        bank.freeze();
        bank_forks.write().unwrap().insert(Bank::new_from_parent(
            &bank,
            &Pubkey::default(),
            bank.slot() + TEST_SLOTS_PER_EPOCH,
        ));

        // The leader vote account should now be delinquent, and the other vote account disappears
        // because it's inactive with no stake
        {
            let req = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"getVoteAccounts","params":{}}}"#,
                json!([CommitmentConfig::recent()])
            );

            let res = io.handle_request_sync(&req, meta);
            let result: Value = serde_json::from_str(&res.expect("actual response"))
                .expect("actual response deserialization");

            let vote_account_status: RpcVoteAccountStatus =
                serde_json::from_value(result["result"].clone()).unwrap();

            assert!(vote_account_status.current.is_empty());
            assert_eq!(vote_account_status.delinquent.len(), 1);
            for vote_account_info in vote_account_status.delinquent {
                assert_eq!(
                    vote_account_info.vote_pubkey,
                    leader_vote_keypair.pubkey().to_string()
                );
            }
        }
    }
}
