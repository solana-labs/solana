use {
    super::*,
    crate::entry_verification_service::{shared_wrapper::SharedWrapper, Results},
    solana_entry::entry::create_ticks,
    solana_ledger::{
        blockstore::{make_many_slot_entries, make_slot_entries, Blockstore, CompletedDataSetInfo},
        get_tmp_ledger_path_auto_delete,
        shred::{ProcessShredsStats, ReedSolomonCache, Shredder},
    },
    solana_sdk::signature::Keypair,
    std::{
        panic,
        time::{Duration, Instant},
    },
};

/// Describes a single data set in a slot, holding its shreds and entries.
///
/// Tests have to deal with data sets a lot.  And in particular, in the assertion code, there is a
/// lot of repetition related to generating derived values from a data set.  Combining this data in
/// a single type, and providing a number of transformation functions, removes a lot of the
/// syntactic noise from the tests.
struct DataSetDetails {
    slot: u64,
    start_index: u32,
    end_index: u32,
    last_in_slot: bool,
    entries: Arc<Vec<Entry>>,
    shreds: Vec<Shred>,
}

impl DataSetDetails {
    fn first_shred_index(&self) -> DataSetFirstShred {
        self.start_index.into()
    }

    fn covered_shred_indices(&self) -> RangeInclusive<u32> {
        self.start_index..=self.end_index
    }

    fn entries(&self) -> Arc<Vec<Entry>> {
        self.entries.clone()
    }

    fn first_entry(&self) -> Entry {
        self.entries.first().unwrap().clone()
    }

    fn last_entry_hash(&self) -> Hash {
        self.entries.last().unwrap().hash
    }

    fn shreds(&self) -> Vec<Shred> {
        self.shreds.clone()
    }

    fn to_last_data_set(&self) -> LastSlotDataSet {
        LastSlotDataSet {
            index: self.first_shred_index(),
            last_entry_hash: self.last_entry_hash(),
        }
    }

    fn as_todo(&self) -> DataSetAsTodoItem<'_> {
        DataSetAsTodoItem(self)
    }

    fn as_status(&self) -> DataSetAsDataSetOptimisticStatus<'_> {
        DataSetAsDataSetOptimisticStatus(self)
    }

    fn to_completed_data_set_info(&self) -> CompletedDataSetInfo {
        let Self {
            slot,
            start_index,
            end_index,
            last_in_slot,
            ..
        } = self;

        CompletedDataSetInfo {
            slot: *slot,
            start_index: *start_index,
            end_index: *end_index,
            last_in_slot: *last_in_slot,
        }
    }
}

struct DataSetAsTodoItem<'a>(&'a DataSetDetails);

impl<'a> DataSetAsTodoItem<'a> {
    fn all(&self, preceding: &DataSetDetails) -> TodoItem {
        let DataSetDetails { slot, entries, .. } = self.0;

        let poh_start_hash = preceding.last_entry_hash();

        TodoItem {
            slot: *slot,
            op: TodoOp::All {
                poh_start_hash,
                entries: entries.clone(),
            },
        }
    }

    fn all_but_poh_on_first_entry(&self) -> TodoItem {
        let DataSetDetails { slot, entries, .. } = self.0;

        TodoItem {
            slot: *slot,
            op: TodoOp::AllButPohOnFirstEntry {
                entries: entries.clone(),
            },
        }
    }

    fn only_first_entry_poh(&self, preceding: &DataSetDetails) -> TodoItem {
        let DataSetDetails { slot, .. } = self.0;

        let poh_start_hash = preceding.last_entry_hash();

        TodoItem {
            slot: *slot,
            op: TodoOp::OnlyFirstEntryPoh {
                poh_start_hash,
                entry: self.0.first_entry(),
            },
        }
    }
}

struct DataSetAsDataSetOptimisticStatus<'a>(&'a DataSetDetails);

impl<'a> DataSetAsDataSetOptimisticStatus<'a> {
    /// Creates a `DataSetAsDataSetOptimisticStatus::PohStartHash` instance for a data set that
    /// succeeds the current one in the same slot.
    fn same_slot_succeeding_poh_start_hash(&self) -> (DataSetFirstShred, DataSetOptimisticStatus) {
        let DataSetDetails { end_index, .. } = self.0;

        let poh_start_hash = self.0.last_entry_hash();
        (
            DataSetFirstShred::from(*end_index + 1),
            DataSetOptimisticStatus::PohStartHash(poh_start_hash),
        )
    }

    fn all_but_first(&self) -> (DataSetFirstShred, DataSetOptimisticStatus) {
        let DataSetDetails {
            start_index,
            end_index,
            ..
        } = self.0;

        let next_data_set = (end_index + 1).into();
        (
            DataSetFirstShred::from(*start_index),
            DataSetOptimisticStatus::AllButFirst {
                next_data_set,
                first_entry: self.0.first_entry(),
            },
        )
    }

    fn verified(&self) -> (DataSetFirstShred, DataSetOptimisticStatus) {
        let DataSetDetails {
            start_index,
            end_index,
            ..
        } = self.0;

        let next_data_set = (end_index + 1).into();
        (
            DataSetFirstShred::from(*start_index),
            DataSetOptimisticStatus::Verified { next_data_set },
        )
    }
}

/// Creates a sequence of data sets, with entries that contain just tick entries, covering a single
/// slot.
fn make_slot_data_sets(
    poh_start_hash: Option<Hash>,
    slot: Slot,
    parent_slot: Slot,
    num_data_sets: u64,
    num_entries_per_data_set: u64,
) -> Vec<DataSetDetails> {
    let shredder = Shredder::new(slot, parent_slot, 0, 0).expect("Can create a shredder");

    let keypair = Keypair::new();
    let mut next_shred_index = 0;

    let mut hash = poh_start_hash.unwrap_or_else(|| Hash::new_unique());
    (0..num_data_sets)
        .map(|data_set_i| {
            let entries = create_ticks(num_entries_per_data_set, 1, hash);
            let last_in_slot = data_set_i + 1 == num_data_sets;
            let (data_shreds, _code_shreds) = shredder.entries_to_shreds(
                &keypair,
                &entries,
                last_in_slot,
                next_shred_index,
                0,
                true,
                &ReedSolomonCache::default(),
                &mut ProcessShredsStats::default(),
            );

            let start_index = next_shred_index;

            next_shred_index += u32::try_from(data_shreds.len()).unwrap();
            hash = entries.last().unwrap().hash;

            let end_index = next_shred_index - 1;

            DataSetDetails {
                slot,
                start_index,
                end_index,
                last_in_slot,
                entries: Arc::new(entries),
                shreds: data_shreds,
            }
        })
        .collect()
}

/// A number of tests operate on slots with a single data set.
///
/// This is a helper that removes syntactic noise from a call to `make_slot_data_sets()`,
/// specialized for the one data set case.
fn make_slot_with_one_data_set(
    poh_start_hash: Option<Hash>,
    slot: Slot,
    parent_slot: Slot,
    num_entries_per_data_set: u64,
) -> DataSetDetails {
    let res = make_slot_data_sets(
        poh_start_hash,
        slot,
        parent_slot,
        1,
        num_entries_per_data_set,
    );
    assert_eq!(res.len(), 1);
    res.into_iter().next().unwrap()
}

/// Calls `make_slot_with_one_data_set()`, using a fresh starting hash for the PoH chain.
fn make_isolated_slot_with_one_data_set(
    slot: Slot,
    parent_slot: Slot,
    num_entries_per_data_set: u64,
) -> DataSetDetails {
    make_slot_with_one_data_set(None, slot, parent_slot, num_entries_per_data_set)
}

/// A number of tests operate on slots with just to data sets.
///
/// This is a helper that removes syntactic noise from a call to `make_slot_data_sets()`,
/// specialized for the two data set case.
fn make_slot_with_two_data_sets(
    poh_start_hash: Option<Hash>,
    slot: Slot,
    parent_slot: Slot,
    num_entries_per_data_set: u64,
) -> (DataSetDetails, DataSetDetails) {
    let mut res = make_slot_data_sets(
        poh_start_hash,
        slot,
        parent_slot,
        2,
        num_entries_per_data_set,
    );
    assert_eq!(res.len(), 2);
    let second = res.pop().unwrap();
    let first = res.pop().unwrap();
    (first, second)
}

/// Calls `make_slot_with_two_data_sets()`, using a fresh starting hash for the PoH chain.
fn make_isolated_slot_with_two_data_sets(
    slot: Slot,
    parent_slot: Slot,
    num_entries_per_data_set: u64,
) -> (DataSetDetails, DataSetDetails) {
    make_slot_with_two_data_sets(None, slot, parent_slot, num_entries_per_data_set)
}

