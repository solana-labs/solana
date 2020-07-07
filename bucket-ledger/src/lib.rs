pub mod bucket;
pub mod compression;

use crate::compression::{compress_best, decompress};
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
    ConfirmedBlock, ConfirmedTransaction, EncodedTransaction, Rewards, RpcTransactionStatusMeta,
    TransactionEncoding, TransactionStatus, TransactionWithStatusMeta,
};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("BucketError: {0}")]
    BucketError(bucket::Error),

    #[error("I/O Error: {0}")]
    IoError(std::io::Error),

    #[error("Object {0} is corrupt")]
    ObjectCorrupt(/*key*/ String),

    #[error("Transaction encoded is not supported")]
    UnsupportedTransactionEncoding,
}

impl std::convert::From<bucket::Error> for Error {
    fn from(err: bucket::Error) -> Self {
        Self::BucketError(err)
    }
}

impl std::convert::From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

// A serialized `BucketBlock` is stored in `block/<slot>` bucket objects
//
// BucketBlock holds the same contents as ConfirmedBlock, but is slightly compressed and avoids
// some serde JSON directives that cause issues with bincode
//
#[derive(Serialize, Deserialize)]
struct BucketBlock {
    previous_blockhash: String,
    blockhash: String,
    parent_slot: Slot,
    transactions: Vec<BucketBlockTransaction>,
    rewards: Rewards,
    block_time: Option<UnixTimestamp>,
}

