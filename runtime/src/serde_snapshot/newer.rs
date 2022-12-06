use {
    super::{
        storage::SerializableAccountStorageEntry,
        utils::{serialize_iter_as_map, serialize_iter_as_seq},
        *,
    },
    crate::{
        accounts_hash::AccountsHash,
        ancestors::AncestorsForSerialization,
        stakes::{serde_stakes_enum_compat, StakesEnum},
    },
    solana_measure::measure::Measure,
    solana_sdk::{deserialize_utils::ignore_eof_error, stake::state::Delegation},
    std::{cell::RefCell, collections::HashSet, sync::RwLock},
};

pub(super) type AccountsDbFields = super::AccountsDbFields<SerializableAccountStorageEntry>;

#[derive(Default, Clone, PartialEq, Eq, Debug, Deserialize, Serialize, AbiExample)]
struct UnusedAccounts {
    unused1: HashSet<Pubkey>,
    unused2: HashSet<Pubkey>,
    unused3: HashMap<Pubkey, u64>,
}

// Deserializable version of Bank which need not be serializable,
// because it's handled by SerializableVersionedBank.
// So, sync fields with it!
#[derive(Clone, Deserialize)]
struct DeserializableVersionedBank {
    blockhash_queue: BlockhashQueue,
    ancestors: AncestorsForSerialization,
    hash: Hash,
    parent_hash: Hash,
    parent_slot: Slot,
    hard_forks: HardForks,
    transaction_count: u64,
    tick_height: u64,
    signature_count: u64,
    capitalization: u64,
    max_tick_height: u64,
    hashes_per_tick: Option<u64>,
    ticks_per_slot: u64,
    ns_per_slot: u128,
    genesis_creation_time: UnixTimestamp,
    slots_per_year: f64,
    accounts_data_len: u64,
    slot: Slot,
    epoch: Epoch,
    block_height: u64,
    collector_id: Pubkey,
    collector_fees: u64,
    fee_calculator: FeeCalculator,
    fee_rate_governor: FeeRateGovernor,
    collected_rent: u64,
    rent_collector: RentCollector,
    epoch_schedule: EpochSchedule,
    inflation: Inflation,
    stakes: Stakes<Delegation>,
    #[allow(dead_code)]
    unused_accounts: UnusedAccounts,
    epoch_stakes: HashMap<Epoch, EpochStakes>,
    is_delta: bool,
}

impl From<DeserializableVersionedBank> for BankFieldsToDeserialize {
    fn from(dvb: DeserializableVersionedBank) -> Self {
        BankFieldsToDeserialize {
            blockhash_queue: dvb.blockhash_queue,
            ancestors: dvb.ancestors,
            hash: dvb.hash,
            parent_hash: dvb.parent_hash,
            parent_slot: dvb.parent_slot,
            hard_forks: dvb.hard_forks,
            transaction_count: dvb.transaction_count,
            tick_height: dvb.tick_height,
            signature_count: dvb.signature_count,
            capitalization: dvb.capitalization,
            max_tick_height: dvb.max_tick_height,
            hashes_per_tick: dvb.hashes_per_tick,
            ticks_per_slot: dvb.ticks_per_slot,
            ns_per_slot: dvb.ns_per_slot,
            genesis_creation_time: dvb.genesis_creation_time,
            slots_per_year: dvb.slots_per_year,
            accounts_data_len: dvb.accounts_data_len,
            slot: dvb.slot,
            epoch: dvb.epoch,
            block_height: dvb.block_height,
            collector_id: dvb.collector_id,
            collector_fees: dvb.collector_fees,
            fee_calculator: dvb.fee_calculator,
            fee_rate_governor: dvb.fee_rate_governor,
            collected_rent: dvb.collected_rent,
            rent_collector: dvb.rent_collector,
            epoch_schedule: dvb.epoch_schedule,
            inflation: dvb.inflation,
            stakes: dvb.stakes,
            epoch_stakes: dvb.epoch_stakes,
            is_delta: dvb.is_delta,
            incremental_snapshot_persistence: None,
            epoch_accounts_hash: None,
        }
    }
}

