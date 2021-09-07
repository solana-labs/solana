#![allow(clippy::integer_arithmetic)]
use {
    log::*,
    serde::{Deserialize, Serialize},
    solana_sdk::{
        clock::{Slot, UnixTimestamp},
        deserialize_utils::default_on_eof,
        pubkey::Pubkey,
        signature::Signature,
        sysvar::is_sysvar_id,
        transaction::{Transaction, TransactionError},
    },
    solana_storage_proto::convert::{generated, tx_by_addr},
    solana_transaction_status::{
        extract_and_fmt_memos, ConfirmedBlock, ConfirmedTransaction,
        ConfirmedTransactionStatusWithSignature, Reward, TransactionByAddrInfo,
        TransactionConfirmationStatus, TransactionStatus, TransactionStatusMeta,
        TransactionWithStatusMeta,
    },
    std::{collections::HashMap, convert::TryInto},
    thiserror::Error,
};

#[macro_use]
extern crate serde_derive;

mod access_token;
mod bigtable;
mod compression;
mod root_ca_certificate;

#[derive(Debug, Error)]
pub enum Error {
    #[error("BigTable: {0}")]
    BigTableError(bigtable::Error),

    #[error("I/O Error: {0}")]
    IoError(std::io::Error),

    #[error("Transaction encoded is not supported")]
    UnsupportedTransactionEncoding,

    #[error("Block not found: {0}")]
    BlockNotFound(Slot),

    #[error("Signature not found")]
    SignatureNotFound,
}

impl std::convert::From<bigtable::Error> for Error {
    fn from(err: bigtable::Error) -> Self {
        Self::BigTableError(err)
    }
}

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// Convert a slot to its bucket representation whereby lower slots are always lexically ordered
// before higher slots
fn slot_to_key(slot: Slot) -> String {
    format!("{:016x}", slot)
}

// Reverse of `slot_to_key`
fn key_to_slot(key: &str) -> Option<Slot> {
    match Slot::from_str_radix(key, 16) {
        Ok(slot) => Some(slot),
        Err(err) => {
            // bucket data is probably corrupt
            warn!("Failed to parse object key as a slot: {}: {}", key, err);
            None
        }
    }
}

// A serialized `StoredConfirmedBlock` is stored in the `block` table
//
// StoredConfirmedBlock holds the same contents as ConfirmedBlock, but is slightly compressed and avoids
// some serde JSON directives that cause issues with bincode
//
// Note: in order to continue to support old bincode-serialized bigtable entries, if new fields are
// added to ConfirmedBlock, they must either be excluded or set to `default_on_eof` here
//
#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlock {
    previous_blockhash: String,
    blockhash: String,
    parent_slot: Slot,
    transactions: Vec<StoredConfirmedBlockTransaction>,
    rewards: StoredConfirmedBlockRewards,
    block_time: Option<UnixTimestamp>,
    #[serde(deserialize_with = "default_on_eof")]
    block_height: Option<u64>,
}

impl From<ConfirmedBlock> for StoredConfirmedBlock {
    fn from(confirmed_block: ConfirmedBlock) -> Self {
        let ConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
            block_height,
        } = confirmed_block;

        Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions.into_iter().map(|tx| tx.into()).collect(),
            rewards: rewards.into_iter().map(|reward| reward.into()).collect(),
            block_time,
            block_height,
        }
    }
}