mod add_data_set {
    use {
        super::*,
        pretty_assertions_sorted::{assert_eq, assert_eq_sorted},
    };

    #[test]
    // Insert two data set, both within the same slot, sequentially.
    // First insert data set X, next insert data set X+1.
    fn single_slot_sequential() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let mut todo = vec![];
        let mut processing_slots = HashMap::new();
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();
        let slot = 7;

        let entries_per_data_set = 100;
        let (first_data_set, second_data_set) =
            make_isolated_slot_with_two_data_sets(slot, slot - 1, entries_per_data_set);

        // Insert the first data set shreds into the blockstore.  `add_data_set()` will ask for the
        // parent slot, using those shreds.

        let completed_data_sets = blockstore
            .insert_shreds(first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![first_data_set.to_completed_data_set_info()]
        );

        // Insert the first data set...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            slot,
            first_data_set.covered_shred_indices(),
            false,
            first_data_set.entries(),
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            res,
            Some(slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );
        assert_eq!(
            todo,
            vec![first_data_set.as_todo().all_but_poh_on_first_entry()]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        first_data_set.as_status().all_but_first(),
                        first_data_set
                            .as_status()
                            .same_slot_succeeding_poh_start_hash(),
                    ]),
                }
            )])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::from([(6, vec![7])]));

        // Insert the second data set shreds into the blockstore.  `add_data_set()` will ask for the
        // parent slot, using those shreds.

        let completed_data_sets = blockstore
            .insert_shreds(second_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![second_data_set.to_completed_data_set_info()],
        );

        // Insert the second data set...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            slot,
            second_data_set.covered_shred_indices(),
            true,
            second_data_set.entries(),
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            res,
            Some(slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );
        assert_eq!(
            todo,
            vec![
                first_data_set.as_todo().all_but_poh_on_first_entry(),
                second_data_set.as_todo().all(&first_data_set),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: Some(second_data_set.to_last_data_set()),
                    data_sets: HashMap::from([
                        first_data_set.as_status().all_but_first(),
                        second_data_set.as_status().verified(),
                    ]),
                }
            )])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(slot - 1, vec![slot])])
        );
    }

    #[test]
    // Insert two data set, both within the same slot, out of order.
    // First insert data set X + 1, next insert data set X.
    fn single_slot_reversed() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let mut todo = vec![];
        let mut processing_slots = HashMap::new();
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();
        let slot = 7;

        let entries_per_data_set = 100;
        let (first_data_set, second_data_set) =
            make_isolated_slot_with_two_data_sets(slot, slot - 1, entries_per_data_set);

        // Insert the second data set shreds into the blockstore.  `add_data_set()` will ask for the
        // parent slot, using those shreds.

        let completed_data_sets = blockstore
            .insert_shreds(second_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        // Data sets are marked completed only when the preceding data set was received.  As it
        // contains the [`ShredFlags::DATA_COMPLETE_SHRED`] flag, to indicate the boundary.
        assert_eq!(completed_data_sets, vec![]);

        // Insert the second data set...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            slot,
            second_data_set.covered_shred_indices(),
            true,
            second_data_set.entries(),
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            res,
            Some(slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );
        assert_eq!(
            todo,
            vec![second_data_set.as_todo().all_but_poh_on_first_entry()],
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: Some(second_data_set.to_last_data_set()),
                    data_sets: HashMap::from([second_data_set.as_status().all_but_first()]),
                }
            )])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());

        // Insert the first data set shreds into the blockstore.  `add_data_set()` will ask for the
        // parent slot, using those shreds.

        let completed_data_sets = blockstore
            .insert_shreds(first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![
                first_data_set.to_completed_data_set_info(),
                second_data_set.to_completed_data_set_info(),
            ]
        );

        // Insert the first data set...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            slot,
            first_data_set.covered_shred_indices(),
            false,
            first_data_set.entries(),
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            res,
            Some(slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );
        assert_eq!(
            todo,
            vec![
                second_data_set.as_todo().all_but_poh_on_first_entry(),
                first_data_set.as_todo().all_but_poh_on_first_entry(),
                second_data_set
                    .as_todo()
                    .only_first_entry_poh(&first_data_set),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: Some(second_data_set.to_last_data_set()),
                    data_sets: HashMap::from([
                        first_data_set.as_status().all_but_first(),
                        second_data_set.as_status().verified(),
                    ]),
                }
            )])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(slot - 1, vec![slot])])
        );
    }

    #[test]
    // First populate the parent slot with two data sets, then insert the first data set into the
    // child slot.
    //
    // Two data sets in the parent ensure that the second parent data set is recognized as
    // completed.  Effectively, this is the scenario tested by `single_slot_sequential()`.
    fn two_slots_sequential() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let mut todo = vec![];
        let mut processing_slots = HashMap::new();
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();

        let parent_slot = 3;
        let child_slot = 7;

        let entries_per_data_set = 100;

        // Parent slot is populated following exactly the same steps as in the
        // `single_slot_sequential()` test.
        let (parent_slot_first_data_set, parent_slot_second_data_set) = {
            let (first_data_set, second_data_set) = make_isolated_slot_with_two_data_sets(
                parent_slot,
                parent_slot - 1,
                entries_per_data_set,
            );

            let completed_data_sets = blockstore
                .insert_shreds(first_data_set.shreds(), None, true)
                .expect("Shred insertion succeeds");

            assert_eq!(
                completed_data_sets,
                vec![first_data_set.to_completed_data_set_info()]
            );

            let res = add_data_set(
                &mut todo,
                &blockstore,
                &mut processing_slots,
                &mut waiting_for_parent_hash,
                &results,
                parent_slot,
                first_data_set.covered_shred_indices(),
                false,
                first_data_set.entries(),
            );
            assert_eq!(
                res,
                Some(parent_slot - 1),
                "`add_data_set()` must return the parent slot, on success."
            );

            let completed_data_sets = blockstore
                .insert_shreds(second_data_set.shreds(), None, true)
                .expect("Shred insertion succeeds");
            assert_eq!(
                completed_data_sets,
                vec![second_data_set.to_completed_data_set_info()]
            );

            let res = add_data_set(
                &mut todo,
                &blockstore,
                &mut processing_slots,
                &mut waiting_for_parent_hash,
                &results,
                parent_slot,
                second_data_set.covered_shred_indices(),
                true,
                second_data_set.entries(),
            );
            assert_eq!(
                res,
                Some(parent_slot - 1),
                "`add_data_set()` must return the parent slot, on success."
            );

            (first_data_set, second_data_set)
        };

        // Check initial state.

        assert_eq!(
            todo,
            vec![
                parent_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                parent_slot_second_data_set
                    .as_todo()
                    .all(&parent_slot_first_data_set),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                parent_slot,
                SlotStatus::Processing {
                    parent_slot: parent_slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: Some(parent_slot_second_data_set.to_last_data_set()),
                    data_sets: HashMap::from([
                        parent_slot_first_data_set.as_status().all_but_first(),
                        parent_slot_second_data_set.as_status().verified(),
                    ]),
                }
            )])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot - 1, vec![parent_slot])])
        );

        // Child slot.

        let (child_slot_first_data_set, _) = make_slot_with_two_data_sets(
            Some(parent_slot_second_data_set.last_entry_hash()),
            child_slot,
            parent_slot,
            entries_per_data_set,
        );

        let completed_data_sets = blockstore
            .insert_shreds(child_slot_first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![child_slot_first_data_set.to_completed_data_set_info()]
        );

        // Insert the first data set into the child slot...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            child_slot,
            child_slot_first_data_set.covered_shred_indices(),
            false,
            child_slot_first_data_set.entries(),
        );

        assert_eq!(
            res,
            Some(parent_slot),
            "`add_data_set()` must return the parent slot, on success."
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            todo,
            vec![
                parent_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                parent_slot_second_data_set
                    .as_todo()
                    .all(&parent_slot_first_data_set),
                child_slot_first_data_set
                    .as_todo()
                    .all(&parent_slot_second_data_set),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    parent_slot,
                    SlotStatus::Processing {
                        parent_slot: parent_slot - 1,
                        verified_up_to: 0.into(),
                        last_data_set: Some(parent_slot_second_data_set.to_last_data_set()),
                        data_sets: HashMap::from([
                            parent_slot_first_data_set.as_status().all_but_first(),
                            parent_slot_second_data_set.as_status().verified(),
                        ]),
                    }
                ),
                (
                    child_slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            child_slot_first_data_set.as_status().verified(),
                            child_slot_first_data_set
                                .as_status()
                                .same_slot_succeeding_poh_start_hash(),
                        ]),
                    }
                ),
            ])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot - 1, vec![parent_slot])])
        );
    }

    #[test]
    // First insert the first data set into the child slot.  Then insert two data sets into the
    // parent slot.
    //
    // Two data sets in the parent ensure that the second parent data set is recognized as
    // completed.
    fn two_slots_reversed() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let mut todo = vec![];
        let mut processing_slots = HashMap::new();
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();

        let parent_slot = 3;
        let child_slot = 7;

        let entries_per_data_set = 100;

        let (parent_slot_first_data_set, parent_slot_second_data_set) =
            make_isolated_slot_with_two_data_sets(
                parent_slot,
                parent_slot - 1,
                entries_per_data_set,
            );

        let (child_slot_first_data_set, _) = make_slot_with_two_data_sets(
            Some(parent_slot_second_data_set.last_entry_hash()),
            child_slot,
            parent_slot,
            entries_per_data_set,
        );

        // Child slot.

        let completed_data_sets = blockstore
            .insert_shreds(child_slot_first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![child_slot_first_data_set.to_completed_data_set_info()]
        );

        // Insert the first data set into the child slot...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            child_slot,
            child_slot_first_data_set.covered_shred_indices(),
            false,
            child_slot_first_data_set.entries(),
        );

        assert_eq!(
            res,
            Some(parent_slot),
            "`add_data_set()` must return the parent slot, on success."
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            todo,
            vec![child_slot_first_data_set
                .as_todo()
                .all_but_poh_on_first_entry()]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                child_slot,
                SlotStatus::Processing {
                    parent_slot,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        child_slot_first_data_set.as_status().all_but_first(),
                        child_slot_first_data_set
                            .as_status()
                            .same_slot_succeeding_poh_start_hash(),
                    ]),
                }
            )])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot, vec![child_slot])])
        );

        // Now the parent slot.
        //
        // We are not going to check state between the two `add_data_set()` calls, as this is done
        // in the `single_slot_sequential()` test.

        let completed_data_sets = blockstore
            .insert_shreds(parent_slot_first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");

        assert_eq!(
            completed_data_sets,
            vec![parent_slot_first_data_set.to_completed_data_set_info()]
        );

        let completed_data_sets = blockstore
            .insert_shreds(parent_slot_second_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![parent_slot_second_data_set.to_completed_data_set_info()]
        );

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            parent_slot,
            parent_slot_first_data_set.covered_shred_indices(),
            false,
            parent_slot_first_data_set.entries(),
        );
        assert_eq!(
            res,
            Some(parent_slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            parent_slot,
            parent_slot_second_data_set.covered_shred_indices(),
            true,
            parent_slot_second_data_set.entries(),
        );
        assert_eq!(
            res,
            Some(parent_slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            todo,
            vec![
                child_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                parent_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                parent_slot_second_data_set
                    .as_todo()
                    .all(&parent_slot_first_data_set),
                child_slot_first_data_set
                    .as_todo()
                    .only_first_entry_poh(&parent_slot_second_data_set),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    parent_slot,
                    SlotStatus::Processing {
                        parent_slot: parent_slot - 1,
                        verified_up_to: 0.into(),
                        last_data_set: Some(parent_slot_second_data_set.to_last_data_set()),
                        data_sets: HashMap::from([
                            parent_slot_first_data_set.as_status().all_but_first(),
                            parent_slot_second_data_set.as_status().verified(),
                        ]),
                    }
                ),
                (
                    child_slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            child_slot_first_data_set.as_status().verified(),
                            child_slot_first_data_set
                                .as_status()
                                .same_slot_succeeding_poh_start_hash(),
                        ]),
                    }
                ),
            ])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot - 1, vec![parent_slot])])
        );
    }

    #[test]
    // First insert the first data set into two child slots.  Then insert two data sets into the
    // parent slot.
    //
    // Two data sets in the parent ensure that the second parent data set is recognized as
    // completed.
    fn two_child_slots_reversed() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let mut todo = vec![];
        let mut processing_slots = HashMap::new();
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();

        let parent_slot = 3;
        let child1_slot = 7;
        let child2_slot = 11;

        let entries_per_data_set = 100;

        let (parent_slot_first_data_set, parent_slot_second_data_set) =
            make_isolated_slot_with_two_data_sets(
                parent_slot,
                parent_slot - 1,
                entries_per_data_set,
            );

        let (child1_slot_first_data_set, _) = make_slot_with_two_data_sets(
            Some(parent_slot_second_data_set.last_entry_hash()),
            child1_slot,
            parent_slot,
            entries_per_data_set,
        );

        let (child2_slot_first_data_set, _) = make_slot_with_two_data_sets(
            Some(parent_slot_second_data_set.last_entry_hash()),
            child2_slot,
            parent_slot,
            entries_per_data_set,
        );

        // First child slot.

        let completed_data_sets = blockstore
            .insert_shreds(child1_slot_first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![child1_slot_first_data_set.to_completed_data_set_info()]
        );

        // Insert the first data set into the child slot...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            child1_slot,
            child1_slot_first_data_set.covered_shred_indices(),
            false,
            child1_slot_first_data_set.entries(),
        );

        assert_eq!(
            res,
            Some(parent_slot),
            "`add_data_set()` must return the parent slot, on success."
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            todo,
            vec![child1_slot_first_data_set
                .as_todo()
                .all_but_poh_on_first_entry()]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                child1_slot,
                SlotStatus::Processing {
                    parent_slot,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([
                        child1_slot_first_data_set.as_status().all_but_first(),
                        child1_slot_first_data_set
                            .as_status()
                            .same_slot_succeeding_poh_start_hash(),
                    ]),
                }
            )])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot, vec![child1_slot])])
        );

        // Second child slot.

        let completed_data_sets = blockstore
            .insert_shreds(child2_slot_first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![child2_slot_first_data_set.to_completed_data_set_info()]
        );

        // Insert the first data set into the child slot...

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            child2_slot,
            child2_slot_first_data_set.covered_shred_indices(),
            false,
            child2_slot_first_data_set.entries(),
        );

        assert_eq!(
            res,
            Some(parent_slot),
            "`add_data_set()` must return the parent slot, on success."
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            todo,
            vec![
                child1_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                child2_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    child1_slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            child1_slot_first_data_set.as_status().all_but_first(),
                            child1_slot_first_data_set
                                .as_status()
                                .same_slot_succeeding_poh_start_hash(),
                        ]),
                    }
                ),
                (
                    child2_slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            child2_slot_first_data_set.as_status().all_but_first(),
                            child2_slot_first_data_set
                                .as_status()
                                .same_slot_succeeding_poh_start_hash(),
                        ]),
                    }
                ),
            ])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot, vec![child1_slot, child2_slot])])
        );

        // Now the parent slot.
        //
        // We are not going to check state between the two `add_data_set()` calls, as this is done
        // in the `single_slot_sequential()` test.

        let completed_data_sets = blockstore
            .insert_shreds(parent_slot_first_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");

        assert_eq!(
            completed_data_sets,
            vec![parent_slot_first_data_set.to_completed_data_set_info()]
        );

        let completed_data_sets = blockstore
            .insert_shreds(parent_slot_second_data_set.shreds(), None, true)
            .expect("Shred insertion succeeds");
        assert_eq!(
            completed_data_sets,
            vec![parent_slot_second_data_set.to_completed_data_set_info()]
        );

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            parent_slot,
            parent_slot_first_data_set.covered_shred_indices(),
            false,
            parent_slot_first_data_set.entries(),
        );
        assert_eq!(
            res,
            Some(parent_slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );

        let res = add_data_set(
            &mut todo,
            &blockstore,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            parent_slot,
            parent_slot_second_data_set.covered_shred_indices(),
            true,
            parent_slot_second_data_set.entries(),
        );
        assert_eq!(
            res,
            Some(parent_slot - 1),
            "`add_data_set()` must return the parent slot, on success."
        );

        // ... and check that all the state was updated correctly.

        assert_eq!(
            todo,
            vec![
                child1_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                child2_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                parent_slot_first_data_set
                    .as_todo()
                    .all_but_poh_on_first_entry(),
                parent_slot_second_data_set
                    .as_todo()
                    .all(&parent_slot_first_data_set),
                child1_slot_first_data_set
                    .as_todo()
                    .only_first_entry_poh(&parent_slot_second_data_set),
                child2_slot_first_data_set
                    .as_todo()
                    .only_first_entry_poh(&parent_slot_second_data_set),
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    parent_slot,
                    SlotStatus::Processing {
                        parent_slot: parent_slot - 1,
                        verified_up_to: 0.into(),
                        last_data_set: Some(parent_slot_second_data_set.to_last_data_set()),
                        data_sets: HashMap::from([
                            parent_slot_first_data_set.as_status().all_but_first(),
                            parent_slot_second_data_set.as_status().verified(),
                        ]),
                    }
                ),
                (
                    child1_slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            child1_slot_first_data_set.as_status().verified(),
                            child1_slot_first_data_set
                                .as_status()
                                .same_slot_succeeding_poh_start_hash(),
                        ]),
                    }
                ),
                (
                    child2_slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([
                            child2_slot_first_data_set.as_status().verified(),
                            child2_slot_first_data_set
                                .as_status()
                                .same_slot_succeeding_poh_start_hash(),
                        ]),
                    }
                ),
            ])
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::from([(parent_slot - 1, vec![parent_slot])])
        );
    }
}

