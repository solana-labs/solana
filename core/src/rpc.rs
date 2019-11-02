//! The `rpc` module implements the Solana RPC interface.

use crate::{
    cluster_info::ClusterInfo,
    confidence::{BankConfidence, ForkConfidenceCache},
    contact_info::ContactInfo,
    packet::PACKET_DATA_SIZE,
    storage_stage::StorageState,
    validator::ValidatorExit,
    version::VERSION,
};
use bincode::{deserialize, serialize};
use jsonrpc_core::{Error, Metadata, Result};
use jsonrpc_derive::rpc;
use solana_client::rpc_request::{RpcEpochInfo, RpcVoteAccountInfo, RpcVoteAccountStatus};
use solana_drone::drone::request_airdrop_transaction;
use solana_ledger::bank_forks::BankForks;
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::Account,
    clock::Slot,
    epoch_schedule::EpochSchedule,
    fee_calculator::FeeCalculator,
    hash::Hash,
    inflation::Inflation,
    pubkey::Pubkey,
    signature::Signature,
    transaction::{self, Transaction},
};
use solana_vote_api::vote_state::{VoteState, MAX_LOCKOUT_HISTORY};
use std::{
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    thread::sleep,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct JsonRpcConfig {
    pub enable_validator_exit: bool, // Enable the 'validatorExit' command
    pub drone_addr: Option<SocketAddr>,
}

impl Default for JsonRpcConfig {
    fn default() -> Self {
        Self {
            enable_validator_exit: false,
            drone_addr: None,
        }
    }
}

#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank_forks: Arc<RwLock<BankForks>>,
    fork_confidence_cache: Arc<RwLock<ForkConfidenceCache>>,
    storage_state: StorageState,
    config: JsonRpcConfig,
    validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
}

impl JsonRpcRequestProcessor {
    fn bank(&self) -> Arc<Bank> {
        self.bank_forks.read().unwrap().working_bank()
    }

    pub fn new(
        storage_state: StorageState,
        config: JsonRpcConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        fork_confidence_cache: Arc<RwLock<ForkConfidenceCache>>,
        validator_exit: &Arc<RwLock<Option<ValidatorExit>>>,
    ) -> Self {
        JsonRpcRequestProcessor {
            bank_forks,
            fork_confidence_cache,
            storage_state,
            config,
            validator_exit: validator_exit.clone(),
        }
    }

    pub fn get_account_info(&self, pubkey: &Pubkey) -> Result<Account> {
        self.bank()
            .get_account(&pubkey)
            .ok_or_else(Error::invalid_request)
    }

    pub fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64> {
        Ok(self.bank().get_minimum_balance_for_rent_exemption(data_len))
    }

    pub fn get_program_accounts(&self, program_id: &Pubkey) -> Result<Vec<(String, Account)>> {
        Ok(self
            .bank()
            .get_program_accounts(&program_id)
            .into_iter()
            .map(|(pubkey, account)| (pubkey.to_string(), account))
            .collect())
    }

    pub fn get_inflation(&self) -> Result<Inflation> {
        Ok(self.bank().inflation())
    }

    pub fn get_epoch_schedule(&self) -> Result<EpochSchedule> {
        Ok(*self.bank().epoch_schedule())
    }

    pub fn get_balance(&self, pubkey: &Pubkey) -> u64 {
        self.bank().get_balance(&pubkey)
    }

    fn get_recent_blockhash(&self) -> (String, FeeCalculator) {
        let (blockhash, fee_calculator) = self.bank().confirmed_last_blockhash();
        (blockhash.to_string(), fee_calculator)
    }

    fn get_block_confidence(&self, block: u64) -> (Option<BankConfidence>, u64) {
        let r_fork_confidence = self.fork_confidence_cache.read().unwrap();
        (
            r_fork_confidence.get_fork_confidence(block).cloned(),
            r_fork_confidence.total_stake(),
        )
    }

    pub fn get_signature_status(&self, signature: Signature) -> Option<transaction::Result<()>> {
        self.get_signature_confirmation_status(signature)
            .map(|x| x.1)
    }

