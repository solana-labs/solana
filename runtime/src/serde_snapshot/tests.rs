#![cfg(test)]

use {
    super::*,
    crate::{
        account_storage::{AccountStorageMap, AccountStorageReference},
        accounts::Accounts,
        accounts_db::{
            get_temp_accounts_paths, test_utils::create_test_accounts, AccountShrinkThreshold,
        },
        accounts_file::{AccountsFile, AccountsFileError},
        accounts_hash::AccountsHash,
        snapshot_utils::get_storages_to_serialize,
    },
    bincode::serialize_into,
    rand::{thread_rng, Rng},
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        clock::Slot,
        genesis_config::ClusterType,
        hash::Hash,
        pubkey::Pubkey,
    },
    std::{
        io::{BufReader, Cursor},
        ops::RangeFull,
        path::Path,
        sync::Arc,
    },
    tempfile::TempDir,
};

/// Simulates the unpacking & storage reconstruction done during snapshot unpacking
fn copy_append_vecs<P: AsRef<Path>>(
    accounts_db: &AccountsDb,
    output_dir: P,
) -> Result<StorageAndNextAppendVecId, AccountsFileError> {
    let storage_entries = accounts_db.get_snapshot_storages(RangeFull).0;
    let storage: AccountStorageMap = AccountStorageMap::with_capacity(storage_entries.len());
    let mut next_append_vec_id = 0;
    for storage_entry in storage_entries.into_iter() {
        // Copy file to new directory
        let storage_path = storage_entry.get_path();
        let file_name =
            AccountsFile::file_name(storage_entry.slot(), storage_entry.append_vec_id());
        let output_path = output_dir.as_ref().join(file_name);
        std::fs::copy(storage_path, &output_path)?;

        // Read new file into append-vec and build new entry
        let (accounts_file, num_accounts) =
            AccountsFile::new_from_file(output_path, storage_entry.accounts.len())?;
        let new_storage_entry = AccountStorageEntry::new_existing(
            storage_entry.slot(),
            storage_entry.append_vec_id(),
            accounts_file,
            num_accounts,
        );
        next_append_vec_id = next_append_vec_id.max(new_storage_entry.append_vec_id());
        storage.insert(
            new_storage_entry.slot(),
            AccountStorageReference {
                id: new_storage_entry.append_vec_id(),
                storage: Arc::new(new_storage_entry),
            },
        );
    }

    Ok(StorageAndNextAppendVecId {
        storage,
        next_append_vec_id: AtomicAppendVecId::new(next_append_vec_id + 1),
    })
}

fn check_accounts(accounts: &Accounts, pubkeys: &[Pubkey], num: usize) {
    for _ in 1..num {
        let idx = thread_rng().gen_range(0, num - 1);
        let ancestors = vec![(0, 0)].into_iter().collect();
        let account = accounts.load_without_fixed_root(&ancestors, &pubkeys[idx]);
        let account1 = Some((
            AccountSharedData::new((idx + 1) as u64, 0, AccountSharedData::default().owner()),
            0,
        ));
        assert_eq!(account, account1);
    }
}

fn context_accountsdb_from_stream<'a, C, R>(
    stream: &mut BufReader<R>,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
) -> Result<AccountsDb, Error>
where
    C: TypeContext<'a>,
    R: Read,
{
    // read and deserialise the accounts database directly from the stream
    let accounts_db_fields = C::deserialize_accounts_db_fields(stream)?;
    let snapshot_accounts_db_fields = SnapshotAccountsDbFields {
        full_snapshot_accounts_db_fields: accounts_db_fields,
        incremental_snapshot_accounts_db_fields: None,
    };
    reconstruct_accountsdb_from_fields(
        snapshot_accounts_db_fields,
        account_paths,
        storage_and_next_append_vec_id,
        &GenesisConfig {
            cluster_type: ClusterType::Development,
            ..GenesisConfig::default()
        },
        AccountSecondaryIndexes::default(),
        None,
        AccountShrinkThreshold::default(),
        false,
        Some(crate::accounts_db::ACCOUNTS_DB_CONFIG_FOR_TESTING),
        None,
        Arc::default(),
        None,
        (u64::default(), None),
        None,
    )
    .map(|(accounts_db, _)| accounts_db)
}