impl BucketBlock {
    fn into_confirmed_block(self, encoding: TransactionEncoding) -> ConfirmedBlock {
        let BucketBlock {
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

impl TryFrom<ConfirmedBlock> for BucketBlock {
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
struct BucketBlockTransaction {
    transaction: Transaction,
    meta: Option<BucketBlockTransactionStatusMeta>,
}

impl BucketBlockTransaction {
    fn into_transaction_with_status_meta(
        self,
        encoding: TransactionEncoding,
    ) -> TransactionWithStatusMeta {
        let BucketBlockTransaction { transaction, meta } = self;
        TransactionWithStatusMeta {
            transaction: EncodedTransaction::encode(transaction, encoding),
            meta: meta.map(|meta| meta.into()),
        }
    }
}

impl TryFrom<TransactionWithStatusMeta> for BucketBlockTransaction {
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
struct BucketBlockTransactionStatusMeta {
    err: Option<TransactionError>,
    fee: u64,
    pre_balances: Vec<u64>,
    post_balances: Vec<u64>,
}

impl From<BucketBlockTransactionStatusMeta> for RpcTransactionStatusMeta {
    fn from(value: BucketBlockTransactionStatusMeta) -> Self {
        let BucketBlockTransactionStatusMeta {
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

impl From<RpcTransactionStatusMeta> for BucketBlockTransactionStatusMeta {
    fn from(value: RpcTransactionStatusMeta) -> Self {
        let RpcTransactionStatusMeta {
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

// A serialized `BucketTransactionInfo` is stored in `tx/<signature>` bucket objects
#[derive(Serialize, Deserialize)]
struct BucketTransactionInfo {
    slot: Slot, // The slot that contains the block with this transaction in it
    index: u32, // Where the transaction is located in the block
    err: Option<TransactionError>, // None if the transaction executed successfully
}

impl From<BucketTransactionInfo> for TransactionStatus {
    fn from(transaction_info: BucketTransactionInfo) -> Self {
        let BucketTransactionInfo { slot, err, .. } = transaction_info;
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

// A serialized `Vec<BucketTransactionByAddrInfo>` is stored in
// `tx-by-addr/<address>/<one's-compliment-slot>` bucket objects
#[derive(Serialize, Deserialize)]
struct BucketTransactionByAddrInfo {
    signature: Signature,          // The transaction signature
    err: Option<TransactionError>, // None if the transaction executed successfully
    index: u32,                    // Where the transaction is located in the block
}

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

async fn get_transaction_info(
    bucket: &bucket::Bucket,
    signature: &Signature,
) -> Result<BucketTransactionInfo> {
    let key = format!("tx/{}", signature);
    let data = bucket.get_object(&key).await?;
    bincode::deserialize(&data).map_err(|err| {
        warn!("Failed to deserialize {}: {}", key, err);
        Error::ObjectCorrupt(key)
    })
}

async fn put_transaction_info(
    bucket: &bucket::Bucket,
    signature: &Signature,
    transaction_info: BucketTransactionInfo,
) -> Result<usize> {
    let data = bincode::serialize(&transaction_info).unwrap();
    bucket
        .put_object(format!("tx/{}", signature), &data)
        .await?;
    Ok(data.len())
}

async fn put_transaction_by_addr_info(
    bucket: &bucket::Bucket,
    address: &Pubkey,
    slot: Slot,
    by_addr_info: &[BucketTransactionByAddrInfo],
) -> Result<usize> {
    let data = bincode::serialize(&by_addr_info).unwrap();
    bucket
        .put_object(
            format!("tx-by-addr/{}/{}", address, slot_to_key(!slot)),
            &data,
        )
        .await?;
    Ok(data.len())
}

async fn get_bucket_block(bucket: &bucket::Bucket, slot: Slot) -> Result<BucketBlock> {
    let key = format!("block/{}", slot_to_key(slot));
    let data = bucket.get_object(&key).await?;
    let data = decompress(&data)?;
    bincode::deserialize(&data).map_err(|err| {
        warn!("Failed to deserialize {}: {}", key, err);
        Error::ObjectCorrupt(key)
    })
}

async fn put_bucket_block(
    bucket: &bucket::Bucket,
    slot: Slot,
    bucket_block: BucketBlock,
) -> Result<usize> {
    let data = compress_best(&bincode::serialize(&bucket_block).unwrap())?;
    bucket
        .put_object(format!("block/{}", slot_to_key(slot)), &data)
        .await?;
    Ok(data.len())
}

pub struct BucketLedger {
    bucket: bucket::Bucket,
}

impl BucketLedger {
    pub fn new(s3_bucket_name: &str, s3_region: &str) -> Result<Self> {
        let bucket = bucket::Bucket::new(s3_bucket_name, s3_region)?;
        Ok(Self { bucket })
    }

    /// Return the available slot that contains a block
    pub async fn get_first_available_block(&self) -> Result<Option<Slot>> {
        let blocks = self.bucket.object_keys("block/", None, Some(1)).await?;

        if blocks.is_empty() {
            return Ok(None);
        }
        Ok(key_to_slot(&blocks[0][7..]))
    }

    /// Fetch the next `bucket::MAX_LIST_KEYS` slots after the provided slot that contains a block
    ///
    /// start_slot: slot to start the search from (inclusive)
    /// limit: stop after this many slots have been found.  Values are between 1 and
    /// `MAX_LIST_KEYS`, default is `MAX_LIST_KEYS`
    pub async fn get_confirmed_blocks(
        &self,
        start_slot: Slot,
        limit: Option<usize>, // 1..MAX_LIST_KEYS, default MAX_LIST_KEYS
    ) -> Result<Vec<Slot>> {
        let start_after = if start_slot > 0 {
            Some(format!("block/{}", slot_to_key(start_slot - 1)))
        } else {
            None
        };

        let blocks = self
            .bucket
            .object_keys("block/", start_after, limit)
            .await?;
        Ok(blocks
            .into_iter()
            .filter_map(|key| key_to_slot(&key[7..]))
            .collect())
    }

    /// Fetch the confirmed block from the desired slot
    pub async fn get_confirmed_block(
        &self,
        slot: Slot,
        encoding: TransactionEncoding,
    ) -> Result<ConfirmedBlock> {
        let bucket_block = get_bucket_block(&self.bucket, slot).await?;
        Ok(bucket_block.into_confirmed_block(encoding))
    }

    /// Fetch a confirmed transaction
    pub async fn get_confirmed_transaction(
        &self,
        signature: &Signature,
        encoding: TransactionEncoding,
    ) -> Result<Option<ConfirmedTransaction>> {
        // Figure out which block the transaction is located in
        let BucketTransactionInfo { slot, index, .. } =
            get_transaction_info(&self.bucket, signature).await?;

        // Load the block and return the transaction
        let bucket_block = get_bucket_block(&self.bucket, slot).await?;
        match bucket_block.transactions.into_iter().nth(index as usize) {
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

    pub async fn get_transaction_status(&self, signature: &Signature) -> Result<TransactionStatus> {
        let transaction_info = get_transaction_info(&self.bucket, signature).await?;
        Ok(transaction_info.into())
    }

    /// Get confirmed signatures for the provided address, in descending ledger order
    ///
    /// address: address to search for
    /// start_after_signature: start with the first signature older than this one
    /// limit: stop after this many signatures.  Values are between 1 and `MAX_LIST_KEYS`, default
    /// is `MAX_LIST_KEYS`
    pub async fn get_confirmed_signatures_for_address(
        &self,
        address: &Pubkey,
        start_after_signature: Option<&Signature>,
        limit: Option<usize>, // 1..MAX_LIST_KEYS, default MAX_LIST_KEYS
    ) -> Result<Vec<(Signature, Slot, Option<TransactionError>)>> {
        let limit = std::cmp::min(
            bucket::MAX_LIST_KEYS,
            limit.unwrap_or(bucket::MAX_LIST_KEYS),
        );

        // Figure out where to start listing from, based on `start_after_signature`
        let prefix = format!("tx-by-addr/{}/", address);
        let (start_after_key, mut first_transaction_index) = match start_after_signature {
            None => (None, 0),
            Some(start_after_signature) => {
                let BucketTransactionInfo { slot, index, .. } =
                    get_transaction_info(&self.bucket, &start_after_signature).await?;

                if slot == 0 {
                    (None, 0)
                } else {
                    (Some(format!("{}{}", prefix, slot_to_key(!slot))), index + 1)
                }
            }
        };

        let mut infos = vec![];

        // Return the next `limit` by-addr keys
        let tx_by_addr_info_keys = self
            .bucket
            .object_keys(&prefix, start_after_key, Some(limit))
            .await?;

        // Read each by-addr object until `limit` signatures have been found
        'outer: for key in &tx_by_addr_info_keys {
            let slot = !key_to_slot(&key[prefix.len()..]).ok_or_else(|| {
                warn!("Failed to convert key to slot: {}", key);
                Error::ObjectCorrupt(key.to_string())
            })?;

            let tx_by_addr_infos: Vec<BucketTransactionByAddrInfo> =
                bincode::deserialize(&self.bucket.get_object(key).await?).map_err(|err| {
                    warn!("Failed to deserialize {}: {}", key, err);
                    Error::ObjectCorrupt(key.to_string())
                })?;

            for tx_by_addr_info in tx_by_addr_infos
                .into_iter()
                .skip(first_transaction_index as usize)
            {
                infos.push((tx_by_addr_info.signature, slot, tx_by_addr_info.err));
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

        let mut by_addr: HashMap<Pubkey, Vec<BucketTransactionByAddrInfo>> = HashMap::new();
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
                        .push(BucketTransactionByAddrInfo {
                            signature,
                            err: err.clone(),
                            index,
                        });
                }
            }
            bytes_written += put_transaction_info(
                &self.bucket,
                &signature,
                BucketTransactionInfo { slot, index, err },
            )
            .await?;
        }

        for (address, transaction_info_by_addr) in by_addr.into_iter() {
            bytes_written += put_transaction_by_addr_info(
                &self.bucket,
                &address,
                slot,
                &transaction_info_by_addr,
            )
            .await?;
        }
        let num_transactions = confirmed_block.transactions.len();

        // Upload the block information last to ensure that `upload_confirmed_block` will only
        // include this new block if all object puts succeed
        let bucket_block = confirmed_block.try_into()?;
        bytes_written += put_bucket_block(&self.bucket, slot, bucket_block).await?;
        info!(
            "uploaded block for slot {}: {} transactions, {} bytes",
            slot, num_transactions, bytes_written
        );

        Ok(())
    }
}
