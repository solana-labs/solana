use super::*;

impl Blockstore {
    /// Silently deletes all blockstore column families in the range [from_slot,to_slot]
    /// Dangerous; Use with care:
    /// Does not check for integrity and does not update slot metas that refer to deleted slots
    /// Modifies multiple column families simultaneously
    pub fn purge_slots_with_delay(
        &self,
        from_slot: Slot,
        to_slot: Slot,
        delay_between_purges: Option<Duration>,
        purge_type: PurgeType,
    ) {
        // if there's no upper bound, split the purge request into batches of 1000 slots
        const PURGE_BATCH_SIZE: u64 = 1000;
        let mut batch_start = from_slot;
        while batch_start < to_slot {
            let batch_end = (batch_start + PURGE_BATCH_SIZE).min(to_slot);
            match self.run_purge(batch_start, batch_end, purge_type) {
                Ok(_all_columns_purged) => {
                    batch_start = batch_end;

                    if let Some(ref duration) = delay_between_purges {
                        // Cooperate with other blockstore users
                        std::thread::sleep(*duration);
                    }
                }
                Err(e) => {
                    error!(
                        "Error: {:?}; Purge failed in range {:?} to {:?}",
                        e, batch_start, batch_end
                    );
                    break;
                }
            }
        }

        if !self.no_compaction {
            if let Err(e) = self.compact_storage(from_slot, to_slot) {
                // This error is not fatal and indicates an internal error
                error!(
                    "Error: {:?}; Couldn't compact storage from {:?} to {:?}",
                    e, from_slot, to_slot
                );
            }
        }
    }

    pub fn purge_slots(&self, from_slot: Slot, to_slot: Slot) {
        self.purge_slots_with_delay(from_slot, to_slot, None, PurgeType::Exact)
    }

    /// Ensures that the SlotMeta::next_slots vector for all slots contain no references in the
    /// [from_slot,to_slot] range
    ///
    /// Dangerous; Use with care
    pub fn purge_from_next_slots(&self, from_slot: Slot, to_slot: Slot) {
        for (slot, mut meta) in self
            .slot_meta_iterator(0)
            .expect("unable to iterate over meta")
        {
            if slot > to_slot {
                break;
            }

            let original_len = meta.next_slots.len();
            meta.next_slots
                .retain(|slot| *slot < from_slot || *slot > to_slot);
            if meta.next_slots.len() != original_len {
                info!("purge_from_next_slots: adjusted meta for slot {}", slot);
                self.put_meta_bytes(
                    slot,
                    &bincode::serialize(&meta).expect("couldn't update meta"),
                )
                .expect("couldn't update meta");
            }
        }
    }

    // Returns whether or not all columns successfully purged the slot range
    pub(crate) fn run_purge(
        &self,
        from_slot: Slot,
        to_slot: Slot,
        purge_type: PurgeType,
    ) -> Result<bool> {
        let mut write_batch = self
            .db
            .batch()
            .expect("Database Error: Failed to get write batch");
        // delete range cf is not inclusive
        let to_slot = to_slot.checked_add(1).unwrap_or_else(|| std::u64::MAX);

        let mut delete_range_timer = Measure::start("delete_range");
        let mut columns_purged = self
            .db
            .delete_range_cf::<cf::SlotMeta>(&mut write_batch, from_slot, to_slot)
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
        datapoint_info!(
            "blockstore-purge",
            ("from_slot", from_slot as i64, i64),
            ("to_slot", to_slot as i64, i64),
            ("delete_range_us", delete_range_timer.as_us() as i64, i64),
            ("write_batch_us", write_timer.as_us() as i64, i64)
        );
        Ok(columns_purged)
    }