mod waiting_for {
    use {super::*, pretty_assertions_sorted::assert_eq};

    #[test]
    fn empty_todo() {
        let todo = vec![];
        let mut client_waiting_for = None;
        let slot = 3;
        let data_set_start = 0.into();

        let res = waiting_for(&todo, &mut client_waiting_for, slot, data_set_start);
        assert_eq!(res, AccumulationDecision::CanWaitMore);
        assert_eq!(client_waiting_for, Some((slot, data_set_start)));
    }

    #[test]
    fn non_empty_todo() {
        let todo = vec![TodoItem {
            slot: 2,
            op: TodoOp::All {
                poh_start_hash: Hash::new_unique(),
                entries: Arc::default(),
            },
        }];
        let mut client_waiting_for = None;
        let slot = 3;
        let data_set_start = 0.into();

        let res = waiting_for(&todo, &mut client_waiting_for, slot, data_set_start);
        assert_eq!(res, AccumulationDecision::RunVerification);
        assert_eq!(client_waiting_for, Some((slot, data_set_start)));
    }

    #[test]
    fn keep_the_highest_data_set_index() {
        let todo = vec![TodoItem {
            slot: 2,
            op: TodoOp::All {
                poh_start_hash: Hash::new_unique(),
                entries: Arc::default(),
            },
        }];
        let slot = 3;
        let previous_data_set_start = 5.into();
        let data_set_start = 0.into();
        let mut client_waiting_for = Some((slot, previous_data_set_start));

        let res = waiting_for(&todo, &mut client_waiting_for, slot, data_set_start);
        assert_eq!(res, AccumulationDecision::CanWaitMore);
        assert_eq!(client_waiting_for, Some((slot, previous_data_set_start)));
    }

