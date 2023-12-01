use {super::*, solana_sdk::message::AccountKeys, std::time::Instant};

#[derive(Default)]
pub struct PurgeStats {
    delete_range: u64,
    write_batch: u64,
    delete_files_in_range: u64,
}

#[derive(Clone, Copy)]
/// Controls how `blockstore::purge_slots` purges the data.
pub enum PurgeType {
    /// A slower but more accurate way to purge slots by also ensuring higher
    /// level of consistency between data during the clean up process.
    Exact,
    /// A faster approximation of `Exact` where the purge process only takes
    /// care of the primary index and does not update the associated entries.
    PrimaryIndex,
    /// The fastest purge mode that relies on the slot-id based TTL
    /// compaction filter to do the cleanup.
    CompactionFilter,
}

impl Blockstore {
    /// Performs cleanup based on the specified deletion range.  After this
    /// function call, entries within \[`from_slot`, `to_slot`\] will become
    /// unavailable to the reader immediately, while its disk space occupied
    /// by the deletion entries are reclaimed later via RocksDB's background
    /// compaction.
    ///
    /// Note that this function modifies multiple column families at the same
    /// time and might break the consistency between different column families
    /// as it does not update the associated slot-meta entries that refer to
    /// the deleted entries.
    ///
    /// For slot-id based column families, the purge is done by range deletion,
    /// while the non-slot-id based column families, `cf::TransactionStatus`,
    /// `AddressSignature`, and `cf::TransactionStatusIndex`, are cleaned-up
    /// based on the `purge_type` setting.
    pub fn purge_slots(&self, from_slot: Slot, to_slot: Slot, purge_type: PurgeType) {
        let mut purge_stats = PurgeStats::default();
        let purge_result =
            self.run_purge_with_stats(from_slot, to_slot, purge_type, &mut purge_stats);

        datapoint_info!(
            "blockstore-purge",
            ("from_slot", from_slot as i64, i64),
            ("to_slot", to_slot as i64, i64),
            ("delete_range_us", purge_stats.delete_range as i64, i64),
            ("write_batch_us", purge_stats.write_batch as i64, i64),
            (
                "delete_files_in_range_us",
                purge_stats.write_batch as i64,
                i64
            )
        );
        if let Err(e) = purge_result {
            error!(
                "Error: {:?}; Purge failed in range {:?} to {:?}",
                e, from_slot, to_slot
            );
        }
    }

    /// Usually this is paired with .purge_slots() but we can't internally call this in
    /// that function unconditionally. That's because set_max_expired_slot()
    /// expects to purge older slots by the successive chronological order, while .purge_slots()
    /// can also be used to purge *future* slots for --hard-fork thing, preserving older
    /// slots. It'd be quite dangerous to purge older slots in that case.
    /// So, current legal user of this function is LedgerCleanupService.
    pub fn set_max_expired_slot(&self, to_slot: Slot) {
        // convert here from inclusive purged range end to inclusive alive range start to align
        // with Slot::default() for initial compaction filter behavior consistency
        let to_slot = to_slot.checked_add(1).unwrap();
        self.db.set_oldest_slot(to_slot);
    }

    pub fn purge_and_compact_slots(&self, from_slot: Slot, to_slot: Slot) {
        self.purge_slots(from_slot, to_slot, PurgeType::Exact);
    }

    /// Ensures that the SlotMeta::next_slots vector for all slots contain no references in the
    /// \[from_slot,to_slot\] range
    ///
    /// Dangerous; Use with care
    pub fn purge_from_next_slots(&self, from_slot: Slot, to_slot: Slot) {
        let mut count = 0;
        let mut rewritten = 0;
        let mut last_print = Instant::now();
        let mut total_retain_us = 0;
        for (slot, mut meta) in self
            .slot_meta_iterator(0)
            .expect("unable to iterate over meta")
        {
            if slot > to_slot {
                break;
            }

            count += 1;
            if last_print.elapsed().as_millis() > 2000 {
                info!(
                    "purged: {} slots rewritten: {} retain_time: {}us",
                    count, rewritten, total_retain_us
                );
                count = 0;
                rewritten = 0;
                total_retain_us = 0;
                last_print = Instant::now();
            }
            let mut time = Measure::start("retain");
            let original_len = meta.next_slots.len();
            meta.next_slots
                .retain(|slot| *slot < from_slot || *slot > to_slot);
            if meta.next_slots.len() != original_len {
                rewritten += 1;
                info!(
                    "purge_from_next_slots: meta for slot {} no longer refers to slots {:?}",
                    slot,
                    from_slot..=to_slot
                );
                self.put_meta_bytes(
                    slot,
                    &bincode::serialize(&meta).expect("couldn't update meta"),
                )
                .expect("couldn't update meta");
            }
            time.stop();
            total_retain_us += time.as_us();
        }
    }