    pub fn compact_storage(&self, from_slot: Slot, to_slot: Slot) -> Result<bool> {
        info!("compact_storage: from {} to {}", from_slot, to_slot);
        let mut compact_timer = Measure::start("compact_range");
        let result = self
            .meta_cf
            .compact_range(from_slot, to_slot)
            .unwrap_or(false)
            && self
                .db
                .column::<cf::Root>()
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .data_shred_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .code_shred_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .dead_slots_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .duplicate_slots_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .erasure_meta_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .orphans_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .index_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false)
            && self
                .transaction_status_cf
                .compact_range(0, 2)
                .unwrap_or(false)
            && self
                .address_signatures_cf
                .compact_range(0, 2)
                .unwrap_or(false)
            && self
                .transaction_status_index_cf
                .compact_range(0, 2)
                .unwrap_or(false)
            && self
                .rewards_cf
                .compact_range(from_slot, to_slot)
                .unwrap_or(false);
        compact_timer.stop();
        if !result {
            info!("compact_storage incomplete");
        }
        datapoint_info!(
            "blockstore-compact",
            ("compact_range_us", compact_timer.as_us() as i64, i64),
        );
        Ok(result)
    }

    fn purge_special_columns_exact(
        &self,
        batch: &mut WriteBatch,
        from_slot: Slot,
        to_slot: Slot, // Exclusive
    ) -> Result<()> {
        let mut index0 = self.transaction_status_index_cf.get(0)?.unwrap_or_default();
        let mut index1 = self.transaction_status_index_cf.get(1)?.unwrap_or_default();
        for slot in from_slot..to_slot {
            let slot_entries = self.get_any_valid_slot_entries(slot, 0);
            for transaction in slot_entries
                .iter()
                .cloned()
                .flat_map(|entry| entry.transactions)
            {
                batch.delete::<cf::TransactionStatus>((0, transaction.signatures[0], slot))?;
                batch.delete::<cf::TransactionStatus>((1, transaction.signatures[0], slot))?;
                for pubkey in transaction.message.account_keys {
                    batch.delete::<cf::AddressSignatures>((
                        0,
                        pubkey,
                        slot,
                        transaction.signatures[0],
                    ))?;
                    batch.delete::<cf::AddressSignatures>((
                        1,
                        pubkey,
                        slot,
                        transaction.signatures[0],
                    ))?;
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

    fn purge_special_columns_with_primary_index(
        &self,
        write_batch: &mut WriteBatch,
        columns_purged: &mut bool,
        w_active_transaction_status_index: &mut u64,
        to_slot: Slot,
    ) -> Result<()> {
        if let Some(index) = self.toggle_transaction_status_index(
            write_batch,
            w_active_transaction_status_index,
            to_slot,
        )? {
            *columns_purged &= self
                .db
                .delete_range_cf::<cf::TransactionStatus>(write_batch, index, index + 1)
                .is_ok()
                & self
                    .db
                    .delete_range_cf::<cf::AddressSignatures>(write_batch, index, index + 1)
                    .is_ok();
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::{blockstore::tests::make_slot_entries_with_transactions, get_tmp_ledger_path};

    // check that all columns are either empty or start at `min_slot`
    fn test_all_empty_or_min(blockstore: &Blockstore, min_slot: Slot) {
        let condition_met = blockstore
            .db
            .iter::<cf::SlotMeta>(IteratorMode::Start)
            .unwrap()
            .next()
            .map(|(slot, _)| slot >= min_slot)
            .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::Root>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|(slot, _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::ShredData>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|((slot, _), _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::ShredCode>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|((slot, _), _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::DeadSlots>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|(slot, _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::DuplicateSlots>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|(slot, _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::ErasureMeta>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|((slot, _), _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::Orphans>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|(slot, _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::Index>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|(slot, _)| slot >= min_slot)
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::TransactionStatus>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|((primary_index, _, slot), _)| {
                    slot >= min_slot || (primary_index == 2 && slot == 0)
                })
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::AddressSignatures>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|((primary_index, _, slot, _), _)| {
                    slot >= min_slot || (primary_index == 2 && slot == 0)
                })
                .unwrap_or(true)
            & blockstore
                .db
                .iter::<cf::Rewards>(IteratorMode::Start)
                .unwrap()
                .next()
                .map(|(slot, _)| slot >= min_slot)
                .unwrap_or(true);
        assert!(condition_met);
    }

    #[test]
    fn test_purge_slots() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 50, 5);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        blockstore.purge_slots(0, 5);

        test_all_empty_or_min(&blockstore, 6);

        blockstore.purge_slots(0, 50);

        // min slot shouldn't matter, blockstore should be empty
        test_all_empty_or_min(&blockstore, 100);
        test_all_empty_or_min(&blockstore, 0);

        blockstore
            .slot_meta_iterator(0)
            .unwrap()
            .for_each(|(_, _)| {
                panic!();
            });

        drop(blockstore);
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_purge_huge() {
        let blockstore_path = get_tmp_ledger_path!();
        let blockstore = Blockstore::open(&blockstore_path).unwrap();
        let (shreds, _) = make_many_slot_entries(0, 5000, 10);
        blockstore.insert_shreds(shreds, None, false).unwrap();

        blockstore.purge_slots(0, 4999);

        test_all_empty_or_min(&blockstore, 5000);

        drop(blockstore);
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    fn test_purge_front_of_ledger() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();
            let max_slot = 10;
            for x in 0..max_slot {
                let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
                blockstore
                    .write_transaction_status(
                        x,
                        Signature::new(&random_bytes),
                        vec![&Pubkey::new(&random_bytes[0..32])],
                        vec![&Pubkey::new(&random_bytes[32..])],
                        &TransactionStatusMeta::default(),
                    )
                    .unwrap();
            }
            // Purge to freeze index 0
            blockstore.run_purge(0, 1, PurgeType::PrimaryIndex).unwrap();

            for x in max_slot..2 * max_slot {
                let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
                blockstore
                    .write_transaction_status(
                        x,
                        Signature::new(&random_bytes),
                        vec![&Pubkey::new(&random_bytes[0..32])],
                        vec![&Pubkey::new(&random_bytes[32..])],
                        &TransactionStatusMeta::default(),
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
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    #[test]
    #[allow(clippy::cognitive_complexity)]
    fn test_purge_transaction_status() {
        let blockstore_path = get_tmp_ledger_path!();
        {
            let blockstore = Blockstore::open(&blockstore_path).unwrap();
            let transaction_status_index_cf = blockstore.db.column::<cf::TransactionStatusIndex>();
            let slot = 10;
            for _ in 0..5 {
                let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
                blockstore
                    .write_transaction_status(
                        slot,
                        Signature::new(&random_bytes),
                        vec![&Pubkey::new(&random_bytes[0..32])],
                        vec![&Pubkey::new(&random_bytes[32..])],
                        &TransactionStatusMeta::default(),
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
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }

    fn clear_and_repopulate_transaction_statuses(
        blockstore: &mut Blockstore,
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
            .delete_range_cf::<cf::TransactionStatus>(&mut write_batch, 0, 3)
            .unwrap();
        blockstore
            .db
            .delete_range_cf::<cf::TransactionStatusIndex>(&mut write_batch, 0, 3)
            .unwrap();
        blockstore.db.write(write_batch).unwrap();
        blockstore.initialize_transaction_status_index().unwrap();
        *blockstore.active_transaction_status_index.write().unwrap() = 0;

        for x in 0..index0_max_slot + 1 {
            let entries = make_slot_entries_with_transactions(1);
            let shreds = entries_to_test_shreds(entries.clone(), x, x.saturating_sub(1), true, 0);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let signature = entries
                .iter()
                .cloned()
                .filter(|entry| !entry.is_tick())
                .flat_map(|entry| entry.transactions)
                .map(|transaction| transaction.signatures[0])
                .collect::<Vec<Signature>>()[0];
            let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
            blockstore
                .write_transaction_status(
                    x,
                    signature,
                    vec![&Pubkey::new(&random_bytes[0..32])],
                    vec![&Pubkey::new(&random_bytes[32..])],
                    &TransactionStatusMeta::default(),
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
            let shreds = entries_to_test_shreds(entries.clone(), x, x.saturating_sub(1), true, 0);
            blockstore.insert_shreds(shreds, None, false).unwrap();
            let signature: Signature = entries
                .iter()
                .cloned()
                .filter(|entry| !entry.is_tick())
                .flat_map(|entry| entry.transactions)
                .map(|transaction| transaction.signatures[0])
                .collect::<Vec<Signature>>()[0];
            let random_bytes: Vec<u8> = (0..64).map(|_| rand::random::<u8>()).collect();
            blockstore
                .write_transaction_status(
                    x,
                    signature,
                    vec![&Pubkey::new(&random_bytes[0..32])],
                    vec![&Pubkey::new(&random_bytes[32..])],
                    &TransactionStatusMeta::default(),
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
        let blockstore_path = get_tmp_ledger_path!();
        {
            let mut blockstore = Blockstore::open(&blockstore_path).unwrap();
            let index0_max_slot = 9;
            let index1_max_slot = 19;

            // Test purge outside bounds
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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

            // Test purge all
            clear_and_repopulate_transaction_statuses(
                &mut blockstore,
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
            drop(status_entry_iterator);
        }
        Blockstore::destroy(&blockstore_path).expect("Expected successful database destruction");
    }
}