    #[test]
    fn new_slot_updates_data_set_start_regardless() {
        let todo = vec![TodoItem {
            slot: 2,
            op: TodoOp::All {
                poh_start_hash: Hash::new_unique(),
                entries: Arc::default(),
            },
        }];
        let previous_slot = 3;
        let previous_data_set_start = 5.into();
        let slot = 4;
        let data_set_start = 0.into();
        let mut client_waiting_for = Some((previous_slot, previous_data_set_start));

        let res = waiting_for(&todo, &mut client_waiting_for, slot, data_set_start);
        assert_eq!(res, AccumulationDecision::RunVerification);
        assert_eq!(client_waiting_for, Some((slot, data_set_start)));
    }
}

mod cleanup {
    use {
        super::*,
        pretty_assertions_sorted::{assert_eq, assert_eq_sorted},
    };

    #[test]
    fn simple() {
        let slot4_last_entry_hash = Hash::new_unique();
        let mut processing_slots = HashMap::from([
            (
                3,
                SlotStatus::Processing {
                    parent_slot: 2,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        11.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 17.into(),
                        },
                    )]),
                },
            ),
            (
                4,
                SlotStatus::Processing {
                    parent_slot: 3,
                    verified_up_to: 9.into(),
                    last_data_set: Some(LastSlotDataSet {
                        index: 9.into(),
                        last_entry_hash: slot4_last_entry_hash,
                    }),
                    data_sets: HashMap::from([(
                        9.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 13.into(),
                        },
                    )]),
                },
            ),
        ]);
        let mut waiting_for_parent_hash = HashMap::from([(2, vec![3]), (3, vec![4]), (4, vec![5])]);
        let mut client_waiting_for = Some((3, 7.into()));
        let results = SharedResults::new(Results {
            starting_at: 2,
            slots: HashMap::from([
                (3, SlotVerificationStatus::Valid),
                (
                    4,
                    SlotVerificationStatus::Partial {
                        verified_up_to: 4.into(),
                    },
                ),
            ]),
        });

        cleanup(
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &mut client_waiting_for,
            &results,
            4,
        );

        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                4,
                SlotStatus::Processing {
                    parent_slot: 3,
                    verified_up_to: 9.into(),
                    last_data_set: Some(LastSlotDataSet {
                        index: 9.into(),
                        last_entry_hash: slot4_last_entry_hash,
                    }),
                    data_sets: HashMap::from([(
                        9.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 13.into(),
                        },
                    )]),
                },
            )])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::from([(4, vec![5])]));
        assert_eq!(client_waiting_for, None);
        assert_non_empty_results_in_millis!(
            0,
            results,
            Results {
                starting_at: 4,
                slots: HashMap::from([(
                    4,
                    SlotVerificationStatus::Partial {
                        verified_up_to: 4.into()
                    }
                ),]),
            }
        );
    }

    #[test]
    fn noop() {
        let slot4_last_entry_hash = Hash::new_unique();
        let mut processing_slots = HashMap::from([(
            4,
            SlotStatus::Processing {
                parent_slot: 3,
                verified_up_to: 9.into(),
                last_data_set: Some(LastSlotDataSet {
                    index: 9.into(),
                    last_entry_hash: slot4_last_entry_hash,
                }),
                data_sets: HashMap::from([(
                    9.into(),
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 13.into(),
                    },
                )]),
            },
        )]);
        let mut waiting_for_parent_hash = HashMap::from([(4, vec![5])]);
        let mut client_waiting_for = Some((4, 13.into()));
        let results = SharedResults::new(Results {
            starting_at: 4,
            slots: HashMap::from([(
                4,
                SlotVerificationStatus::Partial {
                    verified_up_to: 4.into(),
                },
            )]),
        });

        cleanup(
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &mut client_waiting_for,
            &results,
            4,
        );

        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                4,
                SlotStatus::Processing {
                    parent_slot: 3,
                    verified_up_to: 9.into(),
                    last_data_set: Some(LastSlotDataSet {
                        index: 9.into(),
                        last_entry_hash: slot4_last_entry_hash,
                    }),
                    data_sets: HashMap::from([(
                        9.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 13.into(),
                        },
                    )]),
                },
            )])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::from([(4, vec![5])]));
        assert_eq!(client_waiting_for, Some((4, 13.into())));
        assert_non_empty_results_in_millis!(
            0,
            results,
            Results {
                starting_at: 4,
                slots: HashMap::from([(
                    4,
                    SlotVerificationStatus::Partial {
                        verified_up_to: 4.into()
                    }
                )]),
            }
        );
    }
}

mod get_or_insert_slot_for {
    // `get_or_insert_slot_for()` is just forwarding to
    // `get_or_insert_slot_for_first_slot_data_set()` or
    // `get_or_insert_slot_for_non_first_slot_data_set()`.  And as both are tested below, testing
    // `get_or_insert_slot_for()` itself seems like a low value effort.
    //
    // The amount of duplication with the tests for the two forwarded to functions will be very
    // high.  Just to check that a single `if` statement is there.
}

mod get_or_insert_slot_for_first_slot_data_set {
    use {
        super::*,
        pretty_assertions_sorted::{assert_eq, assert_eq_sorted},
    };

    #[test]
    fn absent_parent_absent_target() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let slot = 6;
        let last_in_slot = true;
        let mut processing_slots = HashMap::new();

        // Our blockstore must contain shreds for the first data set, as we need to be able to get
        // the parent slot.  This is tested in the `get_parent_slot()` tests.
        let entries_per_slot = 100;
        let data_set = make_isolated_slot_with_one_data_set(slot, slot - 1, entries_per_slot);

        blockstore
            .insert_shreds(data_set.shreds(), None, true)
            .unwrap();

        let (slot_status, parent_slot_last_entry_hash) = get_or_insert_slot_for_first_slot_data_set(
            &blockstore,
            &mut processing_slots,
            slot,
            last_in_slot.then(|| data_set.last_entry_hash()),
        );

