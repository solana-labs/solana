use {
    crate::{
        accountsdb_plugin_postgres::{
            AccountsDbPluginPostgresConfig, AccountsDbPluginPostgresError,
        },
        postgres_client::{DbWorkItem, ParallelPostgresClient, SimplePostgresClient},
    },
    postgres::{Client, Statement},
    postgres_types::ToSql,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        AccountsDbPluginError, ReplicaTransactionInfo,
    },
    solana_runtime::bank::RewardType,
    solana_sdk::{
        instruction::CompiledInstruction,
        message::{
            v0::{self, AddressMapIndexes},
            MappedAddresses, MappedMessage, Message, MessageHeader, SanitizedMessage,
        },
        transaction::TransactionError,
    },
    solana_transaction_status::{
        InnerInstructions, Reward, TransactionStatusMeta, TransactionTokenBalance,
    },
};

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "CompiledInstruction")]
pub struct DbCompiledInstruction {
    pub program_id_index: i16,
    pub accounts: Vec<i16>,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "InnerInstructions")]
pub struct DbInnerInstructions {
    pub index: i16,
    pub instructions: Vec<DbCompiledInstruction>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionTokenBalance")]
pub struct DbTransactionTokenBalance {
    pub account_index: i16,
    pub mint: String,
    pub ui_token_amount: Option<f64>,
    pub owner: String,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "Reward")]
