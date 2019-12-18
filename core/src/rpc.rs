//! The `rpc` module implements the Solana RPC interface.

use crate::{
    cluster_info::ClusterInfo,
    commitment::{BlockCommitment, BlockCommitmentCache},
    contact_info::ContactInfo,
    packet::PACKET_DATA_SIZE,
    storage_stage::StorageState,
    validator::ValidatorExit,
};
use bincode::serialize;
use jsonrpc_core::{Error, Metadata, Result};
use jsonrpc_derive::rpc;
use solana_client::rpc_request::{
    Response, RpcConfirmedBlock, RpcContactInfo, RpcEpochInfo, RpcLeaderSchedule,
    RpcResponseContext, RpcVersionInfo, RpcVoteAccountInfo, RpcVoteAccountStatus,
};
use solana_faucet::faucet::request_airdrop_transaction;
use solana_ledger::{
    bank_forks::BankForks, blocktree::Blocktree, rooted_slot_iterator::RootedSlotIterator,
};
use solana_runtime::bank::Bank;
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    commitment_config::{CommitmentConfig, CommitmentLevel},
    epoch_schedule::EpochSchedule,
    fee_calculator::FeeCalculator,
    hash::Hash,
    inflation::Inflation,
    pubkey::Pubkey,
    signature::Signature,
    timing::slot_duration_from_slots_per_year,
    transaction::{self, Transaction},
};
use solana_vote_program::vote_state::{VoteState, MAX_LOCKOUT_HISTORY};
use std::{
    collections::HashMap,
    net::{SocketAddr, UdpSocket},
    sync::{Arc, RwLock},
    thread::sleep,
    time::{Duration, Instant},
};

type RpcResponse<T> = Result<Response<T>>;

fn new_response<T>(bank: &Bank, value: T) -> RpcResponse<T> {
    let context = RpcResponseContext { slot: bank.slot() };
    Ok(Response { context, value })
}

#[derive(Debug, Clone)]
pub struct JsonRpcConfig {
    pub enable_validator_exit: bool, // Enable the 'validatorExit' command
    pub faucet_addr: Option<SocketAddr>,
}

impl Default for JsonRpcConfig {
    fn default() -> Self {
        Self {
            enable_validator_exit: false,
            faucet_addr: None,
        }
    }
}

#[derive(Clone)]
pub struct JsonRpcRequestProcessor {
    bank_forks: Arc<RwLock<BankForks>>,
    block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
    blocktree: Arc<Blocktree>,
    config: JsonRpcConfig,
    storage_state: StorageState,
    validator_exit: Arc<RwLock<Option<ValidatorExit>>>,
}

impl JsonRpcRequestProcessor {
    fn bank(&self, commitment: Option<CommitmentConfig>) -> Arc<Bank> {
        debug!("RPC commitment_config: {:?}", commitment);
        let r_bank_forks = self.bank_forks.read().unwrap();
        if commitment.is_some() && commitment.unwrap().commitment == CommitmentLevel::Recent {
            let bank = r_bank_forks.working_bank();
            debug!("RPC using working_bank: {:?}", bank.slot());
            bank
        } else {
            let slot = r_bank_forks.root();
            debug!("RPC using block: {:?}", slot);
            r_bank_forks.get(slot).cloned().unwrap()
        }
    }

    pub fn new(
        config: JsonRpcConfig,
        bank_forks: Arc<RwLock<BankForks>>,
        block_commitment_cache: Arc<RwLock<BlockCommitmentCache>>,
        blocktree: Arc<Blocktree>,
        storage_state: StorageState,
        validator_exit: &Arc<RwLock<Option<ValidatorExit>>>,
    ) -> Self {
        JsonRpcRequestProcessor {
            config,
            bank_forks,
            block_commitment_cache,
            blocktree,
            storage_state,
            validator_exit: validator_exit.clone(),
        }
    }