// Serializable version of Bank, not Deserializable to avoid cloning by using refs.
// Sync fields with DeserializableVersionedBank!
#[derive(Serialize)]
struct SerializableVersionedBank<'a> {
    blockhash_queue: &'a RwLock<BlockhashQueue>,
    ancestors: &'a AncestorsForSerialization,
    hash: Hash,
    parent_hash: Hash,
    parent_slot: Slot,
    hard_forks: &'a RwLock<HardForks>,
    transaction_count: u64,
    tick_height: u64,
    signature_count: u64,
    capitalization: u64,
    max_tick_height: u64,
    hashes_per_tick: Option<u64>,
    ticks_per_slot: u64,
    ns_per_slot: u128,
    genesis_creation_time: UnixTimestamp,
    slots_per_year: f64,
    accounts_data_len: u64,
    slot: Slot,
    epoch: Epoch,
    block_height: u64,
    collector_id: Pubkey,
    collector_fees: u64,
    fee_calculator: FeeCalculator,
    fee_rate_governor: FeeRateGovernor,
    collected_rent: u64,
    rent_collector: RentCollector,
    epoch_schedule: EpochSchedule,
    inflation: Inflation,
    #[serde(serialize_with = "serde_stakes_enum_compat::serialize")]
    stakes: StakesEnum,
    unused_accounts: UnusedAccounts,
    epoch_stakes: &'a HashMap<Epoch, EpochStakes>,
    is_delta: bool,
}

impl<'a> From<crate::bank::BankFieldsToSerialize<'a>> for SerializableVersionedBank<'a> {
    fn from(rhs: crate::bank::BankFieldsToSerialize<'a>) -> Self {
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
            accounts_data_len: rhs.accounts_data_len,
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
            stakes: StakesEnum::from(rhs.stakes.stakes().clone()),
            unused_accounts: UnusedAccounts::default(),
            epoch_stakes: rhs.epoch_stakes,
            is_delta: rhs.is_delta,
        }
    }
}

#[cfg(RUSTC_WITH_SPECIALIZATION)]
impl<'a> solana_frozen_abi::abi_example::IgnoreAsHelper for SerializableVersionedBank<'a> {}

#[derive(PartialEq, Eq)]
pub(super) struct Context {}

impl<'a> TypeContext<'a> for Context {
    type SerializableAccountStorageEntry = SerializableAccountStorageEntry;