    pub fn get_signature_confirmations(&self, signature: Signature) -> Option<usize> {
        self.get_signature_confirmation_status(signature)
            .map(|x| x.0)
    }

    pub fn get_signature_confirmation_status(
        &self,
        signature: Signature,
    ) -> Option<(usize, transaction::Result<()>)> {
        self.bank().get_signature_confirmation_status(&signature)
    }

    fn get_slot(&self) -> Result<u64> {
        Ok(self.bank().slot())
    }

    fn get_slot_leader(&self) -> Result<String> {
        Ok(self.bank().collector_id().to_string())
    }

    fn get_transaction_count(&self) -> Result<u64> {
        Ok(self.bank().transaction_count() as u64)
    }

    fn get_total_supply(&self) -> Result<u64> {
        Ok(self.bank().capitalization())
    }

    fn get_vote_accounts(&self) -> Result<RpcVoteAccountStatus> {
        let bank = self.bank();
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
        Ok(RpcVoteAccountStatus {
            current: current_vote_accounts,
            delinquent: delinquent_vote_accounts,
        })
    }

    fn get_storage_turn_rate(&self) -> Result<u64> {
        Ok(self.storage_state.get_storage_turn_rate())
    }

    fn get_storage_turn(&self) -> Result<(String, u64)> {
        Ok((
            self.storage_state.get_storage_blockhash().to_string(),
            self.storage_state.get_slot(),
        ))
    }

    fn get_slots_per_segment(&self) -> Result<u64> {
        Ok(self.bank().slots_per_segment())
    }

    fn get_storage_pubkeys_for_slot(&self, slot: Slot) -> Result<Vec<Pubkey>> {
        Ok(self
            .storage_state
            .get_pubkeys_for_slot(slot, &self.bank_forks))
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
}

fn get_tpu_addr(cluster_info: &Arc<RwLock<ClusterInfo>>) -> Result<SocketAddr> {
    let contact_info = cluster_info.read().unwrap().my_data();
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
    pub cluster_info: Arc<RwLock<ClusterInfo>>,
    pub genesis_blockhash: Hash,
}
impl Metadata for Meta {}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RpcContactInfo {
    /// Pubkey of the node as a base-58 string
    pub pubkey: String,
    /// Gossip port
    pub gossip: Option<SocketAddr>,
    /// Tpu port
    pub tpu: Option<SocketAddr>,
    /// JSON RPC port
    pub rpc: Option<SocketAddr>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct RpcVersionInfo {
    /// The current version of solana-core
    pub solana_core: String,
}

#[rpc(server)]
pub trait RpcSol {
    type Metadata;

    #[rpc(meta, name = "confirmTransaction")]
    fn confirm_transaction(&self, _: Self::Metadata, _: String) -> Result<bool>;

    #[rpc(meta, name = "getAccountInfo")]
    fn get_account_info(&self, _: Self::Metadata, _: String) -> Result<Account>;

    #[rpc(meta, name = "getProgramAccounts")]
    fn get_program_accounts(&self, _: Self::Metadata, _: String) -> Result<Vec<(String, Account)>>;

    #[rpc(meta, name = "getMinimumBalanceForRentExemption")]
    fn get_minimum_balance_for_rent_exemption(&self, _: Self::Metadata, _: usize) -> Result<u64>;

    #[rpc(meta, name = "getInflation")]
    fn get_inflation(&self, _: Self::Metadata) -> Result<Inflation>;

    #[rpc(meta, name = "getEpochSchedule")]
    fn get_epoch_schedule(&self, _: Self::Metadata) -> Result<EpochSchedule>;

    #[rpc(meta, name = "getBalance")]
    fn get_balance(&self, _: Self::Metadata, _: String) -> Result<u64>;

    #[rpc(meta, name = "getClusterNodes")]
    fn get_cluster_nodes(&self, _: Self::Metadata) -> Result<Vec<RpcContactInfo>>;