        assert_eq_sorted!(
            slot_status,
            Some(&mut SlotStatus::Processing {
                parent_slot: slot - 1,
                verified_up_to: 0.into(),
                last_data_set: Some(data_set.to_last_data_set()),
                data_sets: HashMap::new(),
            })
        );
        assert_eq!(parent_slot_last_entry_hash, None);
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: Some(data_set.to_last_data_set()),
                    data_sets: HashMap::new(),
                }
            )])
        );
    }

    #[test]
    fn present_parent_absent_target() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let parent_slot = 7;
        let slot = 8;
        let last_in_slot = false;
        let parent_slot_last_entry_hash = Hash::new_unique();
        let mut processing_slots = HashMap::from([(
            parent_slot,
            SlotStatus::Processing {
                parent_slot: parent_slot - 1,
                verified_up_to: 11.into(),
                last_data_set: Some(LastSlotDataSet {
                    index: 11.into(),
                    last_entry_hash: parent_slot_last_entry_hash,
                }),
                data_sets: HashMap::from([(
                    11.into(),
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 17.into(),
                    },
                )]),
            },
        )]);

        // Our blockstore must contain shreds for the first data set, as we need to be able to get
        // the parent slot.  This is tested in the `get_parent_slot()` tests.
        let num_slots = 1;
        let entries_per_slot = 100;
        let (shreds, entries) = make_many_slot_entries(slot, num_slots, entries_per_slot);
        let last_entry_hash = entries.last().unwrap().hash;

        blockstore.insert_shreds(shreds, None, true).unwrap();

        let (slot_status, retrieved_parent_slot_last_entry_hash) =
            get_or_insert_slot_for_first_slot_data_set(
                &blockstore,
                &mut processing_slots,
                slot,
                last_in_slot.then(|| last_entry_hash),
            );

        assert_eq!(
            slot_status,
            Some(&mut SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::new(),
            })
        );
        assert_eq!(
            retrieved_parent_slot_last_entry_hash,
            Some(parent_slot_last_entry_hash)
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    parent_slot,
                    SlotStatus::Processing {
                        parent_slot: parent_slot - 1,
                        verified_up_to: 11.into(),
                        last_data_set: Some(LastSlotDataSet {
                            index: 11.into(),
                            last_entry_hash: parent_slot_last_entry_hash,
                        }),
                        data_sets: HashMap::from([(
                            11.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 17.into(),
                            }
                        )])
                    },
                ),
                (
                    slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::new(),
                    }
                )
            ])
        );
    }

    #[test]
    fn absent_parent_present_target() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let parent_slot = 7;
        let slot = 8;
        let last_in_slot = true;
        let last_entry_hash = Hash::new_unique();
        let mut processing_slots = HashMap::from([(
            slot,
            SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::from([(
                    11.into(),
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 17.into(),
                    },
                )]),
            },
        )]);

        let (slot_status, parent_slot_last_entry_hash) = get_or_insert_slot_for_first_slot_data_set(
            &blockstore,
            &mut processing_slots,
            slot,
            last_in_slot.then(|| last_entry_hash),
        );

        assert_eq!(
            slot_status,
            Some(&mut SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::from([(
                    11.into(),
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 17.into(),
                    },
                )]),
            })
        );
        assert_eq!(parent_slot_last_entry_hash, None);
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        11.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 17.into(),
                        },
                    )]),
                }
            )])
        );
    }

    #[test]
    fn present_parent_present_target() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let parent_slot = 7;
        let slot = 8;
        let parent_slot_last_entry_hash = Hash::new_unique();
        let last_entry_hash = Hash::new_unique();
        let last_in_slot = false;
        let mut processing_slots = HashMap::from([
            (
                parent_slot,
                SlotStatus::Processing {
                    parent_slot: parent_slot - 1,
                    verified_up_to: 11.into(),
                    last_data_set: Some(LastSlotDataSet {
                        index: 11.into(),
                        last_entry_hash: parent_slot_last_entry_hash,
                    }),
                    data_sets: HashMap::from([(
                        11.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 17.into(),
                        },
                    )]),
                },
            ),
            (
                slot,
                SlotStatus::Processing {
                    parent_slot,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        17.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 17.into(),
                        },
                    )]),
                },
            ),
        ]);

        let (slot_status, retrieved_parent_slot_last_entry_hash) =
            get_or_insert_slot_for_first_slot_data_set(
                &blockstore,
                &mut processing_slots,
                slot,
                last_in_slot.then(|| last_entry_hash),
            );

        assert_eq!(
            slot_status,
            Some(&mut SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::from([(
                    17.into(),
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 17.into(),
                    },
                )]),
            })
        );
        assert_eq!(
            retrieved_parent_slot_last_entry_hash,
            Some(parent_slot_last_entry_hash)
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    parent_slot,
                    SlotStatus::Processing {
                        parent_slot: parent_slot - 1,
                        verified_up_to: 11.into(),
                        last_data_set: Some(LastSlotDataSet {
                            index: 11.into(),
                            last_entry_hash: parent_slot_last_entry_hash,
                        }),
                        data_sets: HashMap::from([(
                            11.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 17.into(),
                            },
                        )]),
                    },
                ),
                (
                    slot,
                    SlotStatus::Processing {
                        parent_slot,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([(
                            17.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 17.into(),
                            },
                        )]),
                    },
                )
            ])
        );
    }
}

mod get_or_insert_slot_for_non_first_slot_data_set {
    use {
        super::*,
        pretty_assertions_sorted::{assert_eq, assert_eq_sorted},
    };

    #[test]
    fn absent() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let slot = 6;
        let data_set_start = DataSetFirstShred::from(11);
        let last_in_slot = true;
        let mut processing_slots = HashMap::new();

        // Our blockstore must contain shreds for the first data set, as we need to be able to get
        // the parent slot.  This is tested in the `get_parent_slot()` tests.
        let entries_per_slot = 300;
        let (shreds, entries) = make_slot_entries(slot, slot - 1, entries_per_slot, true);
        assert!(
            shreds.len() > u32::from(data_set_start) as usize,
            "We need to have enough data shreds, so that `data_set_start` is in there.\n\
             Got: {}\n\
             Need: {}",
            shreds.len(),
            data_set_start
        );
        let last_entry_hash = entries.last().unwrap().hash;

        blockstore.insert_shreds(shreds, None, true).unwrap();

        let slot_status = get_or_insert_slot_for_non_first_slot_data_set(
            &blockstore,
            &mut processing_slots,
            slot,
            data_set_start,
            last_in_slot.then(|| last_entry_hash),
        );

        assert_eq!(
            slot_status,
            Some(&mut SlotStatus::Processing {
                parent_slot: slot - 1,
                verified_up_to: 0.into(),
                last_data_set: Some(LastSlotDataSet {
                    index: data_set_start,
                    last_entry_hash,
                }),
                data_sets: HashMap::new(),
            })
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot: slot - 1,
                    verified_up_to: 0.into(),
                    last_data_set: Some(LastSlotDataSet {
                        index: data_set_start,
                        last_entry_hash,
                    }),
                    data_sets: HashMap::new(),
                }
            )])
        );
    }

    #[test]
    fn present() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();
        let parent_slot = 7;
        let slot = 8;
        let data_set_start = DataSetFirstShred::from(11);
        let last_in_slot = true;
        let last_entry_hash = Hash::new_unique();
        let mut processing_slots = HashMap::from([(
            slot,
            SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set: Some(LastSlotDataSet {
                    index: data_set_start,
                    last_entry_hash,
                }),
                data_sets: HashMap::from([(
                    data_set_start,
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 17.into(),
                    },
                )]),
            },
        )]);

        let slot_status = get_or_insert_slot_for_non_first_slot_data_set(
            &blockstore,
            &mut processing_slots,
            slot,
            data_set_start,
            last_in_slot.then(|| last_entry_hash),
        );

        assert_eq!(
            slot_status,
            Some(&mut SlotStatus::Processing {
                parent_slot,
                verified_up_to: 0.into(),
                last_data_set: Some(LastSlotDataSet {
                    index: data_set_start,
                    last_entry_hash,
                }),
                data_sets: HashMap::from([(
                    11.into(),
                    DataSetOptimisticStatus::Verified {
                        next_data_set: 17.into(),
                    },
                )]),
            })
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                slot,
                SlotStatus::Processing {
                    parent_slot,
                    verified_up_to: 0.into(),
                    last_data_set: Some(LastSlotDataSet {
                        index: data_set_start,
                        last_entry_hash,
                    }),
                    data_sets: HashMap::from([(
                        11.into(),
                        DataSetOptimisticStatus::Verified {
                            next_data_set: 17.into(),
                        },
                    )]),
                }
            )])
        );
    }
}