    pub fn get_account_info(
        &self,
        pubkey: Result<Pubkey>,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<Option<Account>> {
        let bank = &*self.bank(commitment);
        match pubkey {
            Ok(key) => new_response(bank, bank.get_account(&key)),
            Err(e) => Err(e),
        }
    }

    pub fn get_minimum_balance_for_rent_exemption(
        &self,
        data_len: usize,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64> {
        Ok(self
            .bank(commitment)
            .get_minimum_balance_for_rent_exemption(data_len))
    }

    pub fn get_program_accounts(
        &self,
        program_id: &Pubkey,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Vec<(String, Account)>> {
        Ok(self
            .bank(commitment)
            .get_program_accounts(&program_id)
            .into_iter()
            .map(|(pubkey, account)| (pubkey.to_string(), account))
            .collect())
    }

    pub fn get_inflation(&self, commitment: Option<CommitmentConfig>) -> Result<Inflation> {
        Ok(self.bank(commitment).inflation())
    }

    pub fn get_epoch_schedule(&self) -> Result<EpochSchedule> {
        // Since epoch schedule data comes from the genesis config, any commitment level should be
        // fine
        Ok(*self.bank(None).epoch_schedule())
    }

    pub fn get_balance(
        &self,
        pubkey: Result<Pubkey>,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<u64> {
        let bank = &*self.bank(commitment);
        match pubkey {
            Ok(key) => new_response(bank, bank.get_balance(&key)),
            Err(e) => Err(e),
        }
    }

    fn get_recent_blockhash(
        &self,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<(String, FeeCalculator)> {
        let bank = &*self.bank(commitment);
        let (blockhash, fee_calculator) = bank.confirmed_last_blockhash();
        new_response(bank, (blockhash.to_string(), fee_calculator))
    }

    pub fn confirm_transaction(
        &self,
        signature: Result<Signature>,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<bool> {
        let bank = &*self.bank(commitment);
        match signature {
            Err(e) => Err(e),
            Ok(sig) => {
                let status = bank.get_signature_confirmation_status(&sig);
                match status {
                    Some((_, result)) => new_response(bank, result.is_ok()),
                    None => new_response(bank, false),
                }
            }
        }
    }

    fn get_block_commitment(&self, block: Slot) -> (Option<BlockCommitment>, u64) {
        let r_block_commitment = self.block_commitment_cache.read().unwrap();
        (
            r_block_commitment.get_block_commitment(block).cloned(),
            r_block_commitment.total_stake(),
        )
    }

    pub fn get_signature_confirmation_status(
        &self,
        signature: Signature,
        commitment: Option<CommitmentConfig>,
    ) -> Option<(usize, transaction::Result<()>)> {
        self.bank(commitment)
            .get_signature_confirmation_status(&signature)
    }

    fn get_slot(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment).slot())
    }

    fn get_slot_leader(&self, commitment: Option<CommitmentConfig>) -> Result<String> {
        Ok(self.bank(commitment).collector_id().to_string())
    }

    fn get_transaction_count(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment).transaction_count() as u64)
    }

    fn get_total_supply(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment).capitalization())
    }

