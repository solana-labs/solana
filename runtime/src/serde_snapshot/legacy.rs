use super::common::UnusedAccounts;
#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
use solana_sdk::abi_example::IgnoreAsHelper;
use {super::*, bincode::config::Options, solana_measure::measure::Measure, std::cell::RefCell};

const MAX_ACCOUNTS_DB_STREAM_SIZE: u64 = MAX_STREAM_SIZE;

type AccountsDbFields = super::AccountsDbFields<SerializableAccountStorageEntry>;

// Serializable version of AccountStorageEntry for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct SerializableAccountStorageEntry {
    id: AppendVecId,
    accounts: SerializableAppendVec,
    count_and_status: (usize, AccountStorageStatus),
}

#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
impl IgnoreAsHelper for SerializableAccountStorageEntry {}

impl From<&AccountStorageEntry> for SerializableAccountStorageEntry {
    fn from(rhs: &AccountStorageEntry) -> Self {
        Self {
            id: rhs.id,
            accounts: SerializableAppendVec::from(&rhs.accounts),
            ..Self::default()
        }
    }
}

impl Into<AccountStorageEntry> for SerializableAccountStorageEntry {
    fn into(self) -> AccountStorageEntry {
        AccountStorageEntry::new_empty_map(self.id, self.accounts.current_len)
    }
}

// Serializable version of AppendVec for snapshot format
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SerializableAppendVec {
    current_len: usize,
}

#[cfg(all(test, RUSTC_WITH_SPECIALIZATION))]
impl IgnoreAsHelper for SerializableAppendVec {}

impl From<&AppendVec> for SerializableAppendVec {
    fn from(rhs: &AppendVec) -> SerializableAppendVec {
        SerializableAppendVec {
            current_len: rhs.len(),
        }
    }
}

impl Into<AppendVec> for SerializableAppendVec {
    fn into(self) -> AppendVec {
        AppendVec::new_empty_map(self.current_len)
    }
}

// Serialization of AppendVec requires serialization of u64 to
// eight byte vector which is then itself serialized to the stream
impl Serialize for SerializableAppendVec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        const LEN: usize = std::mem::size_of::<usize>();
        let mut buf = [0u8; LEN];
        serialize_into(Cursor::new(&mut buf[..]), &(self.current_len as u64))
            .map_err(serde::ser::Error::custom)?;
        serializer.serialize_bytes(&buf)
    }
}

// Deserialization of AppendVec requires deserialization
// of eight byte vector from which u64 is then deserialized
impl<'de> Deserialize<'de> for SerializableAppendVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        struct SerializableAppendVecVisitor;
        impl<'a> Visitor<'a> for SerializableAppendVecVisitor {
            type Value = SerializableAppendVec;
            fn expecting(&self, formatter: &mut Formatter) -> FormatResult {
                formatter.write_str("Expecting SerializableAppendVec")
            }
            fn visit_bytes<E>(self, data: &[u8]) -> std::result::Result<Self::Value, E>
            where
                E: Error,
            {
                const LEN: u64 = std::mem::size_of::<usize>() as u64;
                let mut rd = Cursor::new(&data[..]);
                let current_len: usize = deserialize_from(&mut rd).map_err(Error::custom)?;
                if rd.position() != LEN {
                    Err(Error::custom("SerializableAppendVec: unexpected length"))
                } else {
                    Ok(SerializableAppendVec { current_len })
                }
            }
        }
        deserializer.deserialize_bytes(SerializableAppendVecVisitor)
    }
}

// Deserializable version of Bank which need not be serializable,
// because it's handled by SerializableVersionedBank.
// So, sync fields with it!
#[derive(Clone, Deserialize)]
pub(crate) struct DeserializableVersionedBank {
    pub(crate) blockhash_queue: BlockhashQueue,
    pub(crate) ancestors: Ancestors,
    pub(crate) hash: Hash,
    pub(crate) parent_hash: Hash,
    pub(crate) parent_slot: Slot,
    pub(crate) hard_forks: HardForks,
    pub(crate) transaction_count: u64,
    pub(crate) tick_height: u64,
    pub(crate) signature_count: u64,
    pub(crate) capitalization: u64,
    pub(crate) max_tick_height: u64,
    pub(crate) hashes_per_tick: Option<u64>,
    pub(crate) ticks_per_slot: u64,
    pub(crate) ns_per_slot: u128,
    pub(crate) genesis_creation_time: UnixTimestamp,
    pub(crate) slots_per_year: f64,
    pub(crate) unused: u64,
    pub(crate) slot: Slot,
    pub(crate) epoch: Epoch,
    pub(crate) block_height: u64,
    pub(crate) collector_id: Pubkey,
    pub(crate) collector_fees: u64,
    pub(crate) fee_calculator: FeeCalculator,
    pub(crate) fee_rate_governor: FeeRateGovernor,
    pub(crate) collected_rent: u64,
    pub(crate) rent_collector: RentCollector,
    pub(crate) epoch_schedule: EpochSchedule,
    pub(crate) inflation: Inflation,
    pub(crate) stakes: Stakes,
    pub(crate) unused_accounts: UnusedAccounts,
    pub(crate) epoch_stakes: HashMap<Epoch, EpochStakes>,
    pub(crate) is_delta: bool,
    pub(crate) message_processor: MessageProcessor,
}