fn accountsdb_from_stream<R>(
    serde_style: SerdeStyle,
    stream: &mut BufReader<R>,
    account_paths: &[PathBuf],
    storage_and_next_append_vec_id: StorageAndNextAppendVecId,
) -> Result<AccountsDb, Error>
where
    R: Read,
{
    match serde_style {
        SerdeStyle::Newer => context_accountsdb_from_stream::<newer::Context, R>(
            stream,
            account_paths,
            storage_and_next_append_vec_id,
        ),
    }
}

fn accountsdb_to_stream<W>(
    serde_style: SerdeStyle,
    stream: &mut W,
    accounts_db: &AccountsDb,
    slot: Slot,
    account_storage_entries: &[Vec<Arc<AccountStorageEntry>>],
) -> Result<(), Error>
where
    W: Write,
{
    match serde_style {
        SerdeStyle::Newer => serialize_into(
            stream,
            &SerializableAccountsDb::<newer::Context> {
                accounts_db,
                slot,
                account_storage_entries,
                phantom: std::marker::PhantomData,
            },
        ),
    }
}

fn test_accounts_serialize_style(serde_style: SerdeStyle) {
    solana_logger::setup();
    let (_accounts_dir, paths) = get_temp_accounts_paths(4).unwrap();
    let accounts = Accounts::new_with_config_for_tests(
        paths,
        &ClusterType::Development,
        AccountSecondaryIndexes::default(),
        AccountShrinkThreshold::default(),
    );

    let slot = 0;
    let mut pubkeys: Vec<Pubkey> = vec![];
    create_test_accounts(&accounts, &mut pubkeys, 100, slot);
    check_accounts(&accounts, &pubkeys, 100);
    accounts.add_root(slot);
    let accounts_delta_hash = accounts.accounts_db.calculate_accounts_delta_hash(slot);
    let accounts_hash = AccountsHash(Hash::new_unique());
    accounts
        .accounts_db
        .set_accounts_hash_for_tests(slot, accounts_hash);

    let mut writer = Cursor::new(vec![]);
    accountsdb_to_stream(
        serde_style,
        &mut writer,
        &accounts.accounts_db,
        slot,
        &get_storages_to_serialize(&accounts.accounts_db.get_snapshot_storages(..=slot).0),
    )
    .unwrap();

    let copied_accounts = TempDir::new().unwrap();

    // Simulate obtaining a copy of the AppendVecs from a tarball
    let storage_and_next_append_vec_id =
        copy_append_vecs(&accounts.accounts_db, copied_accounts.path()).unwrap();

    let buf = writer.into_inner();
    let mut reader = BufReader::new(&buf[..]);
    let (_accounts_dir, daccounts_paths) = get_temp_accounts_paths(2).unwrap();
    let daccounts = Accounts::new_empty(
        accountsdb_from_stream(
            serde_style,
            &mut reader,
            &daccounts_paths,
            storage_and_next_append_vec_id,
        )
        .unwrap(),
    );
    check_accounts(&daccounts, &pubkeys, 100);
    let daccounts_delta_hash = daccounts.accounts_db.calculate_accounts_delta_hash(slot);
    assert_eq!(accounts_delta_hash, daccounts_delta_hash);
    let daccounts_hash = daccounts.accounts_db.get_accounts_hash(slot).unwrap().0;
    assert_eq!(accounts_hash, daccounts_hash);
}

pub(crate) fn reconstruct_accounts_db_via_serialization(
    accounts: &AccountsDb,
    slot: Slot,
) -> AccountsDb {
    let mut writer = Cursor::new(vec![]);
    let snapshot_storages = accounts.get_snapshot_storages(..=slot).0;
    accountsdb_to_stream(
        SerdeStyle::Newer,
        &mut writer,
        accounts,
        slot,
        &get_storages_to_serialize(&snapshot_storages),
    )
    .unwrap();

    let buf = writer.into_inner();
    let mut reader = BufReader::new(&buf[..]);
    let copied_accounts = TempDir::new().unwrap();

    // Simulate obtaining a copy of the AppendVecs from a tarball
    let storage_and_next_append_vec_id =
        copy_append_vecs(accounts, copied_accounts.path()).unwrap();
    let mut accounts_db = accountsdb_from_stream(
        SerdeStyle::Newer,
        &mut reader,
        &[],
        storage_and_next_append_vec_id,
    )
    .unwrap();

    // The append vecs will be used from `copied_accounts` directly by the new AccountsDb so keep
    // its TempDir alive
    accounts_db
        .temp_paths
        .as_mut()
        .unwrap()
        .push(copied_accounts);

    accounts_db
}

#[test]
fn test_accounts_serialize_newer() {
    test_accounts_serialize_style(SerdeStyle::Newer)
}