mod insert_data_set_entries {
    use {
        super::*,
        pretty_assertions_sorted::{assert_eq, assert_eq_sorted},
    };

    #[test]
    fn no_poh_start_hash_slot_first_data_set() {
        let mut todo = vec![];
        let mut waiting_for_parent_hash = HashMap::new();
        let mut data_sets = HashMap::new();
        let entries = Arc::new(create_ticks(10, 0, Hash::new_unique()));
        let first_entry = entries.first().unwrap().clone();
        let next_data_set = 7.into();

        insert_data_set_entries(
            &mut todo,
            &mut waiting_for_parent_hash,
            &mut data_sets,
            3,
            None,
            4,
            0.into(),
            entries.clone(),
            next_data_set,
        );

        assert_eq!(
            todo,
            vec![TodoItem {
                slot: 4,
                op: TodoOp::AllButPohOnFirstEntry { entries }
            }]
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::from([(3, vec![4])]));
        assert_eq_sorted!(
            data_sets,
            HashMap::from([(
                DataSetFirstShred::from(0),
                DataSetOptimisticStatus::AllButFirst {
                    next_data_set,
                    first_entry,
                },
            )])
        );
    }

    #[test]
    fn no_poh_start_hash_slot_second_data_set() {
        let mut todo = vec![];
        let mut waiting_for_parent_hash = HashMap::new();
        let data_set_start = DataSetFirstShred::from(7);
        let mut data_sets = HashMap::new();
        let entries = Arc::new(create_ticks(10, 0, Hash::new_unique()));
        let first_entry = entries.first().unwrap().clone();
        let next_data_set = 7.into();

        insert_data_set_entries(
            &mut todo,
            &mut waiting_for_parent_hash,
            &mut data_sets,
            3,
            None,
            4,
            data_set_start,
            entries.clone(),
            next_data_set,
        );

        assert_eq!(
            todo,
            vec![TodoItem {
                slot: 4,
                op: TodoOp::AllButPohOnFirstEntry { entries }
            }]
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());
        assert_eq_sorted!(
            data_sets,
            HashMap::from([(
                data_set_start,
                DataSetOptimisticStatus::AllButFirst {
                    next_data_set,
                    first_entry,
                },
            )])
        );
    }

    #[test]
    fn with_poh_start_hash_slot_first_data_set() {
        let mut todo = vec![];
        let mut waiting_for_parent_hash = HashMap::new();

        let parent_slot_last_entry_hash = Hash::new_unique();
        let mut data_sets = HashMap::new();
        let entries = Arc::new(create_ticks(10, 0, parent_slot_last_entry_hash));
        let next_data_set = 7.into();

        insert_data_set_entries(
            &mut todo,
            &mut waiting_for_parent_hash,
            &mut data_sets,
            3,
            Some(parent_slot_last_entry_hash),
            4,
            0.into(),
            entries.clone(),
            next_data_set,
        );

        assert_eq!(
            todo,
            vec![TodoItem {
                slot: 4,
                op: TodoOp::All {
                    poh_start_hash: parent_slot_last_entry_hash,
                    entries,
                }
            }]
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());
        assert_eq_sorted!(
            data_sets,
            HashMap::from([(
                DataSetFirstShred::from(0),
                DataSetOptimisticStatus::Verified { next_data_set },
            )])
        );
    }

    #[test]
    fn with_poh_start_hash_slot_second_data_set() {
        let mut todo = vec![];
        let mut waiting_for_parent_hash = HashMap::new();

        let data_set_start = DataSetFirstShred::from(7);
        let poh_start_hash = Hash::new_unique();
        let mut data_sets = HashMap::from([(
            data_set_start,
            DataSetOptimisticStatus::PohStartHash(poh_start_hash),
        )]);
        let entries = Arc::new(create_ticks(10, 0, poh_start_hash));
        let next_data_set = 17.into();

        insert_data_set_entries(
            &mut todo,
            &mut waiting_for_parent_hash,
            &mut data_sets,
            3,
            None,
            4,
            data_set_start,
            entries.clone(),
            next_data_set,
        );

        assert_eq!(
            todo,
            vec![TodoItem {
                slot: 4,
                op: TodoOp::All {
                    poh_start_hash,
                    entries
                }
            }]
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());
        assert_eq_sorted!(
            data_sets,
            HashMap::from([(
                data_set_start,
                DataSetOptimisticStatus::Verified { next_data_set },
            )])
        );
    }
}

mod insert_poh_start_hash {
    use {super::*, pretty_assertions_sorted::assert_eq};

    #[test]
    fn data_set_with_no_state() {
        let mut todo = vec![];
        let slot = 3;
        let data_set_start = DataSetFirstShred::from(7);
        let mut slot_status = SlotStatus::Processing {
            parent_slot: 3,
            verified_up_to: 0.into(),
            last_data_set: None,
            data_sets: HashMap::new(),
        };
        let poh_start_hash = Hash::new_unique();

        insert_poh_start_hash(
            &mut todo,
            slot,
            &mut slot_status,
            data_set_start,
            poh_start_hash,
        );

        assert_eq!(todo, vec![]);
        assert_eq!(
            slot_status,
            SlotStatus::Processing {
                parent_slot: 3,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::from([(
                    data_set_start,
                    DataSetOptimisticStatus::PohStartHash(poh_start_hash),
                )]),
            }
        );
    }

    #[test]
    fn verified_slot() {
        let mut todo = vec![];
        let slot = 3;
        let data_set_start = DataSetFirstShred::from(7);
        let slot_last_entry_hash = Hash::new_unique();
        let mut slot_status = SlotStatus::Verified {
            last_entry_hash: slot_last_entry_hash,
        };
        let poh_start_hash = Hash::new_unique();

        let res = panic::catch_unwind(move || {
            insert_poh_start_hash(
                &mut todo,
                slot,
                &mut slot_status,
                data_set_start,
                poh_start_hash,
            )
        });

        assert!(
            res.is_err(),
            "In tests, insert_poh_start_hash() should panic when the target slot is already \
             verified."
        );
        let err = res.unwrap_err();
        assert_eq!(
            err.downcast_ref::<String>(),
            Some(
                &"entry-verification-service: insert_poh_start_hash: Target data set is in a \
                  slot that is already marked verified.\n\
                  slot: 3, shred: 7"
                    .to_string()
            )
        );
    }

    #[test]
    fn failed_slot() {
        let mut todo = vec![];
        let slot = 3;
        let data_set_start = DataSetFirstShred::from(7);
        let mut slot_status = SlotStatus::Failed;
        let poh_start_hash = Hash::new_unique();

        insert_poh_start_hash(
            &mut todo,
            slot,
            &mut slot_status,
            data_set_start,
            poh_start_hash,
        );

        assert_eq!(todo, vec![]);
        assert_eq!(slot_status, SlotStatus::Failed);
    }

    #[test]
    fn data_set_poh_start_hash() {
        let mut todo = vec![];
        let slot = 3;
        let data_set_start = DataSetFirstShred::from(7);
        let existing_poh_start_hash = Hash::new_unique();
        let mut slot_status = SlotStatus::Processing {
            parent_slot: 3,
            verified_up_to: 0.into(),
            last_data_set: None,
            data_sets: HashMap::from([(
                data_set_start,
                DataSetOptimisticStatus::PohStartHash(existing_poh_start_hash),
            )]),
        };
        let poh_start_hash = Hash::new_unique();

        let res = panic::catch_unwind(move || {
            insert_poh_start_hash(
                &mut todo,
                slot,
                &mut slot_status,
                data_set_start,
                poh_start_hash,
            )
        });

        assert!(
            res.is_err(),
            "In tests, insert_poh_start_hash() should panic when the target data set has state \
             PohStartHash(_)."
        );
        let err = res.unwrap_err();
        assert_eq!(
            err.downcast_ref::<String>(),
            Some(
                &"entry-verification-service: insert_poh_start_hash: Target data set already has \
                  a starting hash.\n\
                  slot: 3, shred: 7"
                    .to_string()
            )
        );
    }