impl Into<BankFieldsToDeserialize> for DeserializableVersionedBank {
    fn into(self) -> BankFieldsToDeserialize {
        BankFieldsToDeserialize {
            blockhash_queue: self.blockhash_queue,
            ancestors: self.ancestors,
            hash: self.hash,
            parent_hash: self.parent_hash,
            parent_slot: self.parent_slot,
            hard_forks: self.hard_forks,
            transaction_count: self.transaction_count,
            tick_height: self.tick_height,
            signature_count: self.signature_count,
            capitalization: self.capitalization,
            max_tick_height: self.max_tick_height,
            hashes_per_tick: self.hashes_per_tick,
            ticks_per_slot: self.ticks_per_slot,
            ns_per_slot: self.ns_per_slot,
            genesis_creation_time: self.genesis_creation_time,
            slots_per_year: self.slots_per_year,
            unused: self.unused,
            slot: self.slot,
            epoch: self.epoch,
            block_height: self.block_height,
            collector_id: self.collector_id,
            collector_fees: self.collector_fees,
            fee_calculator: self.fee_calculator,
            fee_rate_governor: self.fee_rate_governor,
            collected_rent: self.collected_rent,
            rent_collector: self.rent_collector,
            epoch_schedule: self.epoch_schedule,
            inflation: self.inflation,
            stakes: self.stakes,
            epoch_stakes: self.epoch_stakes,
            is_delta: self.is_delta,
        }
    }
}
// Serializable version of Bank, not Deserializable to avoid cloning by using refs.
// Sync fields with DeserializableVersionedBank!
#[derive(Serialize)]
pub(crate) struct SerializableVersionedBank<'a> {
    pub(crate) blockhash_queue: &'a RwLock<BlockhashQueue>,
    pub(crate) ancestors: &'a Ancestors,
    pub(crate) hash: Hash,
    pub(crate) parent_hash: Hash,
    pub(crate) parent_slot: Slot,
    pub(crate) hard_forks: &'a RwLock<HardForks>,
    pub(crate) transaction_count: u64,
    pub(crate) tick_height: u64,
    pub(crate) signature_count: u64,
    pub(crate) capitalization: u64,
    pub(crate) max_tick_height: u64,
    pub(crate) hashes_per_tick: Option<u64>,
    pub(crate) ticks_per_slot: u64,
    pub(crate) ns_per_slot: u128,
    pub(crate) genesis_creation_time: UnixTimestamp,
    pub(crate) slots_per_year: f64,
    pub(crate) unused: u64,
    pub(crate) slot: Slot,
    pub(crate) epoch: Epoch,
    pub(crate) block_height: u64,
    pub(crate) collector_id: Pubkey,
    pub(crate) collector_fees: u64,
    pub(crate) fee_calculator: FeeCalculator,
    pub(crate) fee_rate_governor: FeeRateGovernor,
    pub(crate) collected_rent: u64,
    pub(crate) rent_collector: RentCollector,
    pub(crate) epoch_schedule: EpochSchedule,
    pub(crate) inflation: Inflation,
    pub(crate) stakes: &'a RwLock<Stakes>,
    pub(crate) unused_accounts: UnusedAccounts,
    pub(crate) epoch_stakes: &'a HashMap<Epoch, EpochStakes>,
    pub(crate) is_delta: bool,
    pub(crate) message_processor: MessageProcessor,
}

use std::sync::RwLock;

