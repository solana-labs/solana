use log::*;
use serde::{Deserialize, Serialize};
use solana_sdk::{
    clock::{Slot, UnixTimestamp},
    pubkey::Pubkey,
    signature::Signature,
    sysvar::is_sysvar_id,
    transaction::{Transaction, TransactionError},
};
use solana_transaction_status::{
    ConfirmedBlock, ConfirmedTransaction, EncodedTransaction, Rewards, TransactionStatus,
    TransactionWithStatusMeta, UiTransactionEncoding, UiTransactionStatusMeta,
};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
};
use thiserror::Error;

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
#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlock {
    previous_blockhash: String,
    blockhash: String,
    parent_slot: Slot,
    transactions: Vec<StoredConfirmedBlockTransaction>,
    rewards: Rewards,
    block_time: Option<UnixTimestamp>,
}

impl StoredConfirmedBlock {
    fn into_confirmed_block(self, encoding: UiTransactionEncoding) -> ConfirmedBlock {
        let StoredConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
        } = self;

        ConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: transactions
                .into_iter()
                .map(|transaction| transaction.into_transaction_with_status_meta(encoding))
                .collect(),
            rewards,
            block_time,
        }
    }
}

impl TryFrom<ConfirmedBlock> for StoredConfirmedBlock {
    type Error = Error;