    #[test]
    fn data_set_all_but_first() {
        let mut todo = vec![];
        let slot = 3;
        let data_set_start = DataSetFirstShred::from(7);
        let entries = create_ticks(10, 0, Hash::new_unique());
        let first_entry = entries.first().unwrap().clone();
        let next_data_set = 17.into();
        let mut slot_status = SlotStatus::Processing {
            parent_slot: 3,
            verified_up_to: 0.into(),
            last_data_set: None,
            data_sets: HashMap::from([(
                data_set_start,
                DataSetOptimisticStatus::AllButFirst {
                    next_data_set,
                    first_entry: first_entry.clone(),
                },
            )]),
        };
        let poh_start_hash = Hash::new_unique();

        insert_poh_start_hash(
            &mut todo,
            slot,
            &mut slot_status,
            data_set_start,
            poh_start_hash,
        );

        assert_eq!(
            todo,
            vec![TodoItem {
                slot,
                op: TodoOp::OnlyFirstEntryPoh {
                    poh_start_hash,
                    entry: first_entry,
                }
            }]
        );
        assert_eq!(
            slot_status,
            SlotStatus::Processing {
                parent_slot: 3,
                verified_up_to: 0.into(),
                last_data_set: None,
                data_sets: HashMap::from([(
                    data_set_start,
                    DataSetOptimisticStatus::Verified { next_data_set }
                )]),
            }
        );
    }

    #[test]
    fn data_set_verified() {
        let mut todo = vec![];
        let slot = 3;
        let data_set_start = DataSetFirstShred::from(7);
        let next_data_set = 17.into();
        let mut slot_status = SlotStatus::Processing {
            parent_slot: 3,
            verified_up_to: 0.into(),
            last_data_set: None,
            data_sets: HashMap::from([(
                data_set_start,
                DataSetOptimisticStatus::Verified { next_data_set },
            )]),
        };
        let poh_start_hash = Hash::new_unique();

        let res = panic::catch_unwind(move || {
            insert_poh_start_hash(
                &mut todo,
                slot,
                &mut slot_status,
                data_set_start,
                poh_start_hash,
            )
        });

        assert!(
            res.is_err(),
            "In tests, insert_poh_start_hash() should panic when the target data set has state \
             PohStartHash(_)."
        );
        let err = res.unwrap_err();
        assert_eq!(
            err.downcast_ref::<String>(),
            Some(
                &"entry-verification-service: insert_poh_start_hash: Target data set is already \
                  marked verified.\n\
                  slot: 3, shred: 7"
                    .to_string()
            )
        );
    }
}

mod insert_poh_start_hash_into_child_slots {
    use {
        super::*,
        pretty_assertions_sorted::{assert_eq, assert_eq_sorted},
    };

    #[test]
    fn no_child_slots() {
        let mut todo = vec![];
        let mut processing_slots = HashMap::from([(
            7,
            SlotStatus::Processing {
                parent_slot: 6,
                verified_up_to: 11.into(),
                last_data_set: None,
                data_sets: HashMap::new(),
            },
        )]);
        let mut waiting_for_parent_hash = HashMap::from([(7, vec![8, 9])]);
        let slot = 4;
        let last_entry_hash = Hash::new_unique();

        insert_poh_start_hash_into_child_slots(
            &mut todo,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            slot,
            last_entry_hash,
        );

        assert_eq!(todo, vec![]);
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(
                7,
                SlotStatus::Processing {
                    parent_slot: 6,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            )])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::from([(7, vec![8, 9])]));
    }

    #[test]
    fn one_child_with_no_state() {
        let mut todo = vec![];
        let mut processing_slots = HashMap::from([(
            7,
            SlotStatus::Processing {
                parent_slot: 6,
                verified_up_to: 11.into(),
                last_data_set: None,
                data_sets: HashMap::new(),
            },
        )]);
        let mut waiting_for_parent_hash = HashMap::from([(7, vec![8])]);
        let slot = 7;
        let last_entry_hash = Hash::new_unique();

        let res = panic::catch_unwind(move || {
            insert_poh_start_hash_into_child_slots(
                &mut todo,
                &mut processing_slots,
                &mut waiting_for_parent_hash,
                slot,
                last_entry_hash,
            )
        });

        assert!(
            res.is_err(),
            "In tests, insert_poh_start_hash_into_child_slots() should panic when a child slot \
             has no state."
        );
        let err = res.unwrap_err();
        assert_eq!(
            err.downcast_ref::<String>(),
            Some(
                &"entry-verification-service: insert_poh_start_hash_into_child_slots: Child slot \
                 should always have state, otherwise it must not be in \
                 `waiting_for_parent_hash`.\n\
                 parent_slot: 7, child_slot: 8"
                    .to_string()
            )
        );
    }

    #[test]
    fn one_child_data_set_all_but_first() {
        let mut todo = vec![];
        let child_slot_entries = create_ticks(10, 0, Hash::new_unique());
        let child_slot_first_entry = child_slot_entries.first().unwrap().clone();
        let mut processing_slots = HashMap::from([
            (
                7,
                SlotStatus::Processing {
                    parent_slot: 6,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
            (
                8,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        0.into(),
                        DataSetOptimisticStatus::AllButFirst {
                            next_data_set: 7.into(),
                            first_entry: child_slot_first_entry.clone(),
                        },
                    )]),
                },
            ),
        ]);
        let mut waiting_for_parent_hash = HashMap::from([(7, vec![8])]);
        let slot = 7;
        let last_entry_hash = Hash::new_unique();

        insert_poh_start_hash_into_child_slots(
            &mut todo,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            slot,
            last_entry_hash,
        );

        assert_eq!(
            todo,
            vec![TodoItem {
                slot: 8,
                op: TodoOp::OnlyFirstEntryPoh {
                    poh_start_hash: last_entry_hash,
                    entry: child_slot_first_entry,
                },
            }]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    7,
                    SlotStatus::Processing {
                        parent_slot: 6,
                        verified_up_to: 11.into(),
                        last_data_set: None,
                        data_sets: HashMap::new(),
                    },
                ),
                (
                    8,
                    SlotStatus::Processing {
                        parent_slot: 7,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([(
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 7.into(),
                            }
                        )]),
                    },
                ),
            ])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());
    }

    #[test]
    fn one_child_data_set_with_no_state() {
        let mut todo = vec![];
        let mut processing_slots = HashMap::from([
            (
                7,
                SlotStatus::Processing {
                    parent_slot: 6,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
            (
                8,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
        ]);
        let mut waiting_for_parent_hash = HashMap::from([(7, vec![8])]);
        let slot = 7;
        let last_entry_hash = Hash::new_unique();

        insert_poh_start_hash_into_child_slots(
            &mut todo,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            slot,
            last_entry_hash,
        );

        assert_eq!(todo, vec![]);
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    7,
                    SlotStatus::Processing {
                        parent_slot: 6,
                        verified_up_to: 11.into(),
                        last_data_set: None,
                        data_sets: HashMap::new(),
                    },
                ),
                (
                    8,
                    SlotStatus::Processing {
                        parent_slot: 7,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([(
                            0.into(),
                            DataSetOptimisticStatus::PohStartHash(last_entry_hash)
                        )]),
                    },
                ),
            ])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());
    }

    #[test]
    fn multiple_children() {
        let mut todo = vec![];

        let child_8_entries = create_ticks(10, 0, Hash::new_unique());
        let child_8_first_entry = child_8_entries.first().unwrap().clone();

        let child_9_entries = create_ticks(10, 0, Hash::new_unique());
        let child_9_first_entry = child_9_entries.first().unwrap().clone();

        let child_10_entries = create_ticks(10, 0, Hash::new_unique());
        let child_10_first_entry = child_10_entries.first().unwrap().clone();

        let mut processing_slots = HashMap::from([
            (
                7,
                SlotStatus::Processing {
                    parent_slot: 6,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
            (
                8,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        0.into(),
                        DataSetOptimisticStatus::AllButFirst {
                            next_data_set: 13.into(),
                            first_entry: child_8_first_entry.clone(),
                        },
                    )]),
                },
            ),
            (
                9,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        0.into(),
                        DataSetOptimisticStatus::AllButFirst {
                            next_data_set: 7.into(),
                            first_entry: child_9_first_entry.clone(),
                        },
                    )]),
                },
            ),
            (
                10,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::from([(
                        0.into(),
                        DataSetOptimisticStatus::AllButFirst {
                            next_data_set: 11.into(),
                            first_entry: child_10_first_entry.clone(),
                        },
                    )]),
                },
            ),
        ]);
        let mut waiting_for_parent_hash = HashMap::from([(7, vec![8, 9, 10])]);
        let slot = 7;
        let last_entry_hash = Hash::new_unique();

        insert_poh_start_hash_into_child_slots(
            &mut todo,
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            slot,
            last_entry_hash,
        );

        assert_eq!(
            todo,
            vec![
                TodoItem {
                    slot: 8,
                    op: TodoOp::OnlyFirstEntryPoh {
                        poh_start_hash: last_entry_hash,
                        entry: child_8_first_entry,
                    },
                },
                TodoItem {
                    slot: 9,
                    op: TodoOp::OnlyFirstEntryPoh {
                        poh_start_hash: last_entry_hash,
                        entry: child_9_first_entry,
                    },
                },
                TodoItem {
                    slot: 10,
                    op: TodoOp::OnlyFirstEntryPoh {
                        poh_start_hash: last_entry_hash,
                        entry: child_10_first_entry,
                    },
                },
            ]
        );
        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (
                    7,
                    SlotStatus::Processing {
                        parent_slot: 6,
                        verified_up_to: 11.into(),
                        last_data_set: None,
                        data_sets: HashMap::new(),
                    },
                ),
                (
                    8,
                    SlotStatus::Processing {
                        parent_slot: 7,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([(
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 13.into(),
                            }
                        )]),
                    },
                ),
                (
                    9,
                    SlotStatus::Processing {
                        parent_slot: 7,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([(
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 7.into(),
                            }
                        )]),
                    },
                ),
                (
                    10,
                    SlotStatus::Processing {
                        parent_slot: 7,
                        verified_up_to: 0.into(),
                        last_data_set: None,
                        data_sets: HashMap::from([(
                            0.into(),
                            DataSetOptimisticStatus::Verified {
                                next_data_set: 11.into(),
                            }
                        )]),
                    },
                ),
            ])
        );
        assert_eq_sorted!(waiting_for_parent_hash, HashMap::new());
    }
}