    fn get_vote_accounts(
        &self,
        commitment: Option<CommitmentConfig>,
    ) -> Result<RpcVoteAccountStatus> {
        let bank = self.bank(commitment);
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

    fn get_storage_turn_rate(&self) -> Result<u64> {
        Ok(self.storage_state.get_storage_turn_rate())
    }

    fn get_storage_turn(&self) -> Result<(String, u64)> {
        Ok((
            self.storage_state.get_storage_blockhash().to_string(),
            self.storage_state.get_slot(),
        ))
    }

    fn get_slots_per_segment(&self, commitment: Option<CommitmentConfig>) -> Result<u64> {
        Ok(self.bank(commitment).slots_per_segment())
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

    pub fn get_confirmed_block(&self, slot: Slot) -> Result<Option<RpcConfirmedBlock>> {
        Ok(self.blocktree.get_confirmed_block(slot).ok())
    }

    pub fn get_confirmed_blocks(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> Result<Vec<Slot>> {
        let end_slot = end_slot.unwrap_or_else(|| self.bank(None).slot());
        if end_slot < start_slot {
            return Ok(vec![]);
        }

        let start_slot = (start_slot..end_slot).find(|&slot| self.blocktree.is_root(slot));
        if let Some(start_slot) = start_slot {
            let mut slots: Vec<Slot> = RootedSlotIterator::new(start_slot, &self.blocktree)
                .unwrap()
                .map(|(slot, _)| slot)
                .collect();
            slots.retain(|&x| x <= end_slot);
            Ok(slots)
        } else {
            Ok(vec![])
        }
    }

    pub fn get_block_time(&self, slot: Slot) -> Result<Option<UnixTimestamp>> {
        // This calculation currently assumes that bank.slots_per_year will remain unchanged after
        // genesis (ie. that this bank's slot_per_year will be applicable to any rooted slot being
        // queried). If these values will be variable in the future, those timing parameters will
        // need to be stored persistently, and the slot_duration calculation will likely need to be
        // moved upstream into blocktree. Also, an explicit commitment level will need to be set.
        let bank = self.bank(None);
        let slot_duration = slot_duration_from_slots_per_year(bank.slots_per_year());
        let epoch = bank.epoch_schedule().get_epoch(slot);
        let stakes = HashMap::new();
        let stakes = bank.epoch_vote_accounts(epoch).unwrap_or(&stakes);

        Ok(self.blocktree.get_block_time(slot, slot_duration, stakes))
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
    pub genesis_hash: Hash,
}
impl Metadata for Meta {}

#[rpc(server)]
pub trait RpcSol {
    type Metadata;

    #[rpc(meta, name = "confirmTransaction")]
    fn confirm_transaction(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<bool>;

    #[rpc(meta, name = "getAccountInfo")]
    fn get_account_info(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> RpcResponse<Option<Account>>;

    #[rpc(meta, name = "getProgramAccounts")]
    fn get_program_accounts(
        &self,
        meta: Self::Metadata,
        program_id_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Vec<(String, Account)>>;

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
    ) -> Result<RpcEpochInfo>;

    #[rpc(meta, name = "getBlockCommitment")]
    fn get_block_commitment(
        &self,
        meta: Self::Metadata,
        block: Slot,
    ) -> Result<(Option<BlockCommitment>, u64)>;

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
    ) -> RpcResponse<(String, FeeCalculator)>;

    #[rpc(meta, name = "getSignatureStatus")]
    fn get_signature_status(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<transaction::Result<()>>>;

    #[rpc(meta, name = "getSlot")]
    fn get_slot(&self, meta: Self::Metadata, commitment: Option<CommitmentConfig>) -> Result<u64>;

    #[rpc(meta, name = "getTransactionCount")]
    fn get_transaction_count(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64>;

    #[rpc(meta, name = "getTotalSupply")]
    fn get_total_supply(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64>;

    #[rpc(meta, name = "requestAirdrop")]
    fn request_airdrop(
        &self,
        meta: Self::Metadata,
        pubkey_str: String,
        lamports: u64,
        commitment: Option<CommitmentConfig>,
    ) -> Result<String>;

    #[rpc(meta, name = "sendTransaction")]
    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String>;

    #[rpc(meta, name = "getSlotLeader")]
    fn get_slot_leader(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<String>;

    #[rpc(meta, name = "getVoteAccounts")]
    fn get_vote_accounts(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<RpcVoteAccountStatus>;

    #[rpc(meta, name = "getStorageTurnRate")]
    fn get_storage_turn_rate(&self, meta: Self::Metadata) -> Result<u64>;

    #[rpc(meta, name = "getStorageTurn")]
    fn get_storage_turn(&self, meta: Self::Metadata) -> Result<(String, u64)>;

    #[rpc(meta, name = "getSlotsPerSegment")]
    fn get_slots_per_segment(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64>;

    #[rpc(meta, name = "getStoragePubkeysForSlot")]
    fn get_storage_pubkeys_for_slot(&self, meta: Self::Metadata, slot: u64) -> Result<Vec<Pubkey>>;

    #[rpc(meta, name = "validatorExit")]
    fn validator_exit(&self, meta: Self::Metadata) -> Result<bool>;

    #[rpc(meta, name = "getNumBlocksSinceSignatureConfirmation")]
    fn get_num_blocks_since_signature_confirmation(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<usize>>;

    #[rpc(meta, name = "getSignatureConfirmation")]
    fn get_signature_confirmation(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<(usize, transaction::Result<()>)>>;

    #[rpc(meta, name = "getVersion")]
    fn get_version(&self, meta: Self::Metadata) -> Result<RpcVersionInfo>;

    #[rpc(meta, name = "setLogFilter")]
    fn set_log_filter(&self, _meta: Self::Metadata, filter: String) -> Result<()>;

    #[rpc(meta, name = "getConfirmedBlock")]
    fn get_confirmed_block(
        &self,
        meta: Self::Metadata,
        slot: Slot,
    ) -> Result<Option<RpcConfirmedBlock>>;

    #[rpc(meta, name = "getBlockTime")]
    fn get_block_time(&self, meta: Self::Metadata, slot: Slot) -> Result<Option<UnixTimestamp>>;

    #[rpc(meta, name = "getConfirmedBlocks")]
    fn get_confirmed_blocks(
        &self,
        meta: Self::Metadata,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> Result<Vec<Slot>>;
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
    ) -> RpcResponse<Option<Account>> {
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
    ) -> Result<Vec<(String, Account)>> {
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
        Ok(meta
            .request_processor
            .read()
            .unwrap()
            .get_inflation(commitment)
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

    fn get_epoch_info(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<RpcEpochInfo> {
        let bank = meta.request_processor.read().unwrap().bank(commitment);
        let epoch_schedule = bank.epoch_schedule();

        let slot = bank.slot();
        let (epoch, slot_index) = epoch_schedule.get_epoch_and_slot_index(slot);
        Ok(RpcEpochInfo {
            epoch,
            slot_index,
            slots_in_epoch: epoch_schedule.get_slots_in_epoch(epoch),
            absolute_slot: slot,
        })
    }

    fn get_block_commitment(
        &self,
        meta: Self::Metadata,
        block: Slot,
    ) -> Result<(Option<BlockCommitment>, u64)> {
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
        let bank = meta.request_processor.read().unwrap().bank(commitment);
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
    ) -> RpcResponse<(String, FeeCalculator)> {
        debug!("get_recent_blockhash rpc request received");
        meta.request_processor
            .read()
            .unwrap()
            .get_recent_blockhash(commitment)
    }

    fn get_signature_status(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<transaction::Result<()>>> {
        self.get_signature_confirmation(meta, signature_str, commitment)
            .map(|res| res.map(|x| x.1))
    }

    fn get_slot(&self, meta: Self::Metadata, commitment: Option<CommitmentConfig>) -> Result<u64> {
        meta.request_processor.read().unwrap().get_slot(commitment)
    }

    fn get_num_blocks_since_signature_confirmation(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<usize>> {
        self.get_signature_confirmation(meta, signature_str, commitment)
            .map(|res| res.map(|x| x.0))
    }

    fn get_signature_confirmation(
        &self,
        meta: Self::Metadata,
        signature_str: String,
        commitment: Option<CommitmentConfig>,
    ) -> Result<Option<(usize, transaction::Result<()>)>> {
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
            .bank(commitment.clone())
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
                .get_signature_confirmation_status(signature, commitment.clone())
                .map(|x| x.1);

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

    fn send_transaction(&self, meta: Self::Metadata, data: Vec<u8>) -> Result<String> {
        if data.len() >= PACKET_DATA_SIZE {
            info!(
                "send_transaction: transaction too large: {} bytes (max: {} bytes)",
                data.len(),
                PACKET_DATA_SIZE
            );
            return Err(Error::invalid_request());
        }
        let tx: Transaction = bincode::config()
            .limit(PACKET_DATA_SIZE as u64)
            .deserialize(&data)
            .map_err(|err| {
                info!("send_transaction: deserialize error: {:?}", err);
                Error::invalid_request()
            })?;

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

    fn get_storage_turn_rate(&self, meta: Self::Metadata) -> Result<u64> {
        meta.request_processor
            .read()
            .unwrap()
            .get_storage_turn_rate()
    }

    fn get_storage_turn(&self, meta: Self::Metadata) -> Result<(String, u64)> {
        meta.request_processor.read().unwrap().get_storage_turn()
    }

    fn get_slots_per_segment(
        &self,
        meta: Self::Metadata,
        commitment: Option<CommitmentConfig>,
    ) -> Result<u64> {
        meta.request_processor
            .read()
            .unwrap()
            .get_slots_per_segment(commitment)
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
            solana_core: solana_clap_utils::version!().to_string(),
        })
    }

    fn set_log_filter(&self, _meta: Self::Metadata, filter: String) -> Result<()> {
        solana_logger::setup_with_filter(&filter);
        Ok(())
    }

    fn get_confirmed_block(
        &self,
        meta: Self::Metadata,
        slot: Slot,
    ) -> Result<Option<RpcConfirmedBlock>> {
        meta.request_processor
            .read()
            .unwrap()
            .get_confirmed_block(slot)
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
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{
        contact_info::ContactInfo,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        replay_stage::tests::create_test_transactions_and_populate_blocktree,
    };
    use jsonrpc_core::{MetaIoHandler, Output, Response, Value};
    use solana_ledger::{
        blocktree::entries_to_test_shreds, blocktree_processor::fill_blocktree_slot_with_ticks,
        entry::next_entry_mut, get_tmp_ledger_path,
    };
    use solana_sdk::{
        fee_calculator::DEFAULT_BURN_PERCENT,
        hash::{hash, Hash},
        instruction::InstructionError,
        signature::{Keypair, KeypairUtil},
        system_transaction,
        transaction::TransactionError,
    };
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
        start_rpc_handler_with_tx_and_blocktree(pubkey, vec![], 0)
    }

    fn start_rpc_handler_with_tx_and_blocktree(
        pubkey: &Pubkey,
        blocktree_roots: Vec<Slot>,
        default_timestamp: i64,
    ) -> RpcHandler {
        let (bank_forks, alice, leader_vote_keypair) = new_bank_forks();
        let bank = bank_forks.read().unwrap().working_bank();

        let commitment_slot0 = BlockCommitment::new([8; MAX_LOCKOUT_HISTORY]);
        let commitment_slot1 = BlockCommitment::new([9; MAX_LOCKOUT_HISTORY]);
        let mut block_commitment: HashMap<u64, BlockCommitment> = HashMap::new();
        block_commitment
            .entry(0)
            .or_insert(commitment_slot0.clone());
        block_commitment
            .entry(1)
            .or_insert(commitment_slot1.clone());
        let block_commitment_cache =
            Arc::new(RwLock::new(BlockCommitmentCache::new(block_commitment, 42)));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let blocktree = Arc::new(blocktree);

        let keypair1 = Keypair::new();
        let keypair2 = Keypair::new();
        let keypair3 = Keypair::new();
        bank.transfer(4, &alice, &keypair2.pubkey()).unwrap();
        let confirmed_block_signatures = create_test_transactions_and_populate_blocktree(
            vec![&alice, &keypair1, &keypair2, &keypair3],
            0,
            bank.clone(),
            blocktree.clone(),
        );

        // Add timestamp vote to blocktree
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
            vec![vote_ix],
            Hash::default(),
        );
        let shreds = entries_to_test_shreds(
            vec![next_entry_mut(&mut Hash::default(), 0, vec![vote_tx])],
            1,
            0,
            true,
            0,
        );
        blocktree.insert_shreds(shreds, None, false).unwrap();
        blocktree.set_roots(&[1]).unwrap();

        let mut roots = blocktree_roots.clone();
        if !roots.is_empty() {
            roots.retain(|&x| x > 1);
            let mut parent_bank = bank;
            for (i, root) in roots.iter().enumerate() {
                let new_bank =
                    Bank::new_from_parent(&parent_bank, parent_bank.collector_id(), *root);
                parent_bank = bank_forks.write().unwrap().insert(new_bank);
                parent_bank.squash();
                bank_forks.write().unwrap().set_root(*root, &None);
                let parent = if i > 0 { roots[i - 1] } else { 1 };
                fill_blocktree_slot_with_ticks(&blocktree, 5, *root, parent, Hash::default());
            }
            blocktree.set_roots(&roots).unwrap();
            let new_bank = Bank::new_from_parent(
                &parent_bank,
                parent_bank.collector_id(),
                roots.iter().max().unwrap() + 1,
            );
            bank_forks.write().unwrap().insert(new_bank);
        }

        let bank = bank_forks.read().unwrap().working_bank();

        let leader_pubkey = *bank.collector_id();
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);

        let blockhash = bank.confirmed_last_blockhash().0;
        let tx = system_transaction::transfer(&alice, pubkey, 20, blockhash);
        bank.process_transaction(&tx).expect("process transaction");

        let tx = system_transaction::transfer(&alice, &alice.pubkey(), 20, blockhash);
        let _ = bank.process_transaction(&tx);

        let request_processor = Arc::new(RwLock::new(JsonRpcRequestProcessor::new(
            JsonRpcConfig::default(),
            bank_forks.clone(),
            block_commitment_cache.clone(),
            blocktree,
            StorageState::default(),
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
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let request_processor = JsonRpcRequestProcessor::new(
            JsonRpcConfig::default(),
            bank_forks,
            block_commitment_cache,
            Arc::new(blocktree),
            StorageState::default(),
            &validator_exit,
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
        let expected = format!(r#"{{"jsonrpc":"2.0","result":3,"id":1}}"#);
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
                "owner": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
                "lamports": 20,
                "data": [],
                "executable": false,
                "rent_epoch": 0
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
            "result": {
            "context":{"slot":0},
            "value":[ blockhash.to_string(), {
                "burnPercent": DEFAULT_BURN_PERCENT,
                "lamportsPerSignature": 0,
                "maxLamportsPerSignature": 0,
                "minLamportsPerSignature": 0,
                "targetLamportsPerSignature": 0,
                "targetSignaturesPerSlot": 0
            }]},
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
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();

        let mut io = MetaIoHandler::default();
        let rpc = RpcSolImpl;
        io.extend_with(rpc.to_delegate());
        let meta = Meta {
            request_processor: {
                let request_processor = JsonRpcRequestProcessor::new(
                    JsonRpcConfig::default(),
                    new_bank_forks().0,
                    block_commitment_cache,
                    Arc::new(blocktree),
                    StorageState::default(),
                    &validator_exit,
                );
                Arc::new(RwLock::new(request_processor))
            },
            cluster_info: Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
                ContactInfo::default(),
            ))),
            genesis_hash: Hash::default(),
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
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let request_processor = JsonRpcRequestProcessor::new(
            JsonRpcConfig::default(),
            new_bank_forks().0,
            block_commitment_cache,
            Arc::new(blocktree),
            StorageState::default(),
            &validator_exit,
        );
        assert_eq!(request_processor.validator_exit(), Ok(false));
        assert_eq!(exit.load(Ordering::Relaxed), false);
    }

    #[test]
    fn test_rpc_request_processor_allow_validator_exit_config() {
        let exit = Arc::new(AtomicBool::new(false));
        let validator_exit = create_validator_exit(&exit);
        let block_commitment_cache = Arc::new(RwLock::new(BlockCommitmentCache::default()));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();
        let mut config = JsonRpcConfig::default();
        config.enable_validator_exit = true;
        let request_processor = JsonRpcRequestProcessor::new(
            config,
            new_bank_forks().0,
            block_commitment_cache,
            Arc::new(blocktree),
            StorageState::default(),
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
                "solana-core": solana_clap_utils::version!().to_string()
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
        let commitment_slot0 = BlockCommitment::new([8; MAX_LOCKOUT_HISTORY]);
        let commitment_slot1 = BlockCommitment::new([9; MAX_LOCKOUT_HISTORY]);
        let mut block_commitment: HashMap<u64, BlockCommitment> = HashMap::new();
        block_commitment
            .entry(0)
            .or_insert(commitment_slot0.clone());
        block_commitment
            .entry(1)
            .or_insert(commitment_slot1.clone());
        let block_commitment_cache =
            Arc::new(RwLock::new(BlockCommitmentCache::new(block_commitment, 42)));
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Blocktree::open(&ledger_path).unwrap();

        let mut config = JsonRpcConfig::default();
        config.enable_validator_exit = true;
        let request_processor = JsonRpcRequestProcessor::new(
            config,
            new_bank_forks().0,
            block_commitment_cache,
            Arc::new(blocktree),
            StorageState::default(),
            &validator_exit,
        );
        assert_eq!(
            request_processor.get_block_commitment(0),
            (Some(commitment_slot0), 42)
        );
        assert_eq!(
            request_processor.get_block_commitment(1),
            (Some(commitment_slot1), 42)
        );
        assert_eq!(request_processor.get_block_commitment(2), (None, 42));
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

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockCommitment","params":[0]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let (commitment, total_staked): (Option<BlockCommitment>, u64) =
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
            commitment,
            block_commitment_cache
                .read()
                .unwrap()
                .get_block_commitment(0)
                .cloned()
        );
        assert_eq!(total_staked, 42);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getBlockCommitment","params":[2]}}"#);
        let res = io.handle_request_sync(&req, meta);
        let result: Response = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let (commitment, total_staked): (Option<BlockCommitment>, u64) =
            if let Response::Single(res) = result {
                if let Output::Success(res) = res {
                    serde_json::from_value(res.result).unwrap()
                } else {
                    panic!("Expected success");
                }
            } else {
                panic!("Expected single response");
            };
        assert_eq!(commitment, None);
        assert_eq!(total_staked, 42);
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

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlock","params":[0]}}"#);
        let res = io.handle_request_sync(&req, meta);
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_block: Option<RpcConfirmedBlock> =
            serde_json::from_value(result["result"].clone()).unwrap();
        let confirmed_block = confirmed_block.unwrap();
        assert_eq!(confirmed_block.transactions.len(), 3);

        for (transaction, result) in confirmed_block.transactions.into_iter() {
            if transaction.signatures[0] == confirmed_block_signatures[0] {
                assert_eq!(transaction.message.recent_blockhash, blockhash);
                assert_eq!(result.unwrap().status, Ok(()));
            } else if transaction.signatures[0] == confirmed_block_signatures[1] {
                assert_eq!(
                    result.unwrap().status,
                    Err(TransactionError::InstructionError(
                        0,
                        InstructionError::CustomError(1)
                    ))
                );
            } else {
                assert_eq!(result, None);
            }
        }
    }

    #[test]
    fn test_get_confirmed_blocks() {
        let bob_pubkey = Pubkey::new_rand();
        let roots = vec![0, 1, 3, 4, 8];
        let RpcHandler { io, meta, .. } =
            start_rpc_handler_with_tx_and_blocktree(&bob_pubkey, roots.clone(), 0);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[0]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, roots);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[2]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, vec![3, 4, 8]);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[0, 4]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, vec![0, 1, 3, 4]);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[0, 7]}}"#);
        let res = io.handle_request_sync(&req, meta.clone());
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, vec![0, 1, 3, 4]);

        let req =
            format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getConfirmedBlocks","params":[9, 11]}}"#);
        let res = io.handle_request_sync(&req, meta);
        let result: Value = serde_json::from_str(&res.expect("actual response"))
            .expect("actual response deserialization");
        let confirmed_blocks: Vec<Slot> = serde_json::from_value(result["result"].clone()).unwrap();
        assert_eq!(confirmed_blocks, Vec::<Slot>::new());
    }

    #[test]
    fn test_get_block_time() {
        let bob_pubkey = Pubkey::new_rand();
        let base_timestamp = 1576183541;
        let RpcHandler { io, meta, bank, .. } = start_rpc_handler_with_tx_and_blocktree(
            &bob_pubkey,
            vec![1, 2, 3, 4, 5, 6, 7],
            base_timestamp,
        );

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
        let expected = format!(r#"{{"jsonrpc":"2.0","result":null,"id":1}}"#);
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
            instructions,
            bank.last_blockhash(),
        );
        bank.process_transaction(&transaction)
            .expect("process transaction");
        assert_eq!(bank.vote_accounts().len(), 2);

        // Check getVoteAccounts: the bootstrap leader vote account will be delinquent as it has
        // stake but has never voted, and the vote account with no stake should not be present.
        {
            let req = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"getVoteAccounts"}}"#);
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
            let instructions = vec![
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
                instructions,
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
        assert_eq!(leader_info.epoch_credits, vec![(0, expected_credits, 0)]);

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

            let res = io.handle_request_sync(&req, meta.clone());
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