impl<'a> From<crate::bank::BankFieldsToSerialize<'a>> for SerializableVersionedBank<'a> {
    fn from(rhs: crate::bank::BankFieldsToSerialize<'a>) -> Self {
        fn new<T: Default>() -> T {
            T::default()
        }
        Self {
            blockhash_queue: rhs.blockhash_queue,
            ancestors: rhs.ancestors,
            hash: rhs.hash,
            parent_hash: rhs.parent_hash,
            parent_slot: rhs.parent_slot,
            hard_forks: rhs.hard_forks,
            transaction_count: rhs.transaction_count,
            tick_height: rhs.tick_height,
            signature_count: rhs.signature_count,
            capitalization: rhs.capitalization,
            max_tick_height: rhs.max_tick_height,
            hashes_per_tick: rhs.hashes_per_tick,
            ticks_per_slot: rhs.ticks_per_slot,
            ns_per_slot: rhs.ns_per_slot,
            genesis_creation_time: rhs.genesis_creation_time,
            slots_per_year: rhs.slots_per_year,
            unused: rhs.unused,
            slot: rhs.slot,
            epoch: rhs.epoch,
            block_height: rhs.block_height,
            collector_id: rhs.collector_id,
            collector_fees: rhs.collector_fees,
            fee_calculator: rhs.fee_calculator,
            fee_rate_governor: rhs.fee_rate_governor,
            collected_rent: rhs.collected_rent,
            rent_collector: rhs.rent_collector,
            epoch_schedule: rhs.epoch_schedule,
            inflation: rhs.inflation,
            stakes: rhs.stakes,
            unused_accounts: new(),
            epoch_stakes: rhs.epoch_stakes,
            is_delta: rhs.is_delta,
            message_processor: new(),
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl<'a> IgnoreAsHelper for SerializableVersionedBank<'a> {}

pub(super) struct Context {}
impl<'a> TypeContext<'a> for Context {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

    fn serialize_bank_and_storage<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank_and_storage: &SerializableBankAndStorage<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        (
            SerializableVersionedBank::from(
                serializable_bank_and_storage.bank.get_fields_to_serialize(),
            ),
            SerializableAccountsDB::<'a, Self> {
                accounts_db: &*serializable_bank_and_storage.bank.rc.accounts.accounts_db,
                slot: serializable_bank_and_storage.bank.rc.slot,
                account_storage_entries: serializable_bank_and_storage.snapshot_storages,
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

        let slot_hash = (
            serializable_db.slot,
            serializable_db
                .accounts_db
                .bank_hashes
                .read()
                .unwrap()
                .get(&serializable_db.slot)
                .unwrap_or_else(|| panic!("No bank_hashes entry for slot {}", serializable_db.slot))
                .clone(),
        );

        // as there is no deserialize_bank_rc_fields(), emit two u64 size fields here instead
        let mut serialize_account_storage_timer = Measure::start("serialize_account_storage_ms");
        let result = (
            &MAX_ACCOUNTS_DB_STREAM_SIZE,
            &MAX_ACCOUNTS_DB_STREAM_SIZE,
            &entries,
            &version,
            &slot_hash,
        )
            .serialize(serializer);
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
    ) -> Result<(BankFieldsToDeserialize, AccountsDbFields), Error>
    where
        R: Read,
    {
        let bank_fields = deserialize_from::<_, DeserializableVersionedBank>(&mut stream)?.into();
        let accounts_db_fields = Self::deserialize_accounts_db_fields(stream)?;
        Ok((bank_fields, accounts_db_fields))
    }

    fn deserialize_accounts_db_fields<R>(
        mut stream: &mut BufReader<R>,
    ) -> Result<AccountsDbFields, Error>
    where
        R: Read,
    {
        // read and discard two u64 byte vector lengths
        let serialized_len = MAX_ACCOUNTS_DB_STREAM_SIZE;
        let serialized_len = min(serialized_len, deserialize_from(&mut stream)?);
        let serialized_len = min(serialized_len, deserialize_from(&mut stream)?);

        // (1st of 3 elements) read in map of slots to account storage entries
        let storage: HashMap<Slot, Vec<Self::SerializableAccountStorageEntry>> = bincode::options()
            .with_limit(serialized_len)
            .with_fixint_encoding()
            .allow_trailing_bytes()
            .deserialize_from(&mut stream)?;

        // (2nd of 3 elements) read in write version
        let version: u64 = deserialize_from(&mut stream)?;

        // (3rd of 3 elements) read in (slot, bank hashes) pair
        let (slot, bank_hash_info): (Slot, BankHashInfo) = deserialize_from(&mut stream)?;

        Ok(AccountsDbFields(storage, version, slot, bank_hash_info))
    }
}
