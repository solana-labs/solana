use super::common::{DeserializableBankFields, SerializableBankFields};
use {super::*, solana_measure::measure::Measure, std::cell::RefCell};

type AccountsDbFields = super::AccountsDbFields<SerializableAccountStorageEntry>;

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct SerializableAccountStorageEntry {
    id: AppendVecId,
    accounts_current_len: usize,
}

#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
impl solana_sdk::abi_example::IgnoreAsHelper for SerializableAccountStorageEntry {}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id,
            accounts_current_len: rhs.accounts.len(),
        }
    }
}

impl Into<AccountStorageEntry> for SerializableAccountStorageEntry {
    fn into(self) -> AccountStorageEntry {
        AccountStorageEntry::new_empty_map(self.id, self.accounts_current_len)
    }
}

pub(super) struct Context {}
impl<'a> TypeContext<'a> for Context {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

    fn serialize_bank_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBank<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        (
            SerializableBankFields::from(serializable_bank.bank.get_ref_fields()),
            SerializableAccountsDB::<'a, Self> {
                accounts_db: &*serializable_bank.bank.rc.accounts.accounts_db,
                slot: serializable_bank.bank.rc.slot,
                account_storage_entries: serializable_bank.snapshot_storages,
                phantom: std::marker::PhantomData::default(),
            },
        )
            .serialize(serializer)
    }

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDB<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        // sample write version before serializing storage entries
        let version = serializable_db
            .accounts_db
            .write_version
            .load(Ordering::Relaxed);

        // (1st of 3 elements) write the list of account storage entry lists out as a map
        let entry_count = RefCell::<usize>::new(0);
        let entries =
            serialize_iter_as_map(serializable_db.account_storage_entries.iter().map(|x| {
                *entry_count.borrow_mut() += x.len();
                (
                    x.first().unwrap().slot,
                    serialize_iter_as_seq(
                        x.iter()
                            .map(|x| Self::SerializableAccountStorageEntry::from(x.as_ref())),
                    ),
                )
            }));
        let slot = serializable_db.slot;
        let hash = serializable_db
            .accounts_db
            .bank_hashes
            .read()
            .unwrap()
            .get(&serializable_db.slot)
            .unwrap_or_else(|| panic!("No bank_hashes entry for slot {}", serializable_db.slot))
            .clone();

        let mut serialize_account_storage_timer = Measure::start("serialize_account_storage_ms");
        let result = (entries, version, slot, hash).serialize(serializer);
        serialize_account_storage_timer.stop();
        datapoint_info!(
            "serialize_account_storage_ms",
            ("duration", serialize_account_storage_timer.as_ms(), i64),
            ("num_entries", *entry_count.borrow(), i64),
        );
        result
    }

    fn deserialize_bank_fields<R>(
        mut stream: &mut BufReader<R>,
    ) -> Result<(BankFields, AccountsDbFields), Error>
    where
        R: Read,
    {
        let bank_fields = deserialize_from::<_, DeserializableBankFields>(&mut stream)?.into();
        let accounts_db_fields = Self::deserialize_accounts_db_fields(stream)?;
        Ok((bank_fields, accounts_db_fields))
    }

    fn deserialize_accounts_db_fields<R>(
        stream: &mut BufReader<R>,
    ) -> Result<AccountsDbFields, Error>
    where
        R: Read,
    {
        deserialize_from(stream)
    }
}