    pub(crate) fn run_purge(
        &self,
        from_slot: Slot,
        to_slot: Slot,
        purge_type: PurgeType,
    ) -> Result<bool> {
        self.run_purge_with_stats(from_slot, to_slot, purge_type, &mut PurgeStats::default())
    }

    /// A helper function to `purge_slots` that executes the ledger clean up.
    /// The cleanup applies to \[`from_slot`, `to_slot`\].
    ///
    /// When `from_slot` is 0, any sst-file with a key-range completely older
    /// than `to_slot` will also be deleted.
    pub(crate) fn run_purge_with_stats(
        &self,
        from_slot: Slot,
        to_slot: Slot,
        purge_type: PurgeType,
        purge_stats: &mut PurgeStats,
    ) -> Result<bool> {
        let mut write_batch = self
            .db
            .batch()
            .expect("Database Error: Failed to get write batch");
        let mut delete_range_timer = Measure::start("delete_range");
        let mut columns_purged = self
            .db
            .delete_range_cf::<cf::SlotMeta>(&mut write_batch, from_slot, to_slot)
            .is_ok()
            & self
                .db
                .delete_range_cf::<cf::BankHash>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::Root>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::ShredData>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::ShredCode>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::DeadSlots>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::DuplicateSlots>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::ErasureMeta>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::Orphans>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::Index>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::Rewards>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::Blocktime>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::PerfSamples>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::BlockHeight>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::OptimisticSlots>(&mut write_batch, from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_range_cf::<cf::MerkleRootMeta>(&mut write_batch, from_slot, to_slot)
                .is_ok();
        let mut w_active_transaction_status_index =
            self.active_transaction_status_index.write().unwrap();
        match purge_type {
            PurgeType::Exact => {
                self.purge_special_columns_exact(&mut write_batch, from_slot, to_slot)?;
            }
            PurgeType::PrimaryIndex => {
                self.purge_special_columns_with_primary_index(
                    &mut write_batch,
                    &mut columns_purged,
                    &mut w_active_transaction_status_index,
                    to_slot,
                )?;
            }
            PurgeType::CompactionFilter => {
                // No explicit action is required here because this purge type completely and
                // indefinitely relies on the proper working of compaction filter for those
                // special column families, never toggling the primary index from the current
                // one. Overall, this enables well uniformly distributed writes, resulting
                // in no spiky periodic huge delete_range for them.
            }
        }
        delete_range_timer.stop();

        let mut write_timer = Measure::start("write_batch");
        if let Err(e) = self.db.write(write_batch) {
            error!(
                "Error: {:?} while submitting write batch for slot {:?} retrying...",
                e, from_slot
            );
            return Err(e);
        }
        write_timer.stop();

        let mut purge_files_in_range_timer = Measure::start("delete_file_in_range");
        // purge_files_in_range delete any files whose slot range is within
        // [from_slot, to_slot].  When from_slot is 0, it is safe to run
        // purge_files_in_range because if purge_files_in_range deletes any
        // sst file that contains any range-deletion tombstone, the deletion
        // range of that tombstone will be completely covered by the new
        // range-delete tombstone (0, to_slot) issued above.
        //
        // On the other hand, purge_files_in_range is more effective and
        // efficient than the compaction filter (which runs key-by-key)
        // because all the sst files that have key range below to_slot
        // can be deleted immediately.
        if columns_purged && from_slot == 0 {
            self.purge_files_in_range(from_slot, to_slot);
        }
        purge_files_in_range_timer.stop();

        purge_stats.delete_range += delete_range_timer.as_us();
        purge_stats.write_batch += write_timer.as_us();
        purge_stats.delete_files_in_range += purge_files_in_range_timer.as_us();

        // only drop w_active_transaction_status_index after we do db.write(write_batch);
        // otherwise, readers might be confused with inconsistent state between
        // self.active_transaction_status_index and RockDb's TransactionStatusIndex contents
        drop(w_active_transaction_status_index);
        Ok(columns_purged)
    }

    fn purge_files_in_range(&self, from_slot: Slot, to_slot: Slot) -> bool {
        self.db
            .delete_file_in_range_cf::<cf::SlotMeta>(from_slot, to_slot)
            .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::BankHash>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::Root>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::ShredData>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::ShredCode>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::DeadSlots>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::DuplicateSlots>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::ErasureMeta>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::Orphans>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::Index>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::Rewards>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::Blocktime>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::PerfSamples>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::BlockHeight>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::OptimisticSlots>(from_slot, to_slot)
                .is_ok()
            & self
                .db
                .delete_file_in_range_cf::<cf::MerkleRootMeta>(from_slot, to_slot)
                .is_ok()
    }

    /// Purges special columns (using a non-Slot primary-index) exactly, by
    /// deserializing each slot being purged and iterating through all
    /// transactions to determine the keys of individual records.
    ///
    /// The purge range applies to \[`from_slot`, `to_slot`\].
    ///
    /// **This method is very slow.**
    fn purge_special_columns_exact(
        &self,
        batch: &mut WriteBatch,
        from_slot: Slot,
        to_slot: Slot,
    ) -> Result<()> {
        let mut index0 = self.transaction_status_index_cf.get(0)?.unwrap_or_default();
        let mut index1 = self.transaction_status_index_cf.get(1)?.unwrap_or_default();
        for slot in from_slot..=to_slot {
            let slot_entries = self.get_any_valid_slot_entries(slot, 0);
            let transactions = slot_entries
                .into_iter()
                .flat_map(|entry| entry.transactions);
            for transaction in transactions {
                if let Some(&signature) = transaction.signatures.get(0) {
                    batch.delete::<cf::TransactionStatus>((0, signature, slot))?;
                    batch.delete::<cf::TransactionStatus>((1, signature, slot))?;

                    let meta = self.read_transaction_status((signature, slot))?;
                    let loaded_addresses = meta.map(|meta| meta.loaded_addresses);
                    let account_keys = AccountKeys::new(
                        transaction.message.static_account_keys(),
                        loaded_addresses.as_ref(),
                    );

                    for pubkey in account_keys.iter() {
                        batch.delete::<cf::AddressSignatures>((0, *pubkey, slot, signature))?;
                        batch.delete::<cf::AddressSignatures>((1, *pubkey, slot, signature))?;
                    }
                }
            }
        }
        if index0.max_slot >= from_slot && index0.max_slot <= to_slot {
            index0.max_slot = from_slot.saturating_sub(1);
            batch.put::<cf::TransactionStatusIndex>(0, &index0)?;
        }
        if index1.max_slot >= from_slot && index1.max_slot <= to_slot {
            index1.max_slot = from_slot.saturating_sub(1);
            batch.put::<cf::TransactionStatusIndex>(1, &index1)?;
        }
        Ok(())
    }

    /// Purges special columns (using a non-Slot primary-index) by range. Purge
    /// occurs if frozen primary index has a max-slot less than the highest slot
    /// being purged.
    fn purge_special_columns_with_primary_index(
        &self,
        write_batch: &mut WriteBatch,
        columns_purged: &mut bool,
        w_active_transaction_status_index: &mut u64,
        to_slot: Slot,
    ) -> Result<()> {
        if let Some(purged_index) = self.toggle_transaction_status_index(
            write_batch,
            w_active_transaction_status_index,
            to_slot + 1,
        )? {
            *columns_purged &= self
                .db
                .delete_range_cf::<cf::TransactionStatus>(write_batch, purged_index, purged_index)
                .is_ok()
                & self
                    .db
                    .delete_range_cf::<cf::AddressSignatures>(
                        write_batch,
                        purged_index,
                        purged_index,
                    )
                    .is_ok();
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use {
        super::*,
        crate::{
            blockstore::tests::make_slot_entries_with_transactions, get_tmp_ledger_path_auto_delete,
        },
        bincode::serialize,
        solana_entry::entry::next_entry_mut,
        solana_sdk::{
            hash::{hash, Hash},
            message::Message,
            transaction::Transaction,
        },
    };

    #[test]
    fn test_purge_slots() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        blockstore.purge_and_compact_slots(0, 5);

        test_all_empty_or_min(&blockstore, 6);

        blockstore.purge_and_compact_slots(0, 50);

        // min slot shouldn't matter, blockstore should be empty
        test_all_empty_or_min(&blockstore, 100);
        test_all_empty_or_min(&blockstore, 0);

        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(_, _)| {
                panic!();
            });
    }

    #[test]
    fn test_purge_front_of_ledger() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let max_slot = 10;
        for x in 0..max_slot {
            let random_bytes: [u8; 64] = std::array::from_fn(|_| rand::random::<u8>());
            blockstore
                .write_transaction_status(
                    x,
                    Signature::from(random_bytes),
                    vec![&Pubkey::try_from(&random_bytes[..32]).unwrap()],
                    vec![&Pubkey::try_from(&random_bytes[32..]).unwrap()],
                    TransactionStatusMeta::default(),
                )
                .unwrap();
        }
        // Purge to freeze index 0
        blockstore.run_purge(0, 1, PurgeType::PrimaryIndex).unwrap();

        for x in max_slot..2 * max_slot {
            let random_bytes: [u8; 64] = std::array::from_fn(|_| rand::random::<u8>());
            blockstore
                .write_transaction_status(
                    x,
                    Signature::from(random_bytes),
                    vec![&Pubkey::try_from(&random_bytes[..32]).unwrap()],
                    vec![&Pubkey::try_from(&random_bytes[32..]).unwrap()],
                    TransactionStatusMeta::default(),
                )
                .unwrap();
        }

        // Purging range outside of TransactionStatus max slots should not affect TransactionStatus data
        blockstore.run_purge(20, 30, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        let entry = status_entry_iterator.next().unwrap().0;
        assert_eq!(entry.0, 0);
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_purge_transaction_status() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let transaction_status_index_cf = &blockstore.transaction_status_index_cf;
        let slot = 10;
        for _ in 0..5 {
            let random_bytes: [u8; 64] = std::array::from_fn(|_| rand::random::<u8>());
            blockstore
                .write_transaction_status(
                    slot,
                    Signature::from(random_bytes),
                    vec![&Pubkey::try_from(&random_bytes[..32]).unwrap()],
                    vec![&Pubkey::try_from(&random_bytes[32..]).unwrap()],
                    TransactionStatusMeta::default(),
                )
                .unwrap();
        }
        // Purge to freeze index 0
        blockstore.run_purge(0, 1, PurgeType::PrimaryIndex).unwrap();
        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        for _ in 0..5 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert_eq!(entry.2, slot);
        }
        let mut address_transactions_iterator = blockstore
            .db
            .iter::<cf::AddressSignatures>(IteratorMode::From(
                (0, Pubkey::default(), 0, Signature::default()),
                IteratorDirection::Forward,
            ))
            .unwrap();
        for _ in 0..10 {
            let entry = address_transactions_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert_eq!(entry.2, slot);
        }
        assert_eq!(
            transaction_status_index_cf.get(0).unwrap().unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 10,
                frozen: true,
            }
        );
        drop(status_entry_iterator);
        drop(address_transactions_iterator);