mod mark_slot_failed {
    use {super::*, pretty_assertions_sorted::assert_eq_sorted};

    #[test]
    fn slot_with_no_state() {
        let mut processing_slots = HashMap::from([(
            7,
            SlotStatus::Processing {
                parent_slot: 6,
                verified_up_to: 11.into(),
                last_data_set: None,
                data_sets: HashMap::new(),
            },
        )]);
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();

        mark_slot_failed(
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            4,
        );

        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (4, SlotStatus::Failed),
                (
                    7,
                    SlotStatus::Processing {
                        parent_slot: 6,
                        verified_up_to: 11.into(),
                        last_data_set: None,
                        data_sets: HashMap::new(),
                    }
                ),
            ]),
            "Should insert an entry if the slot does not have state yet"
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::new(),
            "`waiting_for_parent_hash` stays empty"
        );

        assert_non_empty_results_in_millis!(
            10,
            results,
            Results {
                starting_at: 0,
                slots: HashMap::from([(4, SlotVerificationStatus::Failed)]),
            },
        );
    }

    #[test]
    fn slot_being_processed() {
        let mut processing_slots = HashMap::from([(
            7,
            SlotStatus::Processing {
                parent_slot: 6,
                verified_up_to: 11.into(),
                last_data_set: None,
                data_sets: HashMap::new(),
            },
        )]);
        let mut waiting_for_parent_hash = HashMap::new();
        let results = SharedWrapper::default();

        mark_slot_failed(
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            7,
        );

        assert_eq_sorted!(
            processing_slots,
            HashMap::from([(7, SlotStatus::Failed)]),
            "Existing slot is marked `Failed`"
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::new(),
            "`waiting_for_parent_hash` stays empty"
        );

        assert_non_empty_results_in_millis!(
            10,
            results,
            Results {
                starting_at: 0,
                slots: HashMap::from([(7, SlotVerificationStatus::Failed)]),
            }
        );
    }

    #[test]
    fn with_children() {
        let mut processing_slots = HashMap::from([
            (
                7,
                SlotStatus::Processing {
                    parent_slot: 6,
                    verified_up_to: 11.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
            (
                8,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
            (
                9,
                SlotStatus::Processing {
                    parent_slot: 7,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
            (
                10,
                SlotStatus::Processing {
                    parent_slot: 9,
                    verified_up_to: 0.into(),
                    last_data_set: None,
                    data_sets: HashMap::new(),
                },
            ),
        ]);
        let mut waiting_for_parent_hash = HashMap::from([(7, vec![8, 9]), (9, vec![10])]);
        let results = SharedWrapper::default();

        mark_slot_failed(
            &mut processing_slots,
            &mut waiting_for_parent_hash,
            &results,
            7,
        );

        assert_eq_sorted!(
            processing_slots,
            HashMap::from([
                (7, SlotStatus::Failed),
                (8, SlotStatus::Failed),
                (9, SlotStatus::Failed),
                (10, SlotStatus::Failed),
            ]),
            "Existing slot is marked `Failed`"
        );
        assert_eq_sorted!(
            waiting_for_parent_hash,
            HashMap::new(),
            "Failed entries are removed from `waiting_for_parent_hash`"
        );

        assert_non_empty_results_in_millis!(
            10,
            results,
            Results {
                starting_at: 0,
                slots: HashMap::from([
                    (7, SlotVerificationStatus::Failed),
                    (8, SlotVerificationStatus::Failed),
                    (9, SlotVerificationStatus::Failed),
                    (10, SlotVerificationStatus::Failed),
                ]),
            }
        );
    }
}

mod get_parent_slot {
    use {super::*, pretty_assertions_sorted::assert_eq};

    #[test]
    fn existing_parent() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let start_slot = 7;
        let num_slots = 2;
        let entries_per_slot = 100;
        let (shreds, _entries) = make_many_slot_entries(start_slot, num_slots, entries_per_slot);
        let shreds_per_slot = u32::try_from(shreds.len() / num_slots as usize).unwrap();
        // Make sure we have more than 1 shred to test.
        assert!(shreds_per_slot > 1);

        blockstore.insert_shreds(shreds, None, true).unwrap();

        for slot in start_slot..(start_slot + num_slots) {
            for shred_index in 0..shreds_per_slot {
                let parent_slot = get_parent_slot(&blockstore, slot, shred_index);

                assert_eq!(
                    parent_slot,
                    Some(if slot == 0 { 0 } else { slot.saturating_sub(1) }),
                    "All inserted shreds are present, and point to parent slots",
                );
            }
        }
    }

    #[test]
    fn missing_parent() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let start_slot = 6;
        let num_slots = 3;
        let entries_per_slot = 100;
        let (mut shreds, _entries) =
            make_many_slot_entries(start_slot, num_slots, entries_per_slot);
        let shreds_per_slot = u32::try_from(shreds.len() / num_slots as usize).unwrap();
        // Make sure we have more than 1 shred to test.
        assert!(shreds_per_slot > 1);

        let last_slot = start_slot + num_slots - 1;
        let last_slot_first_shred =
            usize::try_from((num_slots - 1) * (shreds_per_slot as u64)).unwrap();
        let third_slot_shreds = shreds.split_off(last_slot_first_shred);
        blockstore
            .insert_shreds(third_slot_shreds, None, true)
            .unwrap();

        for shred_index in 0..shreds_per_slot {
            let parent_slot = get_parent_slot(&blockstore, last_slot, shred_index);

            assert_eq!(
                parent_slot,
                Some(last_slot - 1),
                "Even if parent shreds are absent, we get correct parent slot index",
            );
        }
    }

    #[test]
    fn missing_shred() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let start_slot = 8;
        let num_slots = 3;
        let entries_per_slot = 100;
        let (mut shreds, _entries) =
            make_many_slot_entries(start_slot, num_slots, entries_per_slot);
        let shreds_per_slot = u32::try_from(shreds.len() / num_slots as usize).unwrap();
        // Make sure we have more than 1 shred to test.
        assert!(shreds_per_slot > 1);

        let last_slot = num_slots - 1;
        let last_slot_first_shred = usize::try_from(last_slot * (shreds_per_slot as u64)).unwrap();
        let _third_slot_shreds = shreds.split_off(last_slot_first_shred);
        blockstore.insert_shreds(shreds, None, true).unwrap();

        for shred_index in 0..shreds_per_slot {
            let res = panic::catch_unwind(|| get_parent_slot(&blockstore, last_slot, shred_index));

            assert!(
                res.is_err(),
                "In tests, get_parent_slot() should panic when the target shred is absent.\n\
                 Got: {:?}",
                res.ok()
            );

            let err = res.unwrap_err();
            assert_eq!(
                err.downcast_ref::<String>(),
                Some(&format!(
                    "entry-verification-service: get_parent_slot: get_data_shred() returned None.\n\
                     slot: {last_slot}, shred: {shred_index}"
                )),
                "get_parent_slot() paniced with unexpected message."
            );
        }
    }
}