pub struct DbReward {
    pub pubkey: String,
    pub lamports: i64,
    pub post_balance: i64,
    pub reward_type: Option<String>,
    pub commission: Option<i16>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionStatusMeta")]
pub struct DbTransactionStatusMeta {
    pub status: Option<String>,
    pub fee: i64,
    pub pre_balances: Vec<i64>,
    pub post_balances: Vec<i64>,
    pub inner_instructions: Option<Vec<DbInnerInstructions>>,
    pub log_messages: Option<Vec<String>>,
    pub pre_token_balances: Option<Vec<DbTransactionTokenBalance>>,
    pub post_token_balances: Option<Vec<DbTransactionTokenBalance>>,
    pub rewards: Option<Vec<DbReward>>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionMessageHeader")]
pub struct DbTransactionMessageHeader {
    pub num_required_signatures: i16,
    pub num_readonly_signed_accounts: i16,
    pub num_readonly_unsigned_accounts: i16,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionMessage")]
pub struct DbTransactionMessage {
    pub header: DbTransactionMessageHeader,
    pub account_keys: Vec<Vec<u8>>,
    pub recent_blockhash: Vec<u8>,
    pub instructions: Vec<DbCompiledInstruction>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "AddressMapIndexes")]
pub struct DbAddressMapIndexes {
    pub writable: Vec<i16>,
    pub readonly: Vec<i16>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionMessageV0")]
pub struct DbTransactionMessageV0 {
    pub header: DbTransactionMessageHeader,
    pub account_keys: Vec<Vec<u8>>,
    pub recent_blockhash: Vec<u8>,
    pub instructions: Vec<DbCompiledInstruction>,
    pub address_map_indexes: Vec<DbAddressMapIndexes>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "MappedAddresses")]
pub struct DbMappedAddresses {
    pub writable: Vec<Vec<u8>>,
    pub readonly: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "MappedMessage")]
pub struct DbMappedMessage {
    pub message: DbTransactionMessageV0,
    pub mapped_addresses: DbMappedAddresses,
}

pub struct DbTransaction {
    pub signature: Vec<u8>,
    pub is_vote: bool,
    pub slot: i64,
    pub message_type: i16,
    pub legacy_message: Option<DbTransactionMessage>,
    pub v0_mapped_message: Option<DbMappedMessage>,
    pub message_hash: Vec<u8>,
    pub meta: DbTransactionStatusMeta,
    pub signatures: Vec<Vec<u8>>,
}

pub struct LogTransactionRequest {
    pub transaction_info: DbTransaction,
}

impl From<&AddressMapIndexes> for DbAddressMapIndexes {
    fn from(address_map_indexes: &AddressMapIndexes) -> Self {
        Self {
            writable: address_map_indexes
                .writable
                .iter()
                .map(|address_idx| *address_idx as i16)
                .collect(),
            readonly: address_map_indexes
                .readonly
                .iter()
                .map(|address_idx| *address_idx as i16)
                .collect(),
        }
    }
}

impl From<&MappedAddresses> for DbMappedAddresses {
    fn from(mapped_addresses: &MappedAddresses) -> Self {
        Self {
            writable: mapped_addresses
                .writable
                .iter()
                .map(|pubkey| pubkey.as_ref().to_vec())
                .collect(),
            readonly: mapped_addresses
                .readonly
                .iter()
                .map(|pubkey| pubkey.as_ref().to_vec())
                .collect(),
        }
    }
}

impl From<&MessageHeader> for DbTransactionMessageHeader {
    fn from(header: &MessageHeader) -> Self {
        Self {
            num_required_signatures: header.num_required_signatures as i16,
            num_readonly_signed_accounts: header.num_readonly_signed_accounts as i16,
            num_readonly_unsigned_accounts: header.num_readonly_unsigned_accounts as i16,
        }
    }
}

impl From<&CompiledInstruction> for DbCompiledInstruction {
    fn from(instrtuction: &CompiledInstruction) -> Self {
        Self {
            program_id_index: instrtuction.program_id_index as i16,
            accounts: instrtuction
                .accounts
                .iter()
                .map(|account_idx| *account_idx as i16)
                .collect(),
            data: instrtuction.data.clone(),
        }
    }
}

impl From<&Message> for DbTransactionMessage {
    fn from(message: &Message) -> Self {
        Self {
            header: DbTransactionMessageHeader::from(&message.header),
            account_keys: message
                .account_keys
                .iter()
                .map(|key| key.as_ref().to_vec())
                .collect(),
            recent_blockhash: message.recent_blockhash.as_ref().to_vec(),
            instructions: message
                .instructions
                .iter()
                .map(DbCompiledInstruction::from)
                .collect(),
        }
    }
}

impl From<&v0::Message> for DbTransactionMessageV0 {
    fn from(message: &v0::Message) -> Self {
        Self {
            header: DbTransactionMessageHeader::from(&message.header),
            account_keys: message
                .account_keys
                .iter()
                .map(|key| key.as_ref().to_vec())
                .collect(),
            recent_blockhash: message.recent_blockhash.as_ref().to_vec(),
            instructions: message
                .instructions
                .iter()
                .map(DbCompiledInstruction::from)
                .collect(),
            address_map_indexes: message
                .address_map_indexes
                .iter()
                .map(DbAddressMapIndexes::from)
                .collect(),
        }
    }
}

impl From<&MappedMessage> for DbMappedMessage {
    fn from(message: &MappedMessage) -> Self {
        Self {
            message: DbTransactionMessageV0::from(&message.message),
            mapped_addresses: DbMappedAddresses::from(&message.mapped_addresses),
        }
    }
}

impl From<&InnerInstructions> for DbInnerInstructions {
    fn from(instructions: &InnerInstructions) -> Self {
        Self {
            index: instructions.index as i16,
            instructions: instructions
                .instructions
                .iter()
                .map(DbCompiledInstruction::from)
                .collect(),
        }
    }
}

fn get_reward_type(reward: &Option<RewardType>) -> Option<String> {
    reward.as_ref().map(|reward_type| match reward_type {
        RewardType::Fee => "fee".to_string(),
        RewardType::Rent => "rent".to_string(),
        RewardType::Staking => "staking".to_string(),
        RewardType::Voting => "voting".to_string(),
    })
}

impl From<&Reward> for DbReward {
    fn from(reward: &Reward) -> Self {
        Self {
            pubkey: reward.pubkey.clone(),
            lamports: reward.lamports as i64,
            post_balance: reward.post_balance as i64,
            reward_type: get_reward_type(&reward.reward_type),
            commission: reward
                .commission
                .as_ref()
                .map(|commission| *commission as i16),
        }
    }
}

fn get_transaction_status(result: &Result<(), TransactionError>) -> Option<String> {
    if result.is_ok() {
        return None;
    }

    let err = match result.as_ref().err().unwrap() {
        TransactionError::AccountInUse => "AccountInUse",
        TransactionError::AccountLoadedTwice => "AccountLoadedTwice",
        TransactionError::AccountNotFound => "AccountNotFound",
        TransactionError::ProgramAccountNotFound => "ProgramAccountNotFound",
        TransactionError::InsufficientFundsForFee => "InsufficientFundsForFee",
        TransactionError::InvalidAccountForFee => "InvalidAccountForFee",
        TransactionError::AlreadyProcessed => "AlreadyProcessed",
        TransactionError::BlockhashNotFound => "BlockhashNotFound",
        TransactionError::InstructionError(idx, error) => {
            return Some(format!(
                "InstructionError: idx ({}), error: ({})",
                idx, error
            ));
        }
        TransactionError::CallChainTooDeep => "CallChainTooDeep",
        TransactionError::MissingSignatureForFee => "MissingSignatureForFee",
        TransactionError::InvalidAccountIndex => "InvalidAccountIndex",
        TransactionError::SignatureFailure => "SignatureFailure",
        TransactionError::InvalidProgramForExecution => "InvalidProgramForExecution",
        TransactionError::SanitizeFailure => "SanitizeFailure",
        TransactionError::ClusterMaintenance => "ClusterMaintenance",
        TransactionError::AccountBorrowOutstanding => "AccountBorrowOutstanding",
        TransactionError::WouldExceedMaxBlockCostLimit => "WouldExceedMaxBlockCostLimit",
        TransactionError::UnsupportedVersion => "UnsupportedVersion",
        TransactionError::InvalidWritableAccount => "InvalidWritableAccount",
    };

    Some(err.to_string())
}

impl From<&TransactionTokenBalance> for DbTransactionTokenBalance {
    fn from(token_balance: &TransactionTokenBalance) -> Self {
        Self {
            account_index: token_balance.account_index as i16,
            mint: token_balance.mint.clone(),
            ui_token_amount: token_balance.ui_token_amount.ui_amount,
            owner: token_balance.owner.clone(),
        }
    }
}

impl From<&TransactionStatusMeta> for DbTransactionStatusMeta {
    fn from(meta: &TransactionStatusMeta) -> Self {
        Self {
            status: get_transaction_status(&meta.status),
            fee: meta.fee as i64,
            pre_balances: meta
                .pre_balances
                .iter()
                .map(|balance| *balance as i64)
                .collect(),
            post_balances: meta
                .post_balances
                .iter()
                .map(|balance| *balance as i64)
                .collect(),
            inner_instructions: meta
                .inner_instructions
                .as_ref()
                .map(|instructions| instructions.iter().map(DbInnerInstructions::from).collect()),
            log_messages: meta.log_messages.clone(),
            pre_token_balances: meta.pre_token_balances.as_ref().map(|balances| {
                balances
                    .iter()
                    .map(DbTransactionTokenBalance::from)
                    .collect()
            }),
            post_token_balances: meta.post_token_balances.as_ref().map(|balances| {
                balances
                    .iter()
                    .map(DbTransactionTokenBalance::from)
                    .collect()
            }),
            rewards: meta
                .rewards
                .as_ref()
                .map(|rewards| rewards.iter().map(DbReward::from).collect()),
        }
    }
}

impl SimplePostgresClient {
    pub fn build_transaction_info_upsert_statement(
        client: &mut Client,
        config: &AccountsDbPluginPostgresConfig,
    ) -> Result<Statement, AccountsDbPluginError> {
        let stmt = "INSERT INTO transaction AS txn (signature, is_vote, slot, message_type, legacy_message, \
        v0_mapped_message, signatures, message_hash, meta, updated_on) \
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)";

        let stmt = client.prepare(stmt);

        match stmt {
            Err(err) => {
                return Err(AccountsDbPluginError::Custom(Box::new(AccountsDbPluginPostgresError::DataSchemaError {
                    msg: format!(
                        "Error in preparing for the transaction update PostgreSQL database: ({}) host: {:?} user: {:?} config: {:?}",
                        err, config.host, config.user, config
                    ),
                })));
            }
            Ok(stmt) => Ok(stmt),
        }
    }
}

impl ParallelPostgresClient {
    fn build_db_transaction(slot: u64, transaction_info: &ReplicaTransactionInfo) -> DbTransaction {
        DbTransaction {
            signature: transaction_info.signature.as_ref().to_vec(),
            is_vote: transaction_info.is_vote,
            slot: slot as i64,
            message_type: match transaction_info.transaction.message() {
                SanitizedMessage::Legacy(_) => 0,
                SanitizedMessage::V0(_) => 1,
            },
            legacy_message: match transaction_info.transaction.message() {
                SanitizedMessage::Legacy(legacy_message) => {
                    Some(DbTransactionMessage::from(legacy_message))
                }
                _ => None,
            },
            v0_mapped_message: match transaction_info.transaction.message() {
                SanitizedMessage::V0(mapped_message) => Some(DbMappedMessage::from(mapped_message)),
                _ => None,
            },
            signatures: transaction_info
                .transaction
                .signatures()
                .iter()
                .map(|signature| signature.as_ref().to_vec())
                .collect(),
            message_hash: transaction_info
                .transaction
                .message_hash()
                .as_ref()
                .to_vec(),
            meta: DbTransactionStatusMeta::from(transaction_info.transaction_status_meta),
        }
    }

    fn build_transaction_request(
        slot: u64,
        transaction_info: &ReplicaTransactionInfo,
    ) -> LogTransactionRequest {
        LogTransactionRequest {
            transaction_info: Self::build_db_transaction(slot, transaction_info),
        }
    }

    pub fn log_transaction_info(
        &mut self,
        transaction_info: &ReplicaTransactionInfo,
        slot: u64,
    ) -> Result<(), AccountsDbPluginError> {
        let wrk_item = DbWorkItem::LogTransaction(Box::new(Self::build_transaction_request(
            slot,
            transaction_info,
        )));

        if let Err(err) = self.sender.send(wrk_item) {
            return Err(AccountsDbPluginError::SlotStatusUpdateError {
                msg: format!("Failed to update the transaction, error: {:?}", err),
            });
        }
        Ok(())
    }
}