    #[rpc(meta, name = "getEpochInfo")]
    fn get_epoch_info(&self, _: Self::Metadata) -> Result<RpcEpochInfo>;

    #[rpc(meta, name = "getBlockConfidence")]
    fn get_block_confidence(
        &self,
        _: Self::Metadata,
        _: u64,
    ) -> Result<(Option<BankConfidence>, u64)>;

    #[rpc(meta, name = "getGenesisBlockhash")]
    fn get_genesis_blockhash(&self, _: Self::Metadata) -> Result<String>;

    #[rpc(meta, name = "getLeaderSchedule")]
    fn get_leader_schedule(&self, _: Self::Metadata) -> Result<Option<Vec<String>>>;

    #[rpc(meta, name = "getRecentBlockhash")]
    fn get_recent_blockhash(&self, _: Self::Metadata) -> Result<(String, FeeCalculator)>;

    #[rpc(meta, name = "getSignatureStatus")]
    fn get_signature_status(
        &self,
        _: Self::Metadata,
        _: String,
    ) -> Result<Option<transaction::Result<()>>>;

    #[rpc(meta, name = "getSlot")]
    fn get_slot(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "getTransactionCount")]
    fn get_transaction_count(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "getTotalSupply")]
    fn get_total_supply(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "requestAirdrop")]
    fn request_airdrop(&self, _: Self::Metadata, _: String, _: u64) -> Result<String>;

    #[rpc(meta, name = "sendTransaction")]
    fn send_transaction(&self, _: Self::Metadata, _: Vec<u8>) -> Result<String>;

    #[rpc(meta, name = "getSlotLeader")]
    fn get_slot_leader(&self, _: Self::Metadata) -> Result<String>;

    #[rpc(meta, name = "getVoteAccounts")]
    fn get_vote_accounts(&self, _: Self::Metadata) -> Result<RpcVoteAccountStatus>;

    #[rpc(meta, name = "getStorageTurnRate")]
    fn get_storage_turn_rate(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "getStorageTurn")]
    fn get_storage_turn(&self, _: Self::Metadata) -> Result<(String, u64)>;

    #[rpc(meta, name = "getSlotsPerSegment")]
    fn get_slots_per_segment(&self, _: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "getStoragePubkeysForSlot")]
    fn get_storage_pubkeys_for_slot(&self, _: Self::Metadata, _: u64) -> Result<Vec<Pubkey>>;

    #[rpc(meta, name = "validatorExit")]
    fn validator_exit(&self, _: Self::Metadata) -> Result<bool>;

    #[rpc(meta, name = "getNumBlocksSinceSignatureConfirmation")]
    fn get_num_blocks_since_signature_confirmation(
        &self,
        _: Self::Metadata,
        _: String,
    ) -> Result<Option<usize>>;

    #[rpc(meta, name = "getSignatureConfirmation")]
    fn get_signature_confirmation(
        &self,
        _: Self::Metadata,
        _: String,
    ) -> Result<Option<(usize, transaction::Result<()>)>>;

    #[rpc(meta, name = "getVersion")]
    fn get_version(&self, _: Self::Metadata) -> Result<RpcVersionInfo>;

    #[rpc(meta, name = "setLogFilter")]
    fn set_log_filter(&self, _: Self::Metadata, _: String) -> Result<()>;
}

pub struct RpcSolImpl;
impl RpcSol for RpcSolImpl {
    type Metadata = Meta;

    fn confirm_transaction(&self, meta: Self::Metadata, id: String) -> Result<bool> {
        debug!("confirm_transaction rpc request received: {:?}", id);
        self.get_signature_status(meta, id).map(|status_option| {
            if status_option.is_none() {
                return false;
            }
            status_option.unwrap().is_ok()
        })
    }