impl From<StoredConfirmedBlock> for ConfirmedBlock {
    fn from(confirmed_block: StoredConfirmedBlock) -> Self {
        let StoredConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
            block_height,
        } = confirmed_block;

        Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions.into_iter().map(|tx| tx.into()).collect(),
            rewards: rewards.into_iter().map(|reward| reward.into()).collect(),
            block_time,
            block_height,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlockTransaction {
    transaction: Transaction,
    meta: Option<StoredConfirmedBlockTransactionStatusMeta>,
}

impl From<TransactionWithStatusMeta> for StoredConfirmedBlockTransaction {
    fn from(value: TransactionWithStatusMeta) -> Self {
        Self {
            transaction: value.transaction,
            meta: value.meta.map(|meta| meta.into()),
        }
    }
}

impl From<StoredConfirmedBlockTransaction> for TransactionWithStatusMeta {
    fn from(value: StoredConfirmedBlockTransaction) -> Self {
        Self {
            transaction: value.transaction,
            meta: value.meta.map(|meta| meta.into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlockTransactionStatusMeta {
    err: Option<TransactionError>,
    fee: u64,
    pre_balances: Vec<u64>,
    post_balances: Vec<u64>,
}

impl From<StoredConfirmedBlockTransactionStatusMeta> for TransactionStatusMeta {
    fn from(value: StoredConfirmedBlockTransactionStatusMeta) -> Self {
        let StoredConfirmedBlockTransactionStatusMeta {
            err,
            fee,
            pre_balances,
            post_balances,
        } = value;
        let status = match &err {
            None => Ok(()),
            Some(err) => Err(err.clone()),
        };
        Self {
            status,
            fee,
            pre_balances,
            post_balances,
            inner_instructions: None,
            log_messages: None,
            pre_token_balances: None,
            post_token_balances: None,
            rewards: None,
        }
    }
}

impl From<TransactionStatusMeta> for StoredConfirmedBlockTransactionStatusMeta {
    fn from(value: TransactionStatusMeta) -> Self {
        let TransactionStatusMeta {
            status,
            fee,
            pre_balances,
            post_balances,
            ..
        } = value;
        Self {
            err: status.err(),
            fee,
            pre_balances,
            post_balances,
        }
    }
}

type StoredConfirmedBlockRewards = Vec<StoredConfirmedBlockReward>;

#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlockReward {
    pubkey: String,
    lamports: i64,
}

impl From<StoredConfirmedBlockReward> for Reward {
    fn from(value: StoredConfirmedBlockReward) -> Self {
        let StoredConfirmedBlockReward { pubkey, lamports } = value;
        Self {
            pubkey,
            lamports,
            post_balance: 0,
            reward_type: None,
            commission: None,
        }
    }
}

impl From<Reward> for StoredConfirmedBlockReward {
    fn from(value: Reward) -> Self {
        let Reward {
            pubkey, lamports, ..
        } = value;
        Self { pubkey, lamports }
    }
}

// A serialized `TransactionInfo` is stored in the `tx` table
#[derive(Serialize, Deserialize)]
struct TransactionInfo {
    slot: Slot, // The slot that contains the block with this transaction in it
    index: u32, // Where the transaction is located in the block
    err: Option<TransactionError>, // None if the transaction executed successfully
    memo: Option<String>, // Transaction memo
}

impl From<TransactionInfo> for TransactionStatus {
    fn from(transaction_info: TransactionInfo) -> Self {
        let TransactionInfo { slot, err, .. } = transaction_info;
        let status = match &err {
            None => Ok(()),
            Some(err) => Err(err.clone()),
        };
        Self {
            slot,
            confirmations: None,
            status,
            err,
            confirmation_status: Some(TransactionConfirmationStatus::Finalized),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct LegacyTransactionByAddrInfo {
    pub signature: Signature,          // The transaction signature
    pub err: Option<TransactionError>, // None if the transaction executed successfully
    pub index: u32,                    // Where the transaction is located in the block
    pub memo: Option<String>,          // Transaction memo
}

impl From<LegacyTransactionByAddrInfo> for TransactionByAddrInfo {
    fn from(legacy: LegacyTransactionByAddrInfo) -> Self {
        let LegacyTransactionByAddrInfo {
            signature,
            err,
            index,
            memo,
        } = legacy;

        Self {
            signature,
            err,
            index,
            memo,
            block_time: None,
        }
    }
}

#[derive(Clone)]
pub struct LedgerStorage {
    connection: bigtable::BigTableConnection,
}

impl LedgerStorage {
    pub async fn new(read_only: bool, timeout: Option<std::time::Duration>) -> Result<Self> {
        let connection =
            bigtable::BigTableConnection::new("solana-ledger", read_only, timeout).await?;
        Ok(Self { connection })
    }

    /// Return the available slot that contains a block
    pub async fn get_first_available_block(&self) -> Result<Option<Slot>> {
        let mut bigtable = self.connection.client();
        let blocks = bigtable.get_row_keys("blocks", None, None, 1).await?;
        if blocks.is_empty() {
            return Ok(None);
        }
        Ok(key_to_slot(&blocks[0]))
    }

    /// Fetch the next slots after the provided slot that contains a block
    ///
    /// start_slot: slot to start the search from (inclusive)
    /// limit: stop after this many slots have been found; if limit==0, all records in the table
    /// after start_slot will be read
    pub async fn get_confirmed_blocks(&self, start_slot: Slot, limit: usize) -> Result<Vec<Slot>> {
        let mut bigtable = self.connection.client();
        let blocks = bigtable
            .get_row_keys("blocks", Some(slot_to_key(start_slot)), None, limit as i64)
            .await?;
        Ok(blocks.into_iter().filter_map(|s| key_to_slot(&s)).collect())
    }

    /// Fetch the confirmed block from the desired slot
    pub async fn get_confirmed_block(&self, slot: Slot) -> Result<ConfirmedBlock> {
        let mut bigtable = self.connection.client();
        let block_cell_data = bigtable
            .get_protobuf_or_bincode_cell::<StoredConfirmedBlock, generated::ConfirmedBlock>(
                "blocks",
                slot_to_key(slot),
            )
            .await
            .map_err(|err| match err {
                bigtable::Error::RowNotFound => Error::BlockNotFound(slot),
                _ => err.into(),
            })?;
        Ok(match block_cell_data {
            bigtable::CellData::Bincode(block) => block.into(),
            bigtable::CellData::Protobuf(block) => block.try_into().map_err(|_err| {
                bigtable::Error::ObjectCorrupt(format!("blocks/{}", slot_to_key(slot)))
            })?,
        })
    }

    pub async fn get_signature_status(&self, signature: &Signature) -> Result<TransactionStatus> {
        let mut bigtable = self.connection.client();
        let transaction_info = bigtable
            .get_bincode_cell::<TransactionInfo>("tx", signature.to_string())
            .await
            .map_err(|err| match err {
                bigtable::Error::RowNotFound => Error::SignatureNotFound,
                _ => err.into(),
            })?;
        Ok(transaction_info.into())
    }

    /// Fetch a confirmed transaction
    pub async fn get_confirmed_transaction(
        &self,
        signature: &Signature,
    ) -> Result<Option<ConfirmedTransaction>> {
        let mut bigtable = self.connection.client();

        // Figure out which block the transaction is located in
        let TransactionInfo { slot, index, .. } = bigtable
            .get_bincode_cell("tx", signature.to_string())
            .await
            .map_err(|err| match err {
                bigtable::Error::RowNotFound => Error::SignatureNotFound,
                _ => err.into(),
            })?;

        // Load the block and return the transaction
        let block = self.get_confirmed_block(slot).await?;
        match block.transactions.into_iter().nth(index as usize) {
            None => {
                warn!("Transaction info for {} is corrupt", signature);
                Ok(None)
            }
            Some(bucket_block_transaction) => {
                if bucket_block_transaction.transaction.signatures[0] != *signature {
                    warn!(
                        "Transaction info or confirmed block for {} is corrupt",
                        signature
                    );
                    Ok(None)
                } else {
                    Ok(Some(ConfirmedTransaction {
                        slot,
                        transaction: bucket_block_transaction,
                        block_time: block.block_time,
                    }))
                }
            }
        }
    }

    /// Get confirmed signatures for the provided address, in descending ledger order
    ///
    /// address: address to search for
    /// before_signature: start with the first signature older than this one
    /// until_signature: end with the last signature more recent than this one
    /// limit: stop after this many signatures; if limit==0, all records in the table will be read
    pub async fn get_confirmed_signatures_for_address(
        &self,
        address: &Pubkey,
        before_signature: Option<&Signature>,
        until_signature: Option<&Signature>,
        limit: usize,
    ) -> Result<
        Vec<(
            ConfirmedTransactionStatusWithSignature,
            u32, /*slot index*/
        )>,
    > {
        let mut bigtable = self.connection.client();
        let address_prefix = format!("{}/", address);

        // Figure out where to start listing from based on `before_signature`
        let (first_slot, before_transaction_index) = match before_signature {
            None => (Slot::MAX, 0),
            Some(before_signature) => {
                let TransactionInfo { slot, index, .. } = bigtable
                    .get_bincode_cell("tx", before_signature.to_string())
                    .await?;

                (slot, index)
            }
        };

        // Figure out where to end listing from based on `until_signature`
        let (last_slot, until_transaction_index) = match until_signature {
            None => (0, u32::MAX),
            Some(until_signature) => {
                let TransactionInfo { slot, index, .. } = bigtable
                    .get_bincode_cell("tx", until_signature.to_string())
                    .await?;

                (slot, index)
            }
        };

        let mut infos = vec![];

        let starting_slot_tx_len = bigtable
            .get_protobuf_or_bincode_cell::<Vec<LegacyTransactionByAddrInfo>, tx_by_addr::TransactionByAddr>(
                "tx-by-addr",
                format!("{}{}", address_prefix, slot_to_key(!first_slot)),
            )
            .await
            .map(|cell_data| {
                match cell_data {
                    bigtable::CellData::Bincode(tx_by_addr) => tx_by_addr.len(),
                    bigtable::CellData::Protobuf(tx_by_addr) => tx_by_addr.tx_by_addrs.len(),
                }
            })
            .unwrap_or(0);

        // Return the next tx-by-addr data of amount `limit` plus extra to account for the largest
        // number that might be flitered out
        let tx_by_addr_data = bigtable
            .get_row_data(
                "tx-by-addr",
                Some(format!("{}{}", address_prefix, slot_to_key(!first_slot))),
                Some(format!("{}{}", address_prefix, slot_to_key(!last_slot))),
                limit as i64 + starting_slot_tx_len as i64,
            )
            .await?;

        'outer: for (row_key, data) in tx_by_addr_data {
            let slot = !key_to_slot(&row_key[address_prefix.len()..]).ok_or_else(|| {
                bigtable::Error::ObjectCorrupt(format!(
                    "Failed to convert key to slot: tx-by-addr/{}",
                    row_key
                ))
            })?;

            let deserialized_cell_data = bigtable::deserialize_protobuf_or_bincode_cell_data::<
                Vec<LegacyTransactionByAddrInfo>,
                tx_by_addr::TransactionByAddr,
            >(&data, "tx-by-addr", row_key.clone())?;

            let mut cell_data: Vec<TransactionByAddrInfo> = match deserialized_cell_data {
                bigtable::CellData::Bincode(tx_by_addr) => {
                    tx_by_addr.into_iter().map(|legacy| legacy.into()).collect()
                }
                bigtable::CellData::Protobuf(tx_by_addr) => {
                    tx_by_addr.try_into().map_err(|error| {
                        bigtable::Error::ObjectCorrupt(format!(
                            "Failed to deserialize: {}: tx-by-addr/{}",
                            error,
                            row_key.clone()
                        ))
                    })?
                }
            };

            cell_data.reverse();
            for tx_by_addr_info in cell_data.into_iter() {
                // Filter out records before `before_transaction_index`
                if slot == first_slot && tx_by_addr_info.index >= before_transaction_index {
                    continue;
                }
                // Filter out records after `until_transaction_index`
                if slot == last_slot && tx_by_addr_info.index <= until_transaction_index {
                    continue;
                }
                infos.push((
                    ConfirmedTransactionStatusWithSignature {
                        signature: tx_by_addr_info.signature,
                        slot,
                        err: tx_by_addr_info.err,
                        memo: tx_by_addr_info.memo,
                        block_time: tx_by_addr_info.block_time,
                    },
                    tx_by_addr_info.index,
                ));
                // Respect limit
                if infos.len() >= limit {
                    break 'outer;
                }
            }
        }
        Ok(infos)
    }

    // Upload a new confirmed block and associated meta data.
    pub async fn upload_confirmed_block(
        &self,
        slot: Slot,
        confirmed_block: ConfirmedBlock,
    ) -> Result<()> {
        let mut bytes_written = 0;

        let mut by_addr: HashMap<&Pubkey, Vec<TransactionByAddrInfo>> = HashMap::new();

        let mut tx_cells = vec![];
        for (index, transaction_with_meta) in confirmed_block.transactions.iter().enumerate() {
            let TransactionWithStatusMeta { meta, transaction } = transaction_with_meta;
            let err = meta.as_ref().and_then(|meta| meta.status.clone().err());
            let index = index as u32;
            let signature = transaction.signatures[0];
            let memo = extract_and_fmt_memos(&transaction.message);

            for address in &transaction.message.account_keys {
                if !is_sysvar_id(address) {
                    by_addr
                        .entry(address)
                        .or_default()
                        .push(TransactionByAddrInfo {
                            signature,
                            err: err.clone(),
                            index,
                            memo: memo.clone(),
                            block_time: confirmed_block.block_time,
                        });
                }
            }

            tx_cells.push((
                signature.to_string(),
                TransactionInfo {
                    slot,
                    index,
                    err,
                    memo,
                },
            ));
        }

        let tx_by_addr_cells: Vec<_> = by_addr
            .into_iter()
            .map(|(address, transaction_info_by_addr)| {
                (
                    format!("{}/{}", address, slot_to_key(!slot)),
                    tx_by_addr::TransactionByAddr {
                        tx_by_addrs: transaction_info_by_addr
                            .into_iter()
                            .map(|by_addr| by_addr.into())
                            .collect(),
                    },
                )
            })
            .collect();

        if !tx_cells.is_empty() {
            bytes_written += self
                .connection
                .put_bincode_cells_with_retry::<TransactionInfo>("tx", &tx_cells)
                .await?;
        }

        if !tx_by_addr_cells.is_empty() {
            bytes_written += self
                .connection
                .put_protobuf_cells_with_retry::<tx_by_addr::TransactionByAddr>(
                    "tx-by-addr",
                    &tx_by_addr_cells,
                )
                .await?;
        }

        let num_transactions = confirmed_block.transactions.len();

        // Store the block itself last, after all other metadata about the block has been
        // successfully stored.  This avoids partial uploaded blocks from becoming visible to
        // `get_confirmed_block()` and `get_confirmed_blocks()`
        let blocks_cells = [(slot_to_key(slot), confirmed_block.into())];
        bytes_written += self
            .connection
            .put_protobuf_cells_with_retry::<generated::ConfirmedBlock>("blocks", &blocks_cells)
            .await?;
        info!(
            "uploaded block for slot {}: {} transactions, {} bytes",
            slot, num_transactions, bytes_written
        );

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_slot_to_key() {
        assert_eq!(slot_to_key(0), "0000000000000000");
        assert_eq!(slot_to_key(!0), "ffffffffffffffff");
    }
}
