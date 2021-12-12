/// Module responsible for handling persisting transaction data to the PostgreSQL
/// database.
use {
    crate::{
        accountsdb_plugin_postgres::{
            AccountsDbPluginPostgresConfig, AccountsDbPluginPostgresError,
        },
        postgres_client::{DbWorkItem, ParallelPostgresClient, SimplePostgresClient},
    },
    chrono::Utc,
    log::*,
    postgres::{Client, Statement},
    postgres_types::ToSql,
    solana_accountsdb_plugin_interface::accountsdb_plugin_interface::{
        AccountsDbPluginError, ReplicaTransactionInfo,
    },
    solana_runtime::bank::RewardType,
    solana_sdk::{
        instruction::CompiledInstruction,
        message::{
            v0::{self, LoadedAddresses, MessageAddressTableLookup},
            Message, MessageHeader, SanitizedMessage,
        },
        transaction::TransactionError,
    },
    solana_transaction_status::{
        InnerInstructions, Reward, TransactionStatusMeta, TransactionTokenBalance,
    },
};

const MAX_TRANSACTION_STATUS_LEN: usize = 256;

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

#[derive(Clone, Debug, ToSql, PartialEq)]
#[postgres(name = "RewardType")]
pub enum DbRewardType {
    Fee,
    Rent,
    Staking,
    Voting,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "Reward")]
pub struct DbReward {
    pub pubkey: String,
    pub lamports: i64,
    pub post_balance: i64,
    pub reward_type: Option<DbRewardType>,
    pub commission: Option<i16>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionStatusMeta")]
pub struct DbTransactionStatusMeta {
    pub error: Option<DbTransactionError>,
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
#[postgres(name = "TransactionMessageAddressTableLookup")]
pub struct DbTransactionMessageAddressTableLookup {
    pub account_key: Vec<u8>,
    pub writable_indexes: Vec<i16>,
    pub readonly_indexes: Vec<i16>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "TransactionMessageV0")]
pub struct DbTransactionMessageV0 {
    pub header: DbTransactionMessageHeader,
    pub account_keys: Vec<Vec<u8>>,
    pub recent_blockhash: Vec<u8>,
    pub instructions: Vec<DbCompiledInstruction>,
    pub address_table_lookups: Vec<DbTransactionMessageAddressTableLookup>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "LoadedAddresses")]
pub struct DbLoadedAddresses {
    pub writable: Vec<Vec<u8>>,
    pub readonly: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, ToSql)]
#[postgres(name = "LoadedMessageV0")]
pub struct DbLoadedMessageV0 {
    pub message: DbTransactionMessageV0,
    pub loaded_addresses: DbLoadedAddresses,
}

pub struct DbTransaction {
    pub signature: Vec<u8>,
    pub is_vote: bool,
    pub slot: i64,
    pub message_type: i16,
    pub legacy_message: Option<DbTransactionMessage>,
    pub v0_loaded_message: Option<DbLoadedMessageV0>,
    pub message_hash: Vec<u8>,
    pub meta: DbTransactionStatusMeta,
    pub signatures: Vec<Vec<u8>>,
}

pub struct LogTransactionRequest {
    pub transaction_info: DbTransaction,
}

impl From<&MessageAddressTableLookup> for DbTransactionMessageAddressTableLookup {
    fn from(address_table_lookup: &MessageAddressTableLookup) -> Self {
        Self {
            account_key: address_table_lookup.account_key.as_ref().to_vec(),
            writable_indexes: address_table_lookup
                .writable_indexes
                .iter()
                .map(|idx| *idx as i16)
                .collect(),
            readonly_indexes: address_table_lookup
                .readonly_indexes
                .iter()
                .map(|idx| *idx as i16)
                .collect(),
        }
    }
}