    fn serialize_bank_and_storage<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankAndStorage<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        let ancestors = HashMap::from(&serializable_bank.bank.ancestors);
        let fields = serializable_bank.bank.get_fields_to_serialize(&ancestors);
        let lamports_per_signature = fields.fee_rate_governor.lamports_per_signature;
        (
            SerializableVersionedBank::from(fields),
            SerializableAccountsDb::<'a, Self> {
                accounts_db: &serializable_bank.bank.rc.accounts.accounts_db,
                slot: serializable_bank.bank.rc.slot,
                account_storage_entries: serializable_bank.snapshot_storages,
                phantom: std::marker::PhantomData::default(),
            },
            // Additional fields, we manually store the lamps per signature here so that
            // we can grab it on restart.
            // TODO: if we do a snapshot version bump, consider moving this out.
            lamports_per_signature,
            None::<BankIncrementalSnapshotPersistence>,
            serializable_bank
                .bank
                .get_epoch_accounts_hash_to_serialize()
                .map(|epoch_accounts_hash| *epoch_accounts_hash.as_ref()),
        )
            .serialize(serializer)
    }

    #[cfg(test)]
    fn serialize_bank_and_storage_without_extra_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_bank: &SerializableBankAndStorageNoExtra<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        let ancestors = HashMap::from(&serializable_bank.bank.ancestors);
        let fields = serializable_bank.bank.get_fields_to_serialize(&ancestors);
        (
            SerializableVersionedBank::from(fields),
            SerializableAccountsDb::<'a, Self> {
                accounts_db: &serializable_bank.bank.rc.accounts.accounts_db,
                slot: serializable_bank.bank.rc.slot,
                account_storage_entries: serializable_bank.snapshot_storages,
                phantom: std::marker::PhantomData::default(),
            },
        )
            .serialize(serializer)
    }

    fn serialize_accounts_db_fields<S: serde::ser::Serializer>(
        serializer: S,
        serializable_db: &SerializableAccountsDb<'a, Self>,
    ) -> std::result::Result<S::Ok, S::Error>
    where
        Self: std::marker::Sized,
    {
        // sample write version before serializing storage entries
        let version = serializable_db
            .accounts_db
            .write_version
            .load(Ordering::Acquire);

        // (1st of 3 elements) write the list of account storage entry lists out as a map
        let entry_count = RefCell::<usize>::new(0);
        let entries =
            serialize_iter_as_map(serializable_db.account_storage_entries.iter().map(|x| {
                *entry_count.borrow_mut() += x.len();
                (
                    x.first().unwrap().slot(),
                    serialize_iter_as_seq(
                        x.iter()
                            .map(|x| Self::SerializableAccountStorageEntry::from(x.as_ref())),
                    ),
                )
            }));
        let slot = serializable_db.slot;
        let bank_hash_info: BankHashInfo = serializable_db
            .accounts_db
            .bank_hashes
            .read()
            .unwrap()
            .get(&serializable_db.slot)
            .unwrap_or_else(|| panic!("No bank_hashes entry for slot {}", serializable_db.slot))
            .clone();

        let historical_roots = serializable_db
            .accounts_db
            .accounts_index
            .roots_tracker
            .read()
            .unwrap()
            .historical_roots
            .get_all();
        let historical_roots_with_hash = Vec::<(Slot, Hash)>::default();

        let mut serialize_account_storage_timer = Measure::start("serialize_account_storage_ms");
        let result = (
            entries,
            version,
            slot,
            bank_hash_info,
            historical_roots,
            historical_roots_with_hash,
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
        let mut bank_fields: BankFieldsToDeserialize =
            deserialize_from::<_, DeserializableVersionedBank>(&mut stream)?.into();
        let accounts_db_fields = Self::deserialize_accounts_db_fields(stream)?;
        // Process extra fields
        let lamports_per_signature = ignore_eof_error(deserialize_from(&mut stream))?;
        bank_fields.fee_rate_governor = bank_fields
            .fee_rate_governor
            .clone_with_lamports_per_signature(lamports_per_signature);

        let incremental_snapshot_persistence = ignore_eof_error(deserialize_from(&mut stream))?;
        bank_fields.incremental_snapshot_persistence = incremental_snapshot_persistence;

        let epoch_accounts_hash = ignore_eof_error(deserialize_from(stream))?;
        bank_fields.epoch_accounts_hash = epoch_accounts_hash;

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

    /// deserialize the bank from 'stream_reader'
    /// modify the accounts_hash and incremental_snapshot_persistence
    /// reserialize the bank to 'stream_writer'
    fn reserialize_bank_fields_with_hash<R, W>(
        stream_reader: &mut BufReader<R>,
        stream_writer: &mut BufWriter<W>,
        accounts_hash: &AccountsHash,
        incremental_snapshot_persistence: Option<&BankIncrementalSnapshotPersistence>,
    ) -> std::result::Result<(), Box<bincode::ErrorKind>>
    where
        R: Read,
        W: Write,
    {
        let (bank_fields, mut accounts_db_fields) =
            Self::deserialize_bank_fields(stream_reader).unwrap();
        accounts_db_fields.3.accounts_hash = *accounts_hash;
        let mut rhs = bank_fields;
        let blockhash_queue = RwLock::new(std::mem::take(&mut rhs.blockhash_queue));
        let hard_forks = RwLock::new(std::mem::take(&mut rhs.hard_forks));
        let lamports_per_signature = rhs.fee_rate_governor.lamports_per_signature;
        let epoch_accounts_hash = rhs.epoch_accounts_hash.as_ref();

        let bank = SerializableVersionedBank {
            blockhash_queue: &blockhash_queue,
            ancestors: &rhs.ancestors,
            hash: rhs.hash,
            parent_hash: rhs.parent_hash,
            parent_slot: rhs.parent_slot,
            hard_forks: &hard_forks,
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
            accounts_data_len: rhs.accounts_data_len,
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
            stakes: StakesEnum::from(rhs.stakes),
            unused_accounts: UnusedAccounts::default(),
            epoch_stakes: &rhs.epoch_stakes,
            is_delta: rhs.is_delta,
        };

        bincode::serialize_into(
            stream_writer,
            &(
                bank,
                accounts_db_fields,
                lamports_per_signature,
                incremental_snapshot_persistence,
                epoch_accounts_hash,
            ),
        )
    }
}