    fn try_from(confirmed_block: ConfirmedBlock) -> Result<Self> {
        let ConfirmedBlock {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions,
            rewards,
            block_time,
        } = confirmed_block;

        let mut encoded_transactions = vec![];
        for transaction in transactions.into_iter() {
            encoded_transactions.push(transaction.try_into()?);
        }

        Ok(Self {
            previous_blockhash,
            blockhash,
            parent_slot,
            transactions: encoded_transactions,
            rewards,
            block_time,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlockTransaction {
    transaction: Transaction,
    meta: Option<StoredConfirmedBlockTransactionStatusMeta>,
}

impl StoredConfirmedBlockTransaction {
    fn into_transaction_with_status_meta(
        self,
        encoding: UiTransactionEncoding,
    ) -> TransactionWithStatusMeta {
        let StoredConfirmedBlockTransaction { transaction, meta } = self;
        TransactionWithStatusMeta {
            transaction: EncodedTransaction::encode(transaction, encoding),
            meta: meta.map(|meta| meta.into()),
        }
    }
}

impl TryFrom<TransactionWithStatusMeta> for StoredConfirmedBlockTransaction {
    type Error = Error;

    fn try_from(value: TransactionWithStatusMeta) -> Result<Self> {
        let TransactionWithStatusMeta { transaction, meta } = value;

        Ok(Self {
            transaction: transaction
                .decode()
                .ok_or(Error::UnsupportedTransactionEncoding)?,
            meta: meta.map(|meta| meta.into()),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct StoredConfirmedBlockTransactionStatusMeta {
    err: Option<TransactionError>,
    fee: u64,
    pre_balances: Vec<u64>,
    post_balances: Vec<u64>,
}

impl From<StoredConfirmedBlockTransactionStatusMeta> for UiTransactionStatusMeta {
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
            err,
            status,
            fee,
            pre_balances,
            post_balances,
        }
    }
}

impl From<UiTransactionStatusMeta> for StoredConfirmedBlockTransactionStatusMeta {
    fn from(value: UiTransactionStatusMeta) -> Self {
        let UiTransactionStatusMeta {
            err,
            fee,
            pre_balances,
            post_balances,
            ..
        } = value;
        Self {
            err,
            fee,
            pre_balances,
            post_balances,
        }
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
        }
    }
}

// A serialized `Vec<TransactionByAddrInfo>` is stored in the `tx-by-addr` table.  The row keys are
// the one's compliment of the slot so that rows may be listed in reverse order
#[derive(Serialize, Deserialize)]
struct TransactionByAddrInfo {
    signature: Signature,          // The transaction signature
    err: Option<TransactionError>, // None if the transaction executed successfully
    index: u32,                    // Where the transaction is located in the block
    memo: Option<String>,          // Transaction memo
}

#[derive(Clone)]
pub struct LedgerStorage {
    connection: bigtable::BigTableConnection,
}

impl LedgerStorage {
    pub async fn new(read_only: bool) -> Result<Self> {
        let connection = bigtable::BigTableConnection::new("solana-ledger", read_only).await?;
        Ok(Self { connection })
    }

    /// Return the available slot that contains a block
    pub async fn get_first_available_block(&self) -> Result<Option<Slot>> {
        let mut bigtable = self.connection.client();
        let blocks = bigtable.get_row_keys("blocks", None, 1).await?;
        if blocks.is_empty() {
            return Ok(None);
        }
        Ok(key_to_slot(&blocks[0]))
    }

    /// Fetch the next slots after the provided slot that contains a block
    ///
    /// start_slot: slot to start the search from (inclusive)
    /// limit: stop after this many slots have been found.
    pub async fn get_confirmed_blocks(&self, start_slot: Slot, limit: usize) -> Result<Vec<Slot>> {
        let mut bigtable = self.connection.client();
        let blocks = bigtable
            .get_row_keys("blocks", Some(slot_to_key(start_slot)), limit as i64)
            .await?;
        Ok(blocks.into_iter().filter_map(|s| key_to_slot(&s)).collect())
    }

    /// Fetch the confirmed block from the desired slot
    pub async fn get_confirmed_block(
        &self,
        slot: Slot,
        encoding: UiTransactionEncoding,
    ) -> Result<ConfirmedBlock> {
        let mut bigtable = self.connection.client();
        let block = bigtable
            .get_bincode_cell::<StoredConfirmedBlock>("blocks", slot_to_key(slot))
            .await?;
        Ok(block.into_confirmed_block(encoding))
    }

    pub async fn get_signature_status(&self, signature: &Signature) -> Result<TransactionStatus> {
        let mut bigtable = self.connection.client();
        let transaction_info = bigtable
            .get_bincode_cell::<TransactionInfo>("tx", signature.to_string())
            .await?;
        Ok(transaction_info.into())
    }

    /// Fetch a confirmed transaction
    pub async fn get_confirmed_transaction(
        &self,
        signature: &Signature,
        encoding: UiTransactionEncoding,
    ) -> Result<Option<ConfirmedTransaction>> {
        let mut bigtable = self.connection.client();

        // Figure out which block the transaction is located in
        let TransactionInfo { slot, index, .. } = bigtable
            .get_bincode_cell("tx", signature.to_string())
            .await?;

        // Load the block and return the transaction
        let block = bigtable
            .get_bincode_cell::<StoredConfirmedBlock>("blocks", slot_to_key(slot))
            .await?;
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
                        transaction: bucket_block_transaction
                            .into_transaction_with_status_meta(encoding),
                    }))
                }
            }
        }
    }

    /// Get confirmed signatures for the provided address, in descending ledger order
    ///
    /// address: address to search for
    /// start_after_signature: start with the first signature older than this one
    /// limit: stop after this many signatures.
    pub async fn get_confirmed_signatures_for_address(
        &self,
        address: &Pubkey,
        start_after_signature: Option<&Signature>,
        limit: usize,
    ) -> Result<Vec<(Signature, Slot, Option<String>, Option<TransactionError>)>> {
        let mut bigtable = self.connection.client();
        let address_prefix = format!("{}/", address);

        // Figure out where to start listing from based on `start_after_signature`
        let (first_slot, mut first_transaction_index) = match start_after_signature {
            None => (Slot::MAX, 0),
            Some(start_after_signature) => {
                let TransactionInfo { slot, index, .. } = bigtable
                    .get_bincode_cell("tx", start_after_signature.to_string())
                    .await?;

                (slot, index + 1)
            }
        };

        let mut infos = vec![];

        // Return the next `limit` tx-by-addr keys
        let tx_by_addr_info_keys = bigtable
            .get_row_keys(
                "tx-by-addr",
                Some(format!("{}{}", address_prefix, slot_to_key(!first_slot))),
                limit as i64,
            )
            .await?;

        // Read each tx-by-addr object until `limit` signatures have been found
        'outer: for key in tx_by_addr_info_keys {
            trace!("key is {}:  slot is {}", key, &key[address_prefix.len()..]);
            if !key.starts_with(&address_prefix) {
                break 'outer;
            }

            let slot = !key_to_slot(&key[address_prefix.len()..]).ok_or_else(|| {
                bigtable::Error::ObjectCorrupt(format!(
                    "Failed to convert key to slot: tx-by-addr/{}",
                    key
                ))
            })?;

            let tx_by_addr_infos = bigtable
                .get_bincode_cell::<Vec<TransactionByAddrInfo>>("tx-by-addr", key)
                .await?;

            for tx_by_addr_info in tx_by_addr_infos
                .into_iter()
                .skip(first_transaction_index as usize)
            {
                infos.push((
                    tx_by_addr_info.signature,
                    slot,
                    tx_by_addr_info.memo,
                    tx_by_addr_info.err,
                ));
                if infos.len() >= limit {
                    break 'outer;
                }
            }

            first_transaction_index = 0;
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

        let mut by_addr: HashMap<Pubkey, Vec<TransactionByAddrInfo>> = HashMap::new();

        let mut tx_cells = vec![];
        for (index, transaction_with_meta) in confirmed_block.transactions.iter().enumerate() {
            let err = transaction_with_meta
                .meta
                .as_ref()
                .and_then(|meta| meta.err.clone());
            let index = index as u32;
            let transaction = transaction_with_meta
                .transaction
                .decode()
                .expect("transaction decode failed");
            let signature = transaction.signatures[0];

            for address in transaction.message.account_keys {
                if !is_sysvar_id(&address) {
                    by_addr
                        .entry(address)
                        .or_default()
                        .push(TransactionByAddrInfo {
                            signature,
                            err: err.clone(),
                            index,
                            memo: None, // TODO
                        });
                }
            }

            tx_cells.push((
                signature.to_string(),
                TransactionInfo {
                    slot,
                    index,
                    err,
                    memo: None, // TODO
                },
            ));
        }

        let tx_by_addr_cells: Vec<_> = by_addr
            .into_iter()
            .map(|(address, transaction_info_by_addr)| {
                (
                    format!("{}/{}", address, slot_to_key(!slot)),
                    transaction_info_by_addr,
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
                .put_bincode_cells_with_retry::<Vec<TransactionByAddrInfo>>(
                    "tx-by-addr",
                    &tx_by_addr_cells,
                )
                .await?;
        }

        let num_transactions = confirmed_block.transactions.len();

        // Store the block itself last, after all other metadata about the block has been
        // successfully stored.  This avoids partial uploaded blocks from becoming visible to
        // `get_confirmed_block()` and `get_confirmed_blocks()`
        let blocks_cells = [(slot_to_key(slot), confirmed_block.try_into()?)];
        bytes_written += self
            .connection
            .put_bincode_cells_with_retry::<StoredConfirmedBlock>("blocks", &blocks_cells)
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