    fn get_account_info(&self, meta: Self::Metadata, id: String) -> Result<Account> {
        debug!("get_account_info rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        meta.request_processor
            .read()
            .unwrap()
            .get_account_info(&pubkey)
    }

    fn get_minimum_balance_for_rent_exemption(
        &self,
        meta: Self::Metadata,
        data_len: usize,
    ) -> Result<u64> {
        debug!(
            "get_minimum_balance_for_rent_exemption rpc request received: {:?}",
            data_len
        );
        meta.request_processor
            .read()
            .unwrap()
            .get_minimum_balance_for_rent_exemption(data_len)
    }

    fn get_program_accounts(
        &self,
        meta: Self::Metadata,
        id: String,
    ) -> Result<Vec<(String, Account)>> {
        debug!("get_program_accounts rpc request received: {:?}", id);
        let program_id = verify_pubkey(id)?;
        meta.request_processor
            .read()
            .unwrap()
            .get_program_accounts(&program_id)
    }

    fn get_inflation(&self, meta: Self::Metadata) -> Result<Inflation> {
        debug!("get_inflation rpc request received");
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_inflation()
            .unwrap())
    }

    fn get_epoch_schedule(&self, meta: Self::Metadata) -> Result<EpochSchedule> {
        debug!("get_epoch_schedule rpc request received");
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_epoch_schedule()
            .unwrap())
    }

    fn get_balance(&self, meta: Self::Metadata, id: String) -> Result<u64> {
        debug!("get_balance rpc request received: {:?}", id);
        let pubkey = verify_pubkey(id)?;
        Ok(meta.request_processor.read().unwrap().get_balance(&pubkey))
    }

    fn get_cluster_nodes(&self, meta: Self::Metadata) -> Result<Vec<RpcContactInfo>> {
        let cluster_info = meta.cluster_info.read().unwrap();
        fn valid_address_or_none(addr: &SocketAddr) -> Option<SocketAddr> {
            if ContactInfo::is_valid_address(addr) {
                Some(*addr)
            } else {
                None
            }
        }
        Ok(cluster_info
            .all_peers()
            .iter()
            .filter_map(|(contact_info, _)| {
                if ContactInfo::is_valid_address(&contact_info.gossip) {
                    Some(RpcContactInfo {
                        pubkey: contact_info.id.to_string(),
                        gossip: Some(contact_info.gossip),
                        tpu: valid_address_or_none(&contact_info.tpu),
                        rpc: valid_address_or_none(&contact_info.rpc),
                    })
                } else {
                    None // Exclude spy nodes
                }
            })
            .collect())
    }

    fn get_epoch_info(&self, meta: Self::Metadata) -> Result<RpcEpochInfo> {
        let bank = meta.request_processor.read().unwrap().bank();
        let epoch_schedule = bank.epoch_schedule();
        let (epoch, slot_index) = epoch_schedule.get_epoch_and_slot_index(bank.slot());
        let slot = bank.slot();
        Ok(RpcEpochInfo {
            epoch,
            slot_index,
            slots_in_epoch: epoch_schedule.get_slots_in_epoch(epoch),
            absolute_slot: slot,
        })
    }

    fn get_block_confidence(
        &self,
        meta: Self::Metadata,
        block: u64,
    ) -> Result<(Option<BankConfidence>, u64)> {
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_block_confidence(block))
    }

    fn get_genesis_blockhash(&self, meta: Self::Metadata) -> Result<String> {
        debug!("get_genesis_blockhash rpc request received");
        Ok(meta.genesis_blockhash.to_string())
    }

    fn get_leader_schedule(&self, meta: Self::Metadata) -> Result<Option<Vec<String>>> {
        let bank = meta.request_processor.read().unwrap().bank();
        Ok(
            solana_ledger::leader_schedule_utils::leader_schedule(bank.epoch(), &bank).map(
                |leader_schedule| {
                    leader_schedule
                        .get_slot_leaders()
                        .iter()
                        .map(|pubkey| pubkey.to_string())
                        .collect()
                },
            ),
        )
    }

    fn get_recent_blockhash(&self, meta: Self::Metadata) -> Result<(String, FeeCalculator)> {
        debug!("get_recent_blockhash rpc request received");
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_recent_blockhash())
    }

    fn get_signature_status(
        &self,
        meta: Self::Metadata,
        id: String,
    ) -> Result<Option<transaction::Result<()>>> {
        self.get_signature_confirmation(meta, id)
            .map(|res| res.map(|x| x.1))
    }

    fn get_slot(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor.read().unwrap().get_slot()
    }

    fn get_num_blocks_since_signature_confirmation(
        &self,
        meta: Self::Metadata,
        id: String,
    ) -> Result<Option<usize>> {
        self.get_signature_confirmation(meta, id)
            .map(|res| res.map(|x| x.0))
    }

    fn get_signature_confirmation(
        &self,
        meta: Self::Metadata,
        id: String,
    ) -> Result<Option<(usize, transaction::Result<()>)>> {
        debug!("get_signature_confirmation rpc request received: {:?}", id);
        let signature = verify_signature(&id)?;
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_signature_confirmation_status(signature))
    }

    fn get_transaction_count(&self, meta: Self::Metadata) -> Result<u64> {
        debug!("get_transaction_count rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_transaction_count()
    }

    fn get_total_supply(&self, meta: Self::Metadata) -> Result<u64> {
        debug!("get_total_supply rpc request received");
        meta.request_processor.read().unwrap().get_total_supply()
    }

    fn request_airdrop(&self, meta: Self::Metadata, id: String, lamports: u64) -> Result<String> {
        trace!("request_airdrop id={} lamports={}", id, lamports);

        let drone_addr = meta
            .request_processor
            .read()
            .unwrap()
            .config
            .drone_addr
            .ok_or_else(Error::invalid_request)?;
        let pubkey = verify_pubkey(id)?;

        let blockhash = meta
            .request_processor
            .read()
            .unwrap()
            .bank()
            .confirmed_last_blockhash()
            .0;
        let transaction = request_airdrop_transaction(&drone_addr, &pubkey, lamports, blockhash)
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
        loop {
            signature_status = meta
                .request_processor
                .read()
                .unwrap()
                .get_signature_status(signature);

            if signature_status == Some(Ok(())) {
                info!("airdrop signature ok");
                return Ok(signature.to_string());
            } else if now.elapsed().as_secs() > 5 {
                info!("airdrop signature timeout");
                return Err(Error::internal_error());
            }
            sleep(Duration::from_millis(100));
        }
    }

    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        let tx: Transaction = deserialize(&data).map_err(|err| {
            info!("send_transaction: deserialize error: {:?}", err);
            Error::invalid_request()
        })?;
        if data.len() >= PACKET_DATA_SIZE {
            info!(
                "send_transaction: transaction too large: {} bytes (max: {} bytes)",
                data.len(),
                PACKET_DATA_SIZE
            );
            return Err(Error::invalid_request());
        }
        let transactions_socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let tpu_addr = get_tpu_addr(&meta.cluster_info)?;
        trace!("send_transaction: leader is {:?}", &tpu_addr);
        transactions_socket
            .send_to(&data, tpu_addr)
            .map_err(|err| {
                info!("send_transaction: send_to error: {:?}", err);
                Error::internal_error()
            })?;
        let signature = tx.signatures[0].to_string();
        trace!(
            "send_transaction: sent {} bytes, signature={}",
            data.len(),
            signature
        );
        Ok(signature)
    }

    fn get_slot_leader(&self, meta: Self::Metadata) -> Result<String> {
        meta.request_processor.read().unwrap().get_slot_leader()
    }

    fn get_vote_accounts(&self, meta: Self::Metadata) -> Result<RpcVoteAccountStatus> {
        meta.request_processor.read().unwrap().get_vote_accounts()
    }

    fn get_storage_turn_rate(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_turn_rate()
    }

    fn get_storage_turn(&self, meta: Self::Metadata) -> Result<(String, u64)> {
        meta.request_processor.read().unwrap().get_storage_turn()
    }

    fn get_slots_per_segment(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor
            .read()
            .unwrap()
            .get_slots_per_segment()
    }

    fn get_storage_pubkeys_for_slot(
        &self,
        meta: Self::Metadata,
        slot: Slot,
    ) -> Result<Vec<Pubkey>> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_pubkeys_for_slot(slot)
    }

    fn validator_exit(&self, meta: Self::Metadata) -> Result<bool> {
        meta.request_processor.read().unwrap().validator_exit()
    }

    fn get_version(&self, _: Self::Metadata) -> Result<RpcVersionInfo> {
        Ok(RpcVersionInfo {
            solana_core: VERSION.to_string(),
        })
    }

    fn set_log_filter(&self, _: Self::Metadata, filter: String) -> Result<()> {
        solana_logger::setup_with_filter(&filter);
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        contact_info::ContactInfo,
        genesis_utils::{create_genesis_block, GenesisBlockInfo},
    };
    use jsonrpc_core::{MetaIoHandler, Output, Response, Value};
    use solana_sdk::{
        fee_calculator::DEFAULT_BURN_PERCENT,
        hash::{hash, Hash},
        instruction::InstructionError,
        signature::{Keypair, KeypairUtil},
        system_transaction,
        transaction::TransactionError,
    };
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };

    const TEST_MINT_LAMPORTS: u64 = 10_000;

    struct RpcHandler {
        io: MetaIoHandler<Meta>,
        meta: Meta,
        bank: Arc<Bank>,
        blockhash: Hash,
        alice: Keypair,
        leader_pubkey: Pubkey,
        fork_confidence_cache: Arc<RwLock<ForkConfidenceCache>>,
    }

    fn start_rpc_handler_with_tx(pubkey: &Pubkey) -> RpcHandler {
        let (bank_forks, alice) = new_bank_forks();
        let bank = bank_forks.read().unwrap().working_bank();

        let confidence_slot0 = BankConfidence::new([8; MAX_LOCKOUT_HISTORY]);
        let confidence_slot1 = BankConfidence::new([9; MAX_LOCKOUT_HISTORY]);
        let mut bank_confidence: HashMap<u64, BankConfidence> = HashMap::new();
        bank_confidence.entry(0).or_insert(confidence_slot0.clone());
        bank_confidence.entry(1).or_insert(confidence_slot1.clone());
        let fork_confidence_cache =
            Arc::new(RwLock::new(ForkConfidenceCache::new(bank_confidence, 42)));

        let leader_pubkey = *bank.collector_id();
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);

        let blockhash = bank.confirmed_last_blockhash().0;
        let tx = system_transaction::transfer(&alice, pubkey, 20, blockhash);
        bank.process_transaction(&tx).expect("process transaction");

        let tx = system_transaction::transfer(&alice, &alice.pubkey(), 20, blockhash);
        let _ = bank.process_transaction(&tx);

        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            StorageState::default(),
            JsonRpcConfig::default(),
            bank_forks,
            fork_confidence_cache.clone(),
            &validator_exit,
        )));
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            ContactInfo::default(),
        )));

        cluster_info
            .write()
            .unwrap()
            .insert_info(ContactInfo::new_with_pubkey_socketaddr(
                &leader_pubkey,
                &socketaddr!("127.0.0.1:1234"),
            ));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor,
            cluster_info,
            genesis_blockhash: Hash::default(),
        };
        RpcHandler {
            io,
            meta,
            bank,
            blockhash,
            alice,
            leader_pubkey,
            fork_confidence_cache,
        }
    }

    #[test]
    fn test_rpc_request_processor_new() {
        let bob_pubkey = Pubkey::new_rand();
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let (bank_forks, alice) = new_bank_forks();
        let bank = bank_forks.read().unwrap().working_bank();
        let fork_confidence_cache = Arc::new(RwLock::new(ForkConfidenceCache::default()));
        let request_processor = JsonRpcRequestProcessor::new(
            StorageState::default(),
            JsonRpcConfig::default(),
            bank_forks,
            fork_confidence_cache,
            &validator_exit,
        );
        thread::spawn(move || {
            let blockhash = bank.confirmed_last_blockhash().0;
            let tx = system_transaction::transfer(&alice, &bob_pubkey, 20, blockhash);
            bank.process_transaction(&tx).expect("process transaction");
        })
        .join()
        .unwrap();
        assert_eq!(request_processor.get_transaction_count().unwrap(), 1);
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
        let expected = format!(r#"{{"jsonrpc":"2.0","result":20,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
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

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getClusterNodes"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");

        let expected = format!(
            r#"{{"jsonrpc":"2.0","result":[{{"pubkey": "{}", "gossip": "127.0.0.1:1235", "tpu": "127.0.0.1:1234", "rpc": "127.0.0.1:8899"}}],"id":1}}"#,
            leader_pubkey,
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

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getSlotLeader"}}"#);
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

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getTransactionCount"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = format!(r#"{{"jsonrpc":"2.0","result":1,"id":1}}"#);
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

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getTotalSupply"}}"#);
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

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getInflation"}}"#);
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

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getEpochSchedule"}}"#);
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
    fn test_rpc_get_account_info() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getAccountInfo","params":["{}"]}}"#,
            bob_pubkey
        );
        let res = io.handle_request_sync(&req, meta);
        let expected = r#"{
            "jsonrpc":"2.0",
            "result":{
                "owner": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
                "lamports": 20,
                "data": [],
                "executable": false,
                "rent_epoch": 0
            },
            "id":1}
        "#;
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
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
                "result":[["{}", {{
                    "owner": {:?},
                    "lamports": 20,
                    "data": [],
                    "executable": false,
                    "rent_epoch": 0
                }}]],
                "id":1}}
            "#,
            bob.pubkey(),
            new_program_id.as_ref()
        );
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        assert_eq!(expected, result);
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
        let expected = format!(r#"{{"jsonrpc":"2.0","result":true,"id":1}}"#);
        let expected: Response =
            serde_json::from_str(&expected).expect("expected response deserialization");
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
        let tx = system_transaction::transfer(&alice, &alice.pubkey(), 20, blockhash);
        let req = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"getSignatureStatus","params":["{}"]}}"#,
            tx.signatures[0]
        );
        let res = io.handle_request_sync(&req, meta);
        let expected_res: Option<transaction::Result<()>> = Some(Err(
            TransactionError::InstructionError(0, InstructionError::DuplicateAccountIndex),
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
    fn test_rpc_get_recent_blockhash() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            blockhash,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getRecentBlockhash"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": [ blockhash.to_string(), {
                "burnPercent": DEFAULT_BURN_PERCENT,
                "lamportsPerSignature": 0,
                "maxLamportsPerSignature": 0,
                "minLamportsPerSignature": 0,
                "targetLamportsPerSignature": 0,
                "targetSignaturesPerSlot": 0
            }],
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

        // Expect internal error because no drone is available
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
        let fork_confidence_cache = Arc::new(RwLock::new(ForkConfidenceCache::default()));

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: {
                let request_processor = JsonRpcRequestProcessor::new(
                    StorageState::default(),
                    JsonRpcConfig::default(),
                    new_bank_forks().0,
                    fork_confidence_cache,
                    &validator_exit,
                );
                Arc::new(RwLock::new(request_processor))
            },
            cluster_info: Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
                ContactInfo::default(),
            ))),
            genesis_blockhash: Hash::default(),
        };

        let req =
            r#"{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":[[0,0,0,0,0,0,0,0]]}"#;
        let res = io.handle_request_sync(req, meta.clone());
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
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            ContactInfo::new_with_socketaddr(&socketaddr!("127.0.0.1:1234")),
        )));
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

    fn new_bank_forks() -> (Arc<RwLock<BankForks>>, Keypair) {
        let GenesisBlockInfo {
            mut genesis_block,
            mint_keypair,
            ..
        } = create_genesis_block(TEST_MINT_LAMPORTS);

        genesis_block.rent.lamports_per_byte_year = 50;
        genesis_block.rent.exemption_threshold = 2.0;

        let bank = Bank::new(&genesis_block);
        (
            Arc::new(RwLock::new(BankForks::new(bank.slot(), bank))),
            mint_keypair,
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
        let fork_confidence_cache = Arc::new(RwLock::new(ForkConfidenceCache::default()));
        let request_processor = JsonRpcRequestProcessor::new(
            StorageState::default(),
            JsonRpcConfig::default(),
            new_bank_forks().0,
            fork_confidence_cache,
            &validator_exit,
        );
        assert_eq!(request_processor.validator_exit(), Ok(false));
        assert_eq!(exit.load(Ordering::Relaxed), false);
    }

    #[test]
    fn test_rpc_request_processor_allow_validator_exit_config() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let fork_confidence_cache = Arc::new(RwLock::new(ForkConfidenceCache::default()));
        let mut config = JsonRpcConfig::default();
        config.enable_validator_exit = true;
        let request_processor = JsonRpcRequestProcessor::new(
            StorageState::default(),
            config,
            new_bank_forks().0,
            fork_confidence_cache,
            &validator_exit,
        );
        assert_eq!(request_processor.validator_exit(), Ok(true));
        assert_eq!(exit.load(Ordering::Relaxed), true);
    }

    #[test]
    fn test_rpc_get_version() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler { io, meta, .. } = start_rpc_handler_with_tx(&bob_pubkey);

        let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getVersion"}}"#);
        let res = io.handle_request_sync(&req, meta);
        let expected = json!({
            "jsonrpc": "2.0",
            "result": {
                "solana-core": VERSION
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
    fn test_rpc_processor_get_block_confidence() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let confidence_slot0 = BankConfidence::new([8; MAX_LOCKOUT_HISTORY]);
        let confidence_slot1 = BankConfidence::new([9; MAX_LOCKOUT_HISTORY]);
        let mut bank_confidence: HashMap<u64, BankConfidence> = HashMap::new();
        bank_confidence.entry(0).or_insert(confidence_slot0.clone());
        bank_confidence.entry(1).or_insert(confidence_slot1.clone());
        let fork_confidence_cache =
            Arc::new(RwLock::new(ForkConfidenceCache::new(bank_confidence, 42)));

        let mut config = JsonRpcConfig::default();
        config.enable_validator_exit = true;
        let request_processor = JsonRpcRequestProcessor::new(
            StorageState::default(),
            config,
            new_bank_forks().0,
            fork_confidence_cache,
            &validator_exit,
        );
        assert_eq!(
            request_processor.get_block_confidence(0),
            (Some(confidence_slot0), 42)
        );
        assert_eq!(
            request_processor.get_block_confidence(1),
            (Some(confidence_slot1), 42)
        );
        assert_eq!(request_processor.get_block_confidence(2), (None, 42));
    }

    #[test]
    fn test_rpc_get_block_confidence() {
        let bob_pubkey = Pubkey::new_rand();
        let RpcHandler {
            io,
            meta,
            fork_confidence_cache,
            ..
        } = start_rpc_handler_with_tx(&bob_pubkey);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockConfidence","params":[0]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let (confidence, total_staked): (Option<BankConfidence>, u64) =
            if let Response::Single(res) = result {
                if let Output::Success(res) = res {
                    serde_json::from_value(res.result).unwrap()
                } else {
                    panic!("Expected success");
                }
            } else {
                panic!("Expected single response");
            };
        assert_eq!(
            confidence,
            fork_confidence_cache
                .read()
                .unwrap()
                .get_fork_confidence(0)
                .cloned()
        );
        assert_eq!(total_staked, 42);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockConfidence","params":[2]}}"#);
        let res = io.handle_request_sync(&req, meta);
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let (confidence, total_staked): (Option<BankConfidence>, u64) =
            if let Response::Single(res) = result {
                if let Output::Success(res) = res {
                    serde_json::from_value(res.result).unwrap()
                } else {
                    panic!("Expected success");
                }
            } else {
                panic!("Expected single response");
            };
        assert_eq!(confidence, None);
        assert_eq!(total_staked, 42);
    }
}
