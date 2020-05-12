use chrono::prelude::*;
use pickledb::{error::Error, PickleDb, PickleDbDumpPolicy};
use serde::{Deserialize, Serialize};
use solana_sdk::{hash::Hash, pubkey::Pubkey, signature::Signature, transaction::Transaction};
use solana_transaction_status::TransactionStatus;
use std::{cmp::Ordering, fs, io, path::Path};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TransactionInfo {
    pub recipient: Pubkey,
    pub amount: f64,
    pub new_stake_account_address: Option<Pubkey>,
    pub finalized_date: Option<DateTime<Utc>>,
    pub transaction: Transaction,
}

#[derive(Serialize, Deserialize, Debug, Default, PartialEq)]
struct SignedTransactionInfo {
    recipient: String,
    amount: f64,
    new_stake_account_address: String,
    finalized_date: Option<DateTime<Utc>>,
    signature: String,
}

impl Default for TransactionInfo {
    fn default() -> Self {
        let mut transaction = Transaction::new_unsigned_instructions(&[]);
        transaction.signatures.push(Signature::default());
        Self {
            recipient: Pubkey::default(),
            amount: 0.0,
            new_stake_account_address: None,
            finalized_date: None,
            transaction,
        }
    }
}

pub fn open_db(path: &str, dry_run: bool) -> Result<PickleDb, Error> {
    let policy = if dry_run {
        PickleDbDumpPolicy::NeverDump
    } else {
        PickleDbDumpPolicy::AutoDump
    };
    let path = Path::new(path);
    let db = if path.exists() {
        PickleDb::load_yaml(path, policy)?
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        PickleDb::new_yaml(path, policy)
    };
    Ok(db)
}

pub fn compare_transaction_infos(a: &TransactionInfo, b: &TransactionInfo) -> Ordering {
    let ordering = match (a.finalized_date, b.finalized_date) {
        (Some(a), Some(b)) => a.cmp(&b),
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less, // Future finalized date will be greater
        _ => Ordering::Equal,
    };
    if ordering == Ordering::Equal {
        return a.recipient.to_string().cmp(&b.recipient.to_string());
    }
    ordering
}

pub fn write_transaction_log<P: AsRef<Path>>(db: &PickleDb, path: &P) -> Result<(), io::Error> {
    let mut wtr = csv::WriterBuilder::new().from_path(path).unwrap();
    let mut transaction_infos = read_transaction_infos(db);
    transaction_infos.sort_by(compare_transaction_infos);
    for info in transaction_infos {
        let signed_info = SignedTransactionInfo {
            recipient: info.recipient.to_string(),
            amount: info.amount,
            new_stake_account_address: info
                .new_stake_account_address
                .map(|x| x.to_string())
                .unwrap_or_else(|| "".to_string()),
            finalized_date: info.finalized_date,
            signature: info.transaction.signatures[0].to_string(),
        };
        wtr.serialize(&signed_info)?;
    }
    wtr.flush()
}

pub fn read_transaction_infos(db: &PickleDb) -> Vec<TransactionInfo> {
    db.iter()
        .map(|kv| kv.get_value::<TransactionInfo>().unwrap())
        .collect()
}

pub fn set_transaction_info(
    db: &mut PickleDb,
    recipient: &Pubkey,
    amount: f64,
    transaction: &Transaction,
    new_stake_account_address: Option<&Pubkey>,
    finalized: bool,
) -> Result<(), Error> {
    let finalized_date = if finalized { Some(Utc::now()) } else { None };
    let transaction_info = TransactionInfo {
        recipient: *recipient,
        amount,
        new_stake_account_address: new_stake_account_address.cloned(),
        finalized_date,
        transaction: transaction.clone(),
    };
    let signature = transaction.signatures[0];
    db.set(&signature.to_string(), &transaction_info)?;
    Ok(())
}