impl From<&LoadedAddresses> for DbLoadedAddresses {
    fn from(loaded_addresses: &LoadedAddresses) -> Self {
        Self {
            writable: loaded_addresses
                .writable
                .iter()
                .map(|pubkey| pubkey.as_ref().to_vec())
                .collect(),
            readonly: loaded_addresses
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
    fn from(instruction: &CompiledInstruction) -> Self {
        Self {
            program_id_index: instruction.program_id_index as i16,
            accounts: instruction
                .accounts
                .iter()
                .map(|account_idx| *account_idx as i16)
                .collect(),
            data: instruction.data.clone(),
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
            address_table_lookups: message
                .address_table_lookups
                .iter()
                .map(DbTransactionMessageAddressTableLookup::from)
                .collect(),
        }
    }
}

impl From<&v0::LoadedMessage> for DbLoadedMessageV0 {
    fn from(message: &v0::LoadedMessage) -> Self {
        Self {
            message: DbTransactionMessageV0::from(&message.message),
            loaded_addresses: DbLoadedAddresses::from(&message.loaded_addresses),
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

impl From<&RewardType> for DbRewardType {
    fn from(reward_type: &RewardType) -> Self {
        match reward_type {
            RewardType::Fee => Self::Fee,
            RewardType::Rent => Self::Rent,
            RewardType::Staking => Self::Staking,
            RewardType::Voting => Self::Voting,
        }
    }
}

fn get_reward_type(reward: &Option<RewardType>) -> Option<DbRewardType> {
    reward.as_ref().map(DbRewardType::from)
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

#[derive(Clone, Debug, ToSql, PartialEq)]
#[postgres(name = "TransactionErrorCode")]
pub enum DbTransactionErrorCode {
    AccountInUse,
    AccountLoadedTwice,
    AccountNotFound,
    ProgramAccountNotFound,
    InsufficientFundsForFee,
    InvalidAccountForFee,
    AlreadyProcessed,
    BlockhashNotFound,
    InstructionError,
    CallChainTooDeep,
    MissingSignatureForFee,
    InvalidAccountIndex,
    SignatureFailure,
    InvalidProgramForExecution,
    SanitizeFailure,
    ClusterMaintenance,
    AccountBorrowOutstanding,
    WouldExceedMaxAccountCostLimit,
    WouldExceedMaxBlockCostLimit,
    UnsupportedVersion,
    InvalidWritableAccount,
    WouldExceedMaxAccountDataCostLimit,
}

impl From<&TransactionError> for DbTransactionErrorCode {
    fn from(err: &TransactionError) -> Self {
        match err {
            TransactionError::AccountInUse => Self::AccountInUse,
            TransactionError::AccountLoadedTwice => Self::AccountLoadedTwice,
            TransactionError::AccountNotFound => Self::AccountNotFound,
            TransactionError::ProgramAccountNotFound => Self::ProgramAccountNotFound,
            TransactionError::InsufficientFundsForFee => Self::InsufficientFundsForFee,
            TransactionError::InvalidAccountForFee => Self::InvalidAccountForFee,
            TransactionError::AlreadyProcessed => Self::AlreadyProcessed,
            TransactionError::BlockhashNotFound => Self::BlockhashNotFound,
            TransactionError::InstructionError(_idx, _error) => Self::InstructionError,
            TransactionError::CallChainTooDeep => Self::CallChainTooDeep,
            TransactionError::MissingSignatureForFee => Self::MissingSignatureForFee,
            TransactionError::InvalidAccountIndex => Self::InvalidAccountIndex,
            TransactionError::SignatureFailure => Self::SignatureFailure,
            TransactionError::InvalidProgramForExecution => Self::InvalidProgramForExecution,
            TransactionError::SanitizeFailure => Self::SanitizeFailure,
            TransactionError::ClusterMaintenance => Self::ClusterMaintenance,
            TransactionError::AccountBorrowOutstanding => Self::AccountBorrowOutstanding,
            TransactionError::WouldExceedMaxAccountCostLimit => {
                Self::WouldExceedMaxAccountCostLimit
            }
            TransactionError::WouldExceedMaxBlockCostLimit => Self::WouldExceedMaxBlockCostLimit,
            TransactionError::UnsupportedVersion => Self::UnsupportedVersion,
            TransactionError::InvalidWritableAccount => Self::InvalidWritableAccount,
            TransactionError::WouldExceedMaxAccountDataCostLimit => {
                Self::WouldExceedMaxAccountDataCostLimit
            }
        }
    }
}

#[derive(Clone, Debug, ToSql, PartialEq)]
#[postgres(name = "TransactionError")]
pub struct DbTransactionError {
    error_code: DbTransactionErrorCode,
    error_detail: Option<String>,
}

fn get_transaction_error(result: &Result<(), TransactionError>) -> Option<DbTransactionError> {
    if result.is_ok() {
        return None;
    }

    let error = result.as_ref().err().unwrap();
    Some(DbTransactionError {
        error_code: DbTransactionErrorCode::from(error),
        error_detail: {
            if let TransactionError::InstructionError(idx, instruction_error) = error {
                let mut error_detail = format!(
                    "InstructionError: idx ({}), error: ({})",
                    idx, instruction_error
                );
                if error_detail.len() > MAX_TRANSACTION_STATUS_LEN {
                    error_detail = error_detail
                        .to_string()
                        .split_off(MAX_TRANSACTION_STATUS_LEN);
                }
                Some(error_detail)
            } else {
                None
            }
        },
    })
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
            error: get_transaction_error(&meta.status),
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
        v0_loaded_message: match transaction_info.transaction.message() {
            SanitizedMessage::V0(loaded_message) => Some(DbLoadedMessageV0::from(loaded_message)),
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

impl SimplePostgresClient {
    pub(crate) fn build_transaction_info_upsert_statement(
        client: &mut Client,
        config: &AccountsDbPluginPostgresConfig,
    ) -> Result<Statement, AccountsDbPluginError> {
        let stmt = "INSERT INTO transaction AS txn (signature, is_vote, slot, message_type, legacy_message, \
        v0_loaded_message, signatures, message_hash, meta, updated_on) \
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

    pub(crate) fn log_transaction_impl(
        &mut self,
        transaction_log_info: LogTransactionRequest,
    ) -> Result<(), AccountsDbPluginError> {
        let client = self.client.get_mut().unwrap();
        let statement = &client.update_transaction_log_stmt;
        let client = &mut client.client;
        let updated_on = Utc::now().naive_utc();

        let transaction_info = transaction_log_info.transaction_info;
        let result = client.query(
            statement,
            &[
                &transaction_info.signature,
                &transaction_info.is_vote,
                &transaction_info.slot,
                &transaction_info.message_type,
                &transaction_info.legacy_message,
                &transaction_info.v0_loaded_message,
                &transaction_info.signatures,
                &transaction_info.message_hash,
                &transaction_info.meta,
                &updated_on,
            ],
        );

        if let Err(err) = result {
            let msg = format!(
                "Failed to persist the update of transaction info to the PostgreSQL database. Error: {:?}",
                err
            );
            error!("{}", msg);
            return Err(AccountsDbPluginError::AccountsUpdateError { msg });
        }

        Ok(())
    }
}

impl ParallelPostgresClient {
    fn build_transaction_request(
        slot: u64,
        transaction_info: &ReplicaTransactionInfo,
    ) -> LogTransactionRequest {
        LogTransactionRequest {
            transaction_info: build_db_transaction(slot, transaction_info),
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

#[cfg(test)]
pub(crate) mod tests {
    use {
        super::*,
        solana_account_decoder::parse_token::UiTokenAmount,
        solana_sdk::{
            hash::Hash,
            message::VersionedMessage,
            pubkey::Pubkey,
            sanitize::Sanitize,
            signature::{Keypair, Signature, Signer},
            system_transaction,
            transaction::{SanitizedTransaction, Transaction, VersionedTransaction},
        },
    };

    fn check_compiled_instruction_equality(
        compiled_instruction: &CompiledInstruction,
        db_compiled_instruction: &DbCompiledInstruction,
    ) {
        assert_eq!(
            compiled_instruction.program_id_index,
            db_compiled_instruction.program_id_index as u8
        );
        assert_eq!(
            compiled_instruction.accounts.len(),
            db_compiled_instruction.accounts.len()
        );
        assert_eq!(
            compiled_instruction.data.len(),
            db_compiled_instruction.data.len()
        );

        for i in 0..compiled_instruction.accounts.len() {
            assert_eq!(
                compiled_instruction.accounts[i],
                db_compiled_instruction.accounts[i] as u8
            )
        }
        for i in 0..compiled_instruction.data.len() {
            assert_eq!(
                compiled_instruction.data[i],
                db_compiled_instruction.data[i] as u8
            )
        }
    }

    #[test]
    fn test_transform_compiled_instruction() {
        let compiled_instruction = CompiledInstruction {
            program_id_index: 0,
            accounts: vec![1, 2, 3],
            data: vec![4, 5, 6],
        };

        let db_compiled_instruction = DbCompiledInstruction::from(&compiled_instruction);
        check_compiled_instruction_equality(&compiled_instruction, &db_compiled_instruction);
    }

    fn check_inner_instructions_equality(
        inner_instructions: &InnerInstructions,
        db_inner_instructions: &DbInnerInstructions,
    ) {
        assert_eq!(inner_instructions.index, db_inner_instructions.index as u8);
        assert_eq!(
            inner_instructions.instructions.len(),
            db_inner_instructions.instructions.len()
        );

        for i in 0..inner_instructions.instructions.len() {
            check_compiled_instruction_equality(
                &inner_instructions.instructions[i],
                &db_inner_instructions.instructions[i],
            )
        }
    }

    #[test]
    fn test_transform_inner_instructions() {
        let inner_instructions = InnerInstructions {
            index: 0,
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 0,
                    accounts: vec![1, 2, 3],
                    data: vec![4, 5, 6],
                },
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![12, 13, 14],
                    data: vec![24, 25, 26],
                },
            ],
        };

        let db_inner_instructions = DbInnerInstructions::from(&inner_instructions);
        check_inner_instructions_equality(&inner_instructions, &db_inner_instructions);
    }

    fn check_address_table_lookups_equality(
        address_table_lookups: &MessageAddressTableLookup,
        db_address_table_lookups: &DbTransactionMessageAddressTableLookup,
    ) {
        assert_eq!(
            address_table_lookups.writable_indexes.len(),
            db_address_table_lookups.writable_indexes.len()
        );
        assert_eq!(
            address_table_lookups.readonly_indexes.len(),
            db_address_table_lookups.readonly_indexes.len()
        );

        for i in 0..address_table_lookups.writable_indexes.len() {
            assert_eq!(
                address_table_lookups.writable_indexes[i],
                db_address_table_lookups.writable_indexes[i] as u8
            )
        }
        for i in 0..address_table_lookups.readonly_indexes.len() {
            assert_eq!(
                address_table_lookups.readonly_indexes[i],
                db_address_table_lookups.readonly_indexes[i] as u8
            )
        }
    }

    #[test]
    fn test_transform_address_table_lookups() {
        let address_table_lookups = MessageAddressTableLookup {
            account_key: Pubkey::new_unique(),
            writable_indexes: vec![1, 2, 3],
            readonly_indexes: vec![4, 5, 6],
        };

        let db_address_table_lookups =
            DbTransactionMessageAddressTableLookup::from(&address_table_lookups);
        check_address_table_lookups_equality(&address_table_lookups, &db_address_table_lookups);
    }

    fn check_reward_equality(reward: &Reward, db_reward: &DbReward) {
        assert_eq!(reward.pubkey, db_reward.pubkey);
        assert_eq!(reward.lamports, db_reward.lamports);
        assert_eq!(reward.post_balance, db_reward.post_balance as u64);
        assert_eq!(get_reward_type(&reward.reward_type), db_reward.reward_type);
        assert_eq!(
            reward.commission,
            db_reward
                .commission
                .as_ref()
                .map(|commission| *commission as u8)
        );
    }

    #[test]
    fn test_transform_reward() {
        let reward = Reward {
            pubkey: Pubkey::new_unique().to_string(),
            lamports: 1234,
            post_balance: 45678,
            reward_type: Some(RewardType::Fee),
            commission: Some(12),
        };

        let db_reward = DbReward::from(&reward);
        check_reward_equality(&reward, &db_reward);
    }

    fn check_transaction_token_balance_equality(
        transaction_token_balance: &TransactionTokenBalance,
        db_transaction_token_balance: &DbTransactionTokenBalance,
    ) {
        assert_eq!(
            transaction_token_balance.account_index,
            db_transaction_token_balance.account_index as u8
        );
        assert_eq!(
            transaction_token_balance.mint,
            db_transaction_token_balance.mint
        );
        assert_eq!(
            transaction_token_balance.ui_token_amount.ui_amount,
            db_transaction_token_balance.ui_token_amount
        );
        assert_eq!(
            transaction_token_balance.owner,
            db_transaction_token_balance.owner
        );
    }

    #[test]
    fn test_transform_transaction_token_balance() {
        let transaction_token_balance = TransactionTokenBalance {
            account_index: 3,
            mint: Pubkey::new_unique().to_string(),
            ui_token_amount: UiTokenAmount {
                ui_amount: Some(0.42),
                decimals: 2,
                amount: "42".to_string(),
                ui_amount_string: "0.42".to_string(),
            },
            owner: Pubkey::new_unique().to_string(),
        };

        let db_transaction_token_balance =
            DbTransactionTokenBalance::from(&transaction_token_balance);

        check_transaction_token_balance_equality(
            &transaction_token_balance,
            &db_transaction_token_balance,
        );
    }

    fn check_token_balances(
        token_balances: &Option<Vec<TransactionTokenBalance>>,
        db_token_balances: &Option<Vec<DbTransactionTokenBalance>>,
    ) {
        assert_eq!(
            token_balances
                .as_ref()
                .map(|token_balances| token_balances.len()),
            db_token_balances
                .as_ref()
                .map(|token_balances| token_balances.len()),
        );

        if token_balances.is_some() {
            for i in 0..token_balances.as_ref().unwrap().len() {
                check_transaction_token_balance_equality(
                    &token_balances.as_ref().unwrap()[i],
                    &db_token_balances.as_ref().unwrap()[i],
                );
            }
        }
    }

    fn check_transaction_status_meta(
        transaction_status_meta: &TransactionStatusMeta,
        db_transaction_status_meta: &DbTransactionStatusMeta,
    ) {
        assert_eq!(
            get_transaction_error(&transaction_status_meta.status),
            db_transaction_status_meta.error
        );
        assert_eq!(
            transaction_status_meta.fee,
            db_transaction_status_meta.fee as u64
        );
        assert_eq!(
            transaction_status_meta.pre_balances.len(),
            db_transaction_status_meta.pre_balances.len()
        );

        for i in 0..transaction_status_meta.pre_balances.len() {
            assert_eq!(
                transaction_status_meta.pre_balances[i],
                db_transaction_status_meta.pre_balances[i] as u64
            );
        }
        assert_eq!(
            transaction_status_meta.post_balances.len(),
            db_transaction_status_meta.post_balances.len()
        );
        for i in 0..transaction_status_meta.post_balances.len() {
            assert_eq!(
                transaction_status_meta.post_balances[i],
                db_transaction_status_meta.post_balances[i] as u64
            );
        }
        assert_eq!(
            transaction_status_meta
                .inner_instructions
                .as_ref()
                .map(|inner_instructions| inner_instructions.len()),
            db_transaction_status_meta
                .inner_instructions
                .as_ref()
                .map(|inner_instructions| inner_instructions.len()),
        );

        if transaction_status_meta.inner_instructions.is_some() {
            for i in 0..transaction_status_meta
                .inner_instructions
                .as_ref()
                .unwrap()
                .len()
            {
                check_inner_instructions_equality(
                    &transaction_status_meta.inner_instructions.as_ref().unwrap()[i],
                    &db_transaction_status_meta
                        .inner_instructions
                        .as_ref()
                        .unwrap()[i],
                );
            }
        }

        assert_eq!(
            transaction_status_meta
                .log_messages
                .as_ref()
                .map(|log_messages| log_messages.len()),
            db_transaction_status_meta
                .log_messages
                .as_ref()
                .map(|log_messages| log_messages.len()),
        );

        if transaction_status_meta.log_messages.is_some() {
            for i in 0..transaction_status_meta.log_messages.as_ref().unwrap().len() {
                assert_eq!(
                    &transaction_status_meta.log_messages.as_ref().unwrap()[i],
                    &db_transaction_status_meta.log_messages.as_ref().unwrap()[i]
                );
            }
        }

        check_token_balances(
            &transaction_status_meta.pre_token_balances,
            &db_transaction_status_meta.pre_token_balances,
        );

        check_token_balances(
            &transaction_status_meta.post_token_balances,
            &db_transaction_status_meta.post_token_balances,
        );

        assert_eq!(
            transaction_status_meta
                .rewards
                .as_ref()
                .map(|rewards| rewards.len()),
            db_transaction_status_meta
                .rewards
                .as_ref()
                .map(|rewards| rewards.len()),
        );

        if transaction_status_meta.rewards.is_some() {
            for i in 0..transaction_status_meta.rewards.as_ref().unwrap().len() {
                check_reward_equality(
                    &transaction_status_meta.rewards.as_ref().unwrap()[i],
                    &db_transaction_status_meta.rewards.as_ref().unwrap()[i],
                );
            }
        }
    }

    fn build_transaction_status_meta() -> TransactionStatusMeta {
        TransactionStatusMeta {
            status: Ok(()),
            fee: 23456,
            pre_balances: vec![11, 22, 33],
            post_balances: vec![44, 55, 66],
            inner_instructions: Some(vec![InnerInstructions {
                index: 0,
                instructions: vec![
                    CompiledInstruction {
                        program_id_index: 0,
                        accounts: vec![1, 2, 3],
                        data: vec![4, 5, 6],
                    },
                    CompiledInstruction {
                        program_id_index: 1,
                        accounts: vec![12, 13, 14],
                        data: vec![24, 25, 26],
                    },
                ],
            }]),
            log_messages: Some(vec!["message1".to_string(), "message2".to_string()]),
            pre_token_balances: Some(vec![
                TransactionTokenBalance {
                    account_index: 3,
                    mint: Pubkey::new_unique().to_string(),
                    ui_token_amount: UiTokenAmount {
                        ui_amount: Some(0.42),
                        decimals: 2,
                        amount: "42".to_string(),
                        ui_amount_string: "0.42".to_string(),
                    },
                    owner: Pubkey::new_unique().to_string(),
                },
                TransactionTokenBalance {
                    account_index: 2,
                    mint: Pubkey::new_unique().to_string(),
                    ui_token_amount: UiTokenAmount {
                        ui_amount: Some(0.38),
                        decimals: 2,
                        amount: "38".to_string(),
                        ui_amount_string: "0.38".to_string(),
                    },
                    owner: Pubkey::new_unique().to_string(),
                },
            ]),
            post_token_balances: Some(vec![
                TransactionTokenBalance {
                    account_index: 3,
                    mint: Pubkey::new_unique().to_string(),
                    ui_token_amount: UiTokenAmount {
                        ui_amount: Some(0.82),
                        decimals: 2,
                        amount: "82".to_string(),
                        ui_amount_string: "0.82".to_string(),
                    },
                    owner: Pubkey::new_unique().to_string(),
                },
                TransactionTokenBalance {
                    account_index: 2,
                    mint: Pubkey::new_unique().to_string(),
                    ui_token_amount: UiTokenAmount {
                        ui_amount: Some(0.48),
                        decimals: 2,
                        amount: "48".to_string(),
                        ui_amount_string: "0.48".to_string(),
                    },
                    owner: Pubkey::new_unique().to_string(),
                },
            ]),
            rewards: Some(vec![
                Reward {
                    pubkey: Pubkey::new_unique().to_string(),
                    lamports: 1234,
                    post_balance: 45678,
                    reward_type: Some(RewardType::Fee),
                    commission: Some(12),
                },
                Reward {
                    pubkey: Pubkey::new_unique().to_string(),
                    lamports: 234,
                    post_balance: 324,
                    reward_type: Some(RewardType::Staking),
                    commission: Some(11),
                },
            ]),
        }
    }

    #[test]
    fn test_transform_transaction_status_meta() {
        let transaction_status_meta = build_transaction_status_meta();
        let db_transaction_status_meta = DbTransactionStatusMeta::from(&transaction_status_meta);
        check_transaction_status_meta(&transaction_status_meta, &db_transaction_status_meta);
    }

    fn check_message_header_equality(
        message_header: &MessageHeader,
        db_message_header: &DbTransactionMessageHeader,
    ) {
        assert_eq!(
            message_header.num_readonly_signed_accounts,
            db_message_header.num_readonly_signed_accounts as u8
        );
        assert_eq!(
            message_header.num_readonly_unsigned_accounts,
            db_message_header.num_readonly_unsigned_accounts as u8
        );
        assert_eq!(
            message_header.num_required_signatures,
            db_message_header.num_required_signatures as u8
        );
    }

    #[test]
    fn test_transform_transaction_message_header() {
        let message_header = MessageHeader {
            num_readonly_signed_accounts: 1,
            num_readonly_unsigned_accounts: 2,
            num_required_signatures: 3,
        };

        let db_message_header = DbTransactionMessageHeader::from(&message_header);
        check_message_header_equality(&message_header, &db_message_header)
    }

    fn check_transaction_message_equality(message: &Message, db_message: &DbTransactionMessage) {
        check_message_header_equality(&message.header, &db_message.header);
        assert_eq!(message.account_keys.len(), db_message.account_keys.len());
        for i in 0..message.account_keys.len() {
            assert_eq!(message.account_keys[i].as_ref(), db_message.account_keys[i]);
        }
        assert_eq!(message.instructions.len(), db_message.instructions.len());
        for i in 0..message.instructions.len() {
            check_compiled_instruction_equality(
                &message.instructions[i],
                &db_message.instructions[i],
            );
        }
    }

    fn build_message() -> Message {
        Message {
            header: MessageHeader {
                num_readonly_signed_accounts: 11,
                num_readonly_unsigned_accounts: 12,
                num_required_signatures: 13,
            },
            account_keys: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            recent_blockhash: Hash::new_unique(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 0,
                    accounts: vec![1, 2, 3],
                    data: vec![4, 5, 6],
                },
                CompiledInstruction {
                    program_id_index: 3,
                    accounts: vec![11, 12, 13],
                    data: vec![14, 15, 16],
                },
            ],
        }
    }

    #[test]
    fn test_transform_transaction_message() {
        let message = build_message();

        let db_message = DbTransactionMessage::from(&message);
        check_transaction_message_equality(&message, &db_message);
    }

    fn check_transaction_message_v0_equality(
        message: &v0::Message,
        db_message: &DbTransactionMessageV0,
    ) {
        check_message_header_equality(&message.header, &db_message.header);
        assert_eq!(message.account_keys.len(), db_message.account_keys.len());
        for i in 0..message.account_keys.len() {
            assert_eq!(message.account_keys[i].as_ref(), db_message.account_keys[i]);
        }
        assert_eq!(message.instructions.len(), db_message.instructions.len());
        for i in 0..message.instructions.len() {
            check_compiled_instruction_equality(
                &message.instructions[i],
                &db_message.instructions[i],
            );
        }
        assert_eq!(
            message.address_table_lookups.len(),
            db_message.address_table_lookups.len()
        );
        for i in 0..message.address_table_lookups.len() {
            check_address_table_lookups_equality(
                &message.address_table_lookups[i],
                &db_message.address_table_lookups[i],
            );
        }
    }

    fn build_transaction_message_v0() -> v0::Message {
        v0::Message {
            header: MessageHeader {
                num_readonly_signed_accounts: 2,
                num_readonly_unsigned_accounts: 2,
                num_required_signatures: 3,
            },
            account_keys: vec![
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
                Pubkey::new_unique(),
            ],
            recent_blockhash: Hash::new_unique(),
            instructions: vec![
                CompiledInstruction {
                    program_id_index: 1,
                    accounts: vec![1, 2, 3],
                    data: vec![4, 5, 6],
                },
                CompiledInstruction {
                    program_id_index: 2,
                    accounts: vec![0, 1, 2],
                    data: vec![14, 15, 16],
                },
            ],
            address_table_lookups: vec![
                MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![0],
                    readonly_indexes: vec![1, 2],
                },
                MessageAddressTableLookup {
                    account_key: Pubkey::new_unique(),
                    writable_indexes: vec![1],
                    readonly_indexes: vec![0, 2],
                },
            ],
        }
    }

    #[test]
    fn test_transform_transaction_message_v0() {
        let message = build_transaction_message_v0();

        let db_message = DbTransactionMessageV0::from(&message);
        check_transaction_message_v0_equality(&message, &db_message);
    }

    fn check_loaded_addresses(
        loaded_addresses: &LoadedAddresses,
        db_loaded_addresses: &DbLoadedAddresses,
    ) {
        assert_eq!(
            loaded_addresses.writable.len(),
            db_loaded_addresses.writable.len()
        );
        for i in 0..loaded_addresses.writable.len() {
            assert_eq!(
                loaded_addresses.writable[i].as_ref(),
                db_loaded_addresses.writable[i]
            );
        }

        assert_eq!(
            loaded_addresses.readonly.len(),
            db_loaded_addresses.readonly.len()
        );
        for i in 0..loaded_addresses.readonly.len() {
            assert_eq!(
                loaded_addresses.readonly[i].as_ref(),
                db_loaded_addresses.readonly[i]
            );
        }
    }

    fn check_loaded_message_v0_equality(
        message: &v0::LoadedMessage,
        db_message: &DbLoadedMessageV0,
    ) {
        check_transaction_message_v0_equality(&message.message, &db_message.message);
        check_loaded_addresses(&message.loaded_addresses, &db_message.loaded_addresses);
    }

    #[test]
    fn test_transform_loaded_message_v0() {
        let message = v0::LoadedMessage {
            message: build_transaction_message_v0(),
            loaded_addresses: LoadedAddresses {
                writable: vec![Pubkey::new_unique(), Pubkey::new_unique()],
                readonly: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            },
        };

        let db_message = DbLoadedMessageV0::from(&message);
        check_loaded_message_v0_equality(&message, &db_message);
    }

    fn check_transaction(
        slot: u64,
        transaction: &ReplicaTransactionInfo,
        db_transaction: &DbTransaction,
    ) {
        assert_eq!(transaction.signature.as_ref(), db_transaction.signature);
        assert_eq!(transaction.is_vote, db_transaction.is_vote);
        assert_eq!(slot, db_transaction.slot as u64);
        match transaction.transaction.message() {
            SanitizedMessage::Legacy(message) => {
                assert_eq!(db_transaction.message_type, 0);
                check_transaction_message_equality(
                    message,
                    db_transaction.legacy_message.as_ref().unwrap(),
                );
            }
            SanitizedMessage::V0(message) => {
                assert_eq!(db_transaction.message_type, 1);
                check_loaded_message_v0_equality(
                    message,
                    db_transaction.v0_loaded_message.as_ref().unwrap(),
                );
            }
        }

        assert_eq!(
            transaction.transaction.signatures().len(),
            db_transaction.signatures.len()
        );

        for i in 0..transaction.transaction.signatures().len() {
            assert_eq!(
                transaction.transaction.signatures()[i].as_ref(),
                db_transaction.signatures[i]
            );
        }

        assert_eq!(
            transaction.transaction.message_hash().as_ref(),
            db_transaction.message_hash
        );

        check_transaction_status_meta(transaction.transaction_status_meta, &db_transaction.meta);
    }

    fn build_test_transaction_legacy() -> Transaction {
        let keypair1 = Keypair::new();
        let pubkey1 = keypair1.pubkey();
        let zero = Hash::default();
        system_transaction::transfer(&keypair1, &pubkey1, 42, zero)
    }

    #[test]
    fn test_build_db_transaction_legacy() {
        let signature = Signature::new(&[1u8; 64]);

        let message_hash = Hash::new_unique();
        let transaction = build_test_transaction_legacy();

        let transaction = VersionedTransaction::from(transaction);

        let transaction =
            SanitizedTransaction::try_create(transaction, message_hash, Some(true), |_| {
                Err(TransactionError::UnsupportedVersion)
            })
            .unwrap();

        let transaction_status_meta = build_transaction_status_meta();
        let transaction_info = ReplicaTransactionInfo {
            signature: &signature,
            is_vote: false,
            transaction: &transaction,
            transaction_status_meta: &transaction_status_meta,
        };

        let slot = 54;
        let db_transaction = build_db_transaction(slot, &transaction_info);
        check_transaction(slot, &transaction_info, &db_transaction);
    }

    fn build_test_transaction_v0() -> VersionedTransaction {
        VersionedTransaction {
            signatures: vec![
                Signature::new(&[1u8; 64]),
                Signature::new(&[2u8; 64]),
                Signature::new(&[3u8; 64]),
            ],
            message: VersionedMessage::V0(build_transaction_message_v0()),
        }
    }

    #[test]
    fn test_build_db_transaction_v0() {
        let signature = Signature::new(&[1u8; 64]);

        let message_hash = Hash::new_unique();
        let transaction = build_test_transaction_v0();

        transaction.sanitize().unwrap();

        let transaction =
            SanitizedTransaction::try_create(transaction, message_hash, Some(true), |_message| {
                Ok(LoadedAddresses {
                    writable: vec![Pubkey::new_unique(), Pubkey::new_unique()],
                    readonly: vec![Pubkey::new_unique(), Pubkey::new_unique()],
                })
            })
            .unwrap();

        let transaction_status_meta = build_transaction_status_meta();
        let transaction_info = ReplicaTransactionInfo {
            signature: &signature,
            is_vote: true,
            transaction: &transaction,
            transaction_status_meta: &transaction_status_meta,
        };

        let slot = 54;
        let db_transaction = build_db_transaction(slot, &transaction_info);
        check_transaction(slot, &transaction_info, &db_transaction);
    }
}