        // Low purge should not affect state
        blockstore.run_purge(0, 5, PurgeType::PrimaryIndex).unwrap();
        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        for _ in 0..5 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert_eq!(entry.2, slot);
        }
        let mut address_transactions_iterator = blockstore
            .db
            .iter::<cf::AddressSignatures>(IteratorMode::From(
                cf::AddressSignatures::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        for _ in 0..10 {
            let entry = address_transactions_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert_eq!(entry.2, slot);
        }
        assert_eq!(
            transaction_status_index_cf.get(0).unwrap().unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 10,
                frozen: true,
            }
        );
        drop(status_entry_iterator);
        drop(address_transactions_iterator);

        // Test boundary conditions: < slot should not purge statuses; <= slot should
        blockstore.run_purge(0, 9, PurgeType::PrimaryIndex).unwrap();
        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        for _ in 0..5 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert_eq!(entry.2, slot);
        }
        let mut address_transactions_iterator = blockstore
            .db
            .iter::<cf::AddressSignatures>(IteratorMode::From(
                cf::AddressSignatures::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        for _ in 0..10 {
            let entry = address_transactions_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert_eq!(entry.2, slot);
        }
        assert_eq!(
            transaction_status_index_cf.get(0).unwrap().unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 10,
                frozen: true,
            }
        );
        drop(status_entry_iterator);
        drop(address_transactions_iterator);

        blockstore
            .run_purge(0, 10, PurgeType::PrimaryIndex)
            .unwrap();
        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        let padding_entry = status_entry_iterator.next().unwrap().0;
        assert_eq!(padding_entry.0, 2);
        assert_eq!(padding_entry.2, 0);
        assert!(status_entry_iterator.next().is_none());
        let mut address_transactions_iterator = blockstore
            .db
            .iter::<cf::AddressSignatures>(IteratorMode::From(
                cf::AddressSignatures::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        let padding_entry = address_transactions_iterator.next().unwrap().0;
        assert_eq!(padding_entry.0, 2);
        assert_eq!(padding_entry.2, 0);
        assert!(address_transactions_iterator.next().is_none());
        assert_eq!(
            transaction_status_index_cf.get(0).unwrap().unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 0,
                frozen: false,
            }
        );
        assert_eq!(
            transaction_status_index_cf.get(1).unwrap().unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 0,
                frozen: true,
            }
        );
    }

    fn clear_and_repopulate_transaction_statuses_for_test(
        blockstore: &Blockstore,
        index0_max_slot: u64,
        index1_max_slot: u64,
    ) {
        assert!(index1_max_slot > index0_max_slot);
        let mut write_batch = blockstore.db.batch().unwrap();
        blockstore
            .run_purge(0, index1_max_slot, PurgeType::PrimaryIndex)
            .unwrap();
        blockstore
            .db
            .delete_range_cf::<cf::TransactionStatus>(&mut write_batch, 0, 2)
            .unwrap();
        blockstore
            .db
            .delete_range_cf::<cf::TransactionStatusIndex>(&mut write_batch, 0, 2)
            .unwrap();
        blockstore.db.write(write_batch).unwrap();
        blockstore.initialize_transaction_status_index().unwrap();
        *blockstore.active_transaction_status_index.write().unwrap() = 0;

        for x in 0..index0_max_slot + 1 {
            let entries = make_slot_entries_with_transactions(1);
            let shreds = entries_to_test_shreds(
                &entries,
                x,                   // slot
                x.saturating_sub(1), // parent_slot
                true,                // is_full_slot
                0,                   // version
                true,                // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let signature = entries
                .iter()
                .filter(|entry| !entry.is_tick())
                .cloned()
                .flat_map(|entry| entry.transactions)
                .map(|transaction| transaction.signatures[0])
                .collect::<Vec<Signature>>()[0];
            let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
            blockstore
                .write_transaction_status(
                    x,
                    signature,
                    vec![&Pubkey::try_from(&random_bytes[..32]).unwrap()],
                    vec![&Pubkey::try_from(&random_bytes[32..]).unwrap()],
                    TransactionStatusMeta::default(),
                )
                .unwrap();
        }
        // Freeze index 0
        let mut write_batch = blockstore.db.batch().unwrap();
        let mut w_active_transaction_status_index =
            blockstore.active_transaction_status_index.write().unwrap();
        blockstore
            .toggle_transaction_status_index(
                &mut write_batch,
                &mut w_active_transaction_status_index,
                index0_max_slot + 1,
            )
            .unwrap();
        drop(w_active_transaction_status_index);
        blockstore.db.write(write_batch).unwrap();

        for x in index0_max_slot + 1..index1_max_slot + 1 {
            let entries = make_slot_entries_with_transactions(1);
            let shreds = entries_to_test_shreds(
                &entries,
                x,                   // slot
                x.saturating_sub(1), // parent_slot
                true,                // is_full_slot
                0,                   // version
                true,                // merkle_variant
            );
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let signature: Signature = entries
                .iter()
                .filter(|entry| !entry.is_tick())
                .cloned()
                .flat_map(|entry| entry.transactions)
                .map(|transaction| transaction.signatures[0])
                .collect::<Vec<Signature>>()[0];
            let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
            blockstore
                .write_transaction_status(
                    x,
                    signature,
                    vec![&Pubkey::try_from(&random_bytes[..32]).unwrap()],
                    vec![&Pubkey::try_from(&random_bytes[32..]).unwrap()],
                    TransactionStatusMeta::default(),
                )
                .unwrap();
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index0_max_slot,
                frozen: true,
            }
        );
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index1_max_slot,
                frozen: false,
            }
        );
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_purge_transaction_status_exact() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let index0_max_slot = 9;
        let index1_max_slot = 19;

        // Test purge outside bounds
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore.run_purge(20, 22, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index0_max_slot,
                frozen: true,
            }
        );
        for _ in 0..index0_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index1_max_slot,
                frozen: false,
            }
        );
        for _ in index0_max_slot + 1..index1_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 1);
        }
        drop(status_entry_iterator);

        // Test purge inside index 0
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore.run_purge(2, 4, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index0_max_slot,
                frozen: true,
            }
        );
        for _ in 0..7 {
            // 7 entries remaining
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert!(entry.2 < 2 || entry.2 > 4);
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index1_max_slot,
                frozen: false,
            }
        );
        for _ in index0_max_slot + 1..index1_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 1);
        }
        drop(status_entry_iterator);

        // Test purge inside index 0 at upper boundary
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore
            .run_purge(7, index0_max_slot, PurgeType::Exact)
            .unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 6,
                frozen: true,
            }
        );
        for _ in 0..7 {
            // 7 entries remaining
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert!(entry.2 < 7);
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index1_max_slot,
                frozen: false,
            }
        );
        for _ in index0_max_slot + 1..index1_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 1);
        }
        drop(status_entry_iterator);

        // Test purge inside index 1 at lower boundary
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore.run_purge(10, 12, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index0_max_slot,
                frozen: true,
            }
        );
        for _ in 0..index0_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index1_max_slot,
                frozen: false,
            }
        );
        for _ in 13..index1_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 1);
            assert!(entry.2 > 12);
        }
        drop(status_entry_iterator);

        // Test purge across index boundaries
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore.run_purge(7, 12, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 6,
                frozen: true,
            }
        );
        for _ in 0..7 {
            // 7 entries remaining
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert!(entry.2 < 7);
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index1_max_slot,
                frozen: false,
            }
        );
        for _ in 13..index1_max_slot + 1 {
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 1);
            assert!(entry.2 > 12);
        }
        drop(status_entry_iterator);

        // Test purge include complete index 1
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore.run_purge(7, 22, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 6,
                frozen: true,
            }
        );
        for _ in 0..7 {
            // 7 entries remaining
            let entry = status_entry_iterator.next().unwrap().0;
            assert_eq!(entry.0, 0);
            assert!(entry.2 < 7);
        }
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 6,
                frozen: false,
            }
        );
        let entry = status_entry_iterator.next().unwrap().0;
        assert_eq!(entry.0, 2); // Buffer entry, no index 1 entries remaining
        drop(status_entry_iterator);

        // Purge up to but not including index0_max_slot
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore
            .run_purge(0, index0_max_slot - 1, PurgeType::Exact)
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: index0_max_slot,
                frozen: true,
            }
        );

        // Test purge all
        clear_and_repopulate_transaction_statuses_for_test(
            &blockstore,
            index0_max_slot,
            index1_max_slot,
        );
        blockstore.run_purge(0, 22, PurgeType::Exact).unwrap();

        let mut status_entry_iterator = blockstore
            .db
            .iter::<cf::TransactionStatus>(IteratorMode::From(
                cf::TransactionStatus::as_index(0),
                IteratorDirection::Forward,
            ))
            .unwrap();
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(0)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 0,
                frozen: true,
            }
        );
        assert_eq!(
            blockstore
                .transaction_status_index_cf
                .get(1)
                .unwrap()
                .unwrap(),
            TransactionStatusIndexMeta {
                max_slot: 0,
                frozen: false,
            }
        );
        let entry = status_entry_iterator.next().unwrap().0;
        assert_eq!(entry.0, 2); // Buffer entry, no index 0 or index 1 entries remaining
    }

    #[test]
    fn test_purge_special_columns_exact_no_sigs() {
        let ledger_path = get_tmp_ledger_path_auto_delete!();
        let blockstore = Blockstore::open(ledger_path.path()).unwrap();

        let slot = 1;
        let mut entries: Vec<Entry> = vec![];
        for x in 0..5 {
            let mut tx = Transaction::new_unsigned(Message::default());
            tx.signatures = vec![];
            entries.push(next_entry_mut(&mut Hash::default(), 0, vec![tx]));
            let mut tick = create_ticks(1, 0, hash(&serialize(&x).unwrap()));
            entries.append(&mut tick);
        }
        let shreds = entries_to_test_shreds(
            &entries,
            slot,
            slot - 1, // parent_slot
            true,     // is_full_slot
            0,        // version
            true,     // merkle_variant
        );
        blockstore.insert_shreds(shreds, None, false).unwrap();

        let mut write_batch = blockstore.db.batch().unwrap();
        blockstore
            .purge_special_columns_exact(&mut write_batch, slot, slot + 1)
            .unwrap();
    }
}