// Set the finalized bit in the database if the transaction is rooted.
// Remove the TransactionInfo from the database if the transaction failed.
// Return the number of confirmations on the transaction or None if finalized.
pub fn update_finalized_transaction(
    db: &mut PickleDb,
    signature: &Signature,
    opt_transaction_status: Option<TransactionStatus>,
    blockhash: &Hash,
    recent_blockhashes: &[Hash],
) -> Result<Option<usize>, Error> {
    if opt_transaction_status.is_none() {
        if !recent_blockhashes.contains(blockhash) {
            eprintln!("Signature not found {} and blockhash expired", signature);
            eprintln!("Discarding transaction record");
            db.rem(&signature.to_string())?;
            return Ok(None);
        }

        // Return zero to signal the transaction may still be in flight.
        return Ok(Some(0));
    }
    let transaction_status = opt_transaction_status.unwrap();

    if let Some(confirmations) = transaction_status.confirmations {
        // The transaction was found but is not yet finalized.
        return Ok(Some(confirmations));
    }

    if let Err(e) = &transaction_status.status {
        // The transaction was finalized, but execution failed. Drop it.
        eprintln!(
            "Error in transaction with signature {}: {}",
            signature,
            e.to_string()
        );
        eprintln!("Discarding transaction record");
        db.rem(&signature.to_string())?;
        return Ok(None);
    }

    // Transaction is rooted. Set finalized in the database.
    let mut transaction_info = db.get::<TransactionInfo>(&signature.to_string()).unwrap();
    transaction_info.finalized_date = Some(Utc::now());
    db.set(&signature.to_string(), &transaction_info)?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use csv::{ReaderBuilder, Trim};
    use solana_sdk::transaction::TransactionError;
    use tempfile::NamedTempFile;

    #[test]
    fn test_sort_transaction_infos_finalized_first() {
        let info0 = TransactionInfo {
            finalized_date: Some(Utc.ymd(2014, 7, 8).and_hms(9, 10, 11)),
            ..TransactionInfo::default()
        };
        let info1 = TransactionInfo {
            finalized_date: Some(Utc.ymd(2014, 7, 8).and_hms(9, 10, 42)),
            ..TransactionInfo::default()
        };
        let info2 = TransactionInfo::default();
        let info3 = TransactionInfo {
            recipient: Pubkey::new_rand(),
            ..TransactionInfo::default()
        };

        // Sorted first by date
        assert_eq!(compare_transaction_infos(&info0, &info1), Ordering::Less);

        // Finalized transactions should be before unfinalized ones
        assert_eq!(compare_transaction_infos(&info1, &info2), Ordering::Less);

        // Then sorted by recipient
        assert_eq!(compare_transaction_infos(&info2, &info3), Ordering::Less);
    }

    #[test]
    fn test_write_transaction_log() {
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();

        let csv_file = NamedTempFile::new().unwrap();
        write_transaction_log(&db, &csv_file).unwrap();

        let mut rdr = ReaderBuilder::new().trim(Trim::All).from_reader(csv_file);
        let signed_infos: Vec<SignedTransactionInfo> =
            rdr.deserialize().map(|entry| entry.unwrap()).collect();

        let signed_info = SignedTransactionInfo {
            recipient: Pubkey::default().to_string(),
            signature: Signature::default().to_string(),
            ..SignedTransactionInfo::default()
        };
        assert_eq!(signed_infos, vec![signed_info]);
    }

    #[test]
    fn test_update_finalized_transaction_not_landed() {
        // Keep waiting for a transaction that hasn't landed yet.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let blockhash = Hash::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        assert!(matches!(
            update_finalized_transaction(&mut db, &signature, None, &blockhash, &[blockhash])
                .unwrap(),
            Some(0)
        ));

        // Unchanged
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );

        // Same as before, but now with an expired blockhash
        assert_eq!(
            update_finalized_transaction(&mut db, &signature, None, &blockhash, &[]).unwrap(),
            None
        );

        // Ensure TransactionInfo has been purged.
        assert_eq!(db.get::<TransactionInfo>(&signature.to_string()), None);
    }

    #[test]
    fn test_update_finalized_transaction_confirming() {
        // Keep waiting for a transaction that is still being confirmed.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let blockhash = Hash::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: Some(1),
            status: Ok(()),
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(
                &mut db,
                &signature,
                Some(transaction_status),
                &blockhash,
                &[blockhash]
            )
            .unwrap(),
            Some(1)
        );

        // Unchanged
        assert_eq!(
            db.get::<TransactionInfo>(&signature.to_string()).unwrap(),
            transaction_info
        );
    }

    #[test]
    fn test_update_finalized_transaction_failed() {
        // Don't wait if the transaction failed to execute.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let blockhash = Hash::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let status = Err(TransactionError::AccountNotFound);
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status,
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(
                &mut db,
                &signature,
                Some(transaction_status),
                &blockhash,
                &[blockhash]
            )
            .unwrap(),
            None
        );

        // Ensure TransactionInfo has been purged.
        assert_eq!(db.get::<TransactionInfo>(&signature.to_string()), None);
    }

    #[test]
    fn test_update_finalized_transaction_finalized() {
        // Don't wait once the transaction has been finalized.
        let mut db =
            PickleDb::new_yaml(NamedTempFile::new().unwrap(), PickleDbDumpPolicy::NeverDump);
        let signature = Signature::default();
        let blockhash = Hash::default();
        let transaction_info = TransactionInfo::default();
        db.set(&signature.to_string(), &transaction_info).unwrap();
        let transaction_status = TransactionStatus {
            slot: 0,
            confirmations: None,
            status: Ok(()),
            err: None,
        };
        assert_eq!(
            update_finalized_transaction(
                &mut db,
                &signature,
                Some(transaction_status),
                &blockhash,
                &[blockhash]
            )
            .unwrap(),
            None
        );

        assert!(db
            .get::<TransactionInfo>(&signature.to_string())
            .unwrap()
            .finalized_date
            .is_some());
    }
}
