use super::{
    broadcast_utils::{self, ReceiveResults},
    *,
};
use crate::broadcast_stage::broadcast_utils::UnfinishedSlotInfo;
use solana_ledger::{
    entry::Entry,
    shred::{Shred, Shredder, RECOMMENDED_FEC_RATE, SHRED_TICK_REFERENCE_MASK},
};
use solana_sdk::{pubkey::Pubkey, signature::Keypair, timing::duration_as_us};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub struct StandardBroadcastRun {
    process_shreds_stats: ProcessShredsStats,
    transmit_shreds_stats: Arc<Mutex<SlotBroadcastStats<TransmitShredsStats>>>,
    insert_shreds_stats: Arc<Mutex<SlotBroadcastStats<InsertShredsStats>>>,
    unfinished_slot: Option<UnfinishedSlotInfo>,
    current_slot_and_parent: Option<(u64, u64)>,
    slot_broadcast_start: Option<Instant>,
    keypair: Arc<Keypair>,
    shred_version: u16,
    last_datapoint_submit: Arc<AtomicU64>,
    num_batches: usize,
}

impl StandardBroadcastRun {
    pub(super) fn new(keypair: Arc<Keypair>, shred_version: u16) -> Self {
        Self {
            process_shreds_stats: ProcessShredsStats::default(),
            transmit_shreds_stats: Arc::new(Mutex::new(SlotBroadcastStats::default())),
            insert_shreds_stats: Arc::new(Mutex::new(SlotBroadcastStats::default())),
            unfinished_slot: None,
            current_slot_and_parent: None,
            slot_broadcast_start: None,
            keypair,
            shred_version,
            last_datapoint_submit: Arc::new(AtomicU64::new(0)),
            num_batches: 0,
        }
    }

    fn check_for_interrupted_slot(&mut self, max_ticks_in_slot: u8) -> Option<Shred> {
        let (slot, _) = self.current_slot_and_parent.unwrap();
        let mut last_unfinished_slot_shred = self
            .unfinished_slot
            .map(|last_unfinished_slot| {
                if last_unfinished_slot.slot != slot {
                    self.report_and_reset_stats();
                    Some(Shred::new_from_data(
                        last_unfinished_slot.slot,
                        last_unfinished_slot.next_shred_index,
                        (last_unfinished_slot.slot - last_unfinished_slot.parent) as u16,
                        None,
                        true,
                        true,
                        max_ticks_in_slot & SHRED_TICK_REFERENCE_MASK,
                        self.shred_version,
                        last_unfinished_slot.next_shred_index,
                    ))
                } else {
                    None
                }
            })
            .unwrap_or(None);

        // This shred should only be Some if the previous slot was interrupted
        if let Some(ref mut shred) = last_unfinished_slot_shred {
            Shredder::sign_shred(&self.keypair, shred);
            self.unfinished_slot = None;
        }

        last_unfinished_slot_shred
    }
    fn init_shredder(&self, blockstore: &Blockstore, reference_tick: u8) -> (Shredder, u32) {
        let (slot, parent_slot) = self.current_slot_and_parent.unwrap();
        let next_shred_index = self
            .unfinished_slot
            .map(|s| s.next_shred_index)
            .unwrap_or_else(|| {
                blockstore
                    .meta(slot)
                    .expect("Database error")
                    .map(|meta| meta.consumed)
                    .unwrap_or(0) as u32
            });
        (
            Shredder::new(
                slot,
                parent_slot,
                RECOMMENDED_FEC_RATE,
                self.keypair.clone(),
                reference_tick,
                self.shred_version,
            )
            .expect("Expected to create a new shredder"),
            next_shred_index,
        )
    }
    fn entries_to_data_shreds(
        &mut self,
        shredder: &Shredder,
        next_shred_index: u32,
        entries: &[Entry],
        is_slot_end: bool,
    ) -> Vec<Shred> {
        let (data_shreds, new_next_shred_index) =
            shredder.entries_to_data_shreds(entries, is_slot_end, next_shred_index);

        self.unfinished_slot = Some(UnfinishedSlotInfo {
            next_shred_index: new_next_shred_index,
            slot: shredder.slot,
            parent: shredder.parent_slot,
        });

        data_shreds
    }

    #[cfg(test)]
    fn test_process_receive_results(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
        blockstore: &Arc<Blockstore>,
        receive_results: ReceiveResults,
    ) -> Result<()> {
        let (bsend, brecv) = channel();
        let (ssend, srecv) = channel();
        self.process_receive_results(&blockstore, &ssend, &bsend, receive_results)?;
        let srecv = Arc::new(Mutex::new(srecv));
        let brecv = Arc::new(Mutex::new(brecv));
        //data
        let _ = self.transmit(&srecv, cluster_info, sock);
        let _ = self.record(&brecv, blockstore);
        //coding
        let _ = self.transmit(&srecv, cluster_info, sock);
        let _ = self.record(&brecv, blockstore);
        Ok(())
    }

    fn process_receive_results(
        &mut self,
        blockstore: &Arc<Blockstore>,
        socket_sender: &Sender<(TransmitShreds, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
        receive_results: ReceiveResults,
    ) -> Result<()> {
        let mut receive_elapsed = receive_results.time_elapsed;
        let num_entries = receive_results.entries.len();
        let bank = receive_results.bank.clone();
        let last_tick_height = receive_results.last_tick_height;
        inc_new_counter_info!("broadcast_service-entries_received", num_entries);
        let old_broadcast_start = self.slot_broadcast_start;
        let old_num_batches = self.num_batches;
        if self.current_slot_and_parent.is_none()
            || bank.slot() != self.current_slot_and_parent.unwrap().0
        {
            self.slot_broadcast_start = Some(Instant::now());
            self.num_batches = 0;
            let slot = bank.slot();
            let parent_slot = bank.parent_slot();

            self.current_slot_and_parent = Some((slot, parent_slot));
            receive_elapsed = Duration::new(0, 0);
        }

        let to_shreds_start = Instant::now();

        // 1) Check if slot was interrupted
        let last_unfinished_slot_shred =
            self.check_for_interrupted_slot(bank.ticks_per_slot() as u8);

        // 2) Convert entries to shreds and coding shreds
        let (shredder, next_shred_index) = self.init_shredder(
            blockstore,
            (bank.tick_height() % bank.ticks_per_slot()) as u8,
        );
        let is_last_in_slot = last_tick_height == bank.max_tick_height();
        let data_shreds = self.entries_to_data_shreds(
            &shredder,
            next_shred_index,
            &receive_results.entries,
            is_last_in_slot,
        );
        // Insert the first shred so blockstore stores that the leader started this block
        // This must be done before the blocks are sent out over the wire.
        if !data_shreds.is_empty() && data_shreds[0].index() == 0 {
            let first = vec![data_shreds[0].clone()];
            blockstore
                .insert_shreds(first, None, true)
                .expect("Failed to insert shreds in blockstore");
        }
        let last_data_shred = data_shreds.len();
        let to_shreds_elapsed = to_shreds_start.elapsed();

        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
        let stakes = stakes.map(Arc::new);

        // Broadcast the last shred of the interrupted slot if necessary
        if let Some(last_shred) = last_unfinished_slot_shred {
            let batch_info = Some(BroadcastShredBatchInfo {
                slot: last_shred.slot(),
                num_expected_batches: Some(old_num_batches + 1),
                slot_start_ts: old_broadcast_start.expect(
                    "Old broadcast start time for previous slot must exist if the previous slot
                 was interrupted",
                ),
            });
            let last_shred = Arc::new(vec![last_shred]);
            socket_sender.send(((stakes.clone(), last_shred.clone()), batch_info.clone()))?;
            blockstore_sender.send((last_shred, batch_info))?;
        }

        // Increment by two batches, one for the data batch, one for the coding batch.
        self.num_batches += 2;
        let num_expected_batches = {
            if is_last_in_slot {
                Some(self.num_batches)
            } else {
                None
            }
        };
        let batch_info = Some(BroadcastShredBatchInfo {
            slot: bank.slot(),
            num_expected_batches,
            slot_start_ts: self
                .slot_broadcast_start
                .clone()
                .expect("Start timestamp must exist for a slot if we're broadcasting the slot"),
        });

        let data_shreds = Arc::new(data_shreds);
        socket_sender.send(((stakes.clone(), data_shreds.clone()), batch_info.clone()))?;
        blockstore_sender.send((data_shreds.clone(), batch_info.clone()))?;
        let coding_shreds = shredder.data_shreds_to_coding_shreds(&data_shreds[0..last_data_shred]);
        let coding_shreds = Arc::new(coding_shreds);
        socket_sender.send(((stakes, coding_shreds.clone()), batch_info.clone()))?;
        blockstore_sender.send((coding_shreds, batch_info))?;
        self.process_shreds_stats.update(&ProcessShredsStats {
            shredding_elapsed: duration_as_us(&to_shreds_elapsed),
            receive_elapsed: duration_as_us(&receive_elapsed),
        });
        if last_tick_height == bank.max_tick_height() {
            self.report_and_reset_stats();
            self.unfinished_slot = None;
        }

        Ok(())
    }

    fn insert(
        &mut self,
        blockstore: &Arc<Blockstore>,
        shreds: Arc<Vec<Shred>>,
        broadcast_shred_batch_info: Option<BroadcastShredBatchInfo>,
    ) -> Result<()> {
        // Insert shreds into blockstore
        let insert_shreds_start = Instant::now();
        // The first shred is inserted synchronously
        let data_shreds = if !shreds.is_empty() && shreds[0].index() == 0 {
            shreds[1..].to_vec()
        } else {
            shreds.to_vec()
        };
        blockstore
            .insert_shreds(data_shreds, None, true)
            .expect("Failed to insert shreds in blockstore");
        let insert_shreds_elapsed = insert_shreds_start.elapsed();
        let new_insert_shreds_stats = InsertShredsStats {
            insert_shreds_elapsed: duration_as_us(&insert_shreds_elapsed),
            num_shreds: shreds.len(),
        };
        self.update_insertion_metrics(&new_insert_shreds_stats, &broadcast_shred_batch_info);
        Ok(())
    }

    fn update_insertion_metrics(
        &mut self,
        new_insertion_shreds_stats: &InsertShredsStats,
        broadcast_shred_batch_info: &Option<BroadcastShredBatchInfo>,
    ) {
        let mut insert_shreds_stats = self.insert_shreds_stats.lock().unwrap();
        insert_shreds_stats.update(new_insertion_shreds_stats, broadcast_shred_batch_info);
    }

    fn broadcast(
        &mut self,
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        stakes: Option<Arc<HashMap<Pubkey, u64>>>,
        shreds: Arc<Vec<Shred>>,
        broadcast_shred_batch_info: Option<BroadcastShredBatchInfo>,
    ) -> Result<()> {
        trace!("Broadcasting {:?} shreds", shreds.len());
        // Get the list of peers to broadcast to
        let get_peers_start = Instant::now();
        let (peers, peers_and_stakes) = get_broadcast_peers(cluster_info, stakes);
        let get_peers_elapsed = get_peers_start.elapsed();

        // Broadcast the shreds
        let transmit_start = Instant::now();
        let mut send_mmsg_total = 0;
        broadcast_shreds(
            sock,
            &shreds,
            &peers_and_stakes,
            &peers,
            &self.last_datapoint_submit,
            &mut send_mmsg_total,
        )?;
        let transmit_elapsed = transmit_start.elapsed();
        let new_transmit_shreds_stats = TransmitShredsStats {
            transmit_elapsed: duration_as_us(&transmit_elapsed),
            get_peers_elapsed: duration_as_us(&get_peers_elapsed),
            send_mmsg_elapsed: send_mmsg_total,
            num_shreds: shreds.len(),
        };

        // Process metrics
        self.update_transmit_metrics(&new_transmit_shreds_stats, &broadcast_shred_batch_info);
        Ok(())
    }

    fn update_transmit_metrics(
        &mut self,
        new_transmit_shreds_stats: &TransmitShredsStats,
        broadcast_shred_batch_info: &Option<BroadcastShredBatchInfo>,
    ) {
        let mut transmit_shreds_stats = self.transmit_shreds_stats.lock().unwrap();
        transmit_shreds_stats.update(new_transmit_shreds_stats, broadcast_shred_batch_info);
    }

    fn report_and_reset_stats(&mut self) {
        let stats = &self.process_shreds_stats;
        assert!(self.unfinished_slot.is_some());
        datapoint_info!(
            "broadcast-process-shreds-stats",
            ("slot", self.unfinished_slot.unwrap().slot as i64, i64),
            ("shredding_time", stats.shredding_elapsed as i64, i64),
            ("receive_time", stats.receive_elapsed as i64, i64),
            (
                "num_data_shreds",
                i64::from(self.unfinished_slot.unwrap().next_shred_index),
                i64
            ),
            (
                "slot_broadcast_time",
                self.slot_broadcast_start.unwrap().elapsed().as_micros() as i64,
                i64
            ),
        );
        self.process_shreds_stats.reset();
    }
}

impl BroadcastRun for StandardBroadcastRun {
    fn run(
        &mut self,
        blockstore: &Arc<Blockstore>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(TransmitShreds, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()> {
        let receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        self.process_receive_results(
            blockstore,
            socket_sender,
            blockstore_sender,
            receive_results,
        )
    }
    fn transmit(
        &mut self,
        receiver: &Arc<Mutex<TransmitReceiver>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
    ) -> Result<()> {
        let ((stakes, shreds), slot_start_ts) = receiver.lock().unwrap().recv()?;
        self.broadcast(sock, cluster_info, stakes, shreds, slot_start_ts)
    }
    fn record(
        &mut self,
        receiver: &Arc<Mutex<RecordReceiver>>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<()> {
        let (shreds, slot_start_ts) = receiver.lock().unwrap().recv()?;
        self.insert(blockstore, shreds, slot_start_ts)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::cluster_info::{ClusterInfo, Node};
    use solana_ledger::genesis_utils::create_genesis_config;
    use solana_ledger::{
        blockstore::Blockstore, entry::create_ticks, get_tmp_ledger_path,
        shred::max_ticks_per_n_shreds,
    };
    use solana_runtime::bank::Bank;
    use solana_sdk::{
        genesis_config::GenesisConfig,
        signature::{Keypair, Signer},
    };
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    fn setup(
        num_shreds_per_slot: Slot,
    ) -> (
        Arc<Blockstore>,
        GenesisConfig,
        Arc<RwLock<ClusterInfo>>,
        Arc<Bank>,
        Arc<Keypair>,
        UdpSocket,
    ) {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blockstore = Arc::new(
            Blockstore::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_info = Node::new_localhost_with_pubkey(&leader_pubkey);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            leader_info.info.clone(),
        )));
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut genesis_config = create_genesis_config(10_000).genesis_config;
        genesis_config.ticks_per_slot = max_ticks_per_n_shreds(num_shreds_per_slot) + 1;
        let bank0 = Arc::new(Bank::new(&genesis_config));
        (
            blockstore,
            genesis_config,
            cluster_info,
            bank0,
            leader_keypair,
            socket,
        )
    }

    #[test]
    fn test_interrupted_slot_last_shred() {
        let keypair = Arc::new(Keypair::new());
        let mut run = StandardBroadcastRun::new(keypair.clone(), 0);

        // Set up the slot to be interrupted
        let next_shred_index = 10;
        let slot = 1;
        let parent = 0;
        run.unfinished_slot = Some(UnfinishedSlotInfo {
            next_shred_index,
            slot,
            parent,
        });
        run.slot_broadcast_start = Some(Instant::now());

        // Set up a slot to interrupt the old slot
        run.current_slot_and_parent = Some((4, 2));

        // Slot 2 interrupted slot 1
        let shred = run
            .check_for_interrupted_slot(0)
            .expect("Expected a shred that signals an interrupt");

        // Validate the shred
        assert_eq!(shred.parent(), parent);
        assert_eq!(shred.slot(), slot);
        assert_eq!(shred.index(), next_shred_index);
        assert!(shred.is_data());
        assert!(shred.verify(&keypair.pubkey()));
    }

    #[test]
    fn test_slot_interrupt() {
        // Setup
        let num_shreds_per_slot = 2;
        let (blockstore, genesis_config, cluster_info, bank0, leader_keypair, socket) =
            setup(num_shreds_per_slot);

        // Insert 1 less than the number of ticks needed to finish the slot
        let ticks0 = create_ticks(genesis_config.ticks_per_slot - 1, 0, genesis_config.hash());
        let receive_results = ReceiveResults {
            entries: ticks0.clone(),
            time_elapsed: Duration::new(3, 0),
            bank: bank0.clone(),
            last_tick_height: (ticks0.len() - 1) as u64,
        };

        // Step 1: Make an incomplete transmission for slot 0
        let mut standard_broadcast_run = StandardBroadcastRun::new(leader_keypair.clone(), 0);
        standard_broadcast_run
            .test_process_receive_results(&cluster_info, &socket, &blockstore, receive_results)
            .unwrap();
        let unfinished_slot = standard_broadcast_run.unfinished_slot.as_ref().unwrap();
        assert_eq!(unfinished_slot.next_shred_index as u64, num_shreds_per_slot);
        assert_eq!(unfinished_slot.slot, 0);
        assert_eq!(unfinished_slot.parent, 0);
        // Make sure the slot is not complete
        assert!(!blockstore.is_full(0));
        // Modify the stats, should reset later
        standard_broadcast_run.process_shreds_stats.receive_elapsed = 10;
        // Broadcast stats should exist, and 2 batches should have been sent,
        // one for data, one for coding
        assert_eq!(
            standard_broadcast_run
                .transmit_shreds_stats
                .lock()
                .unwrap()
                .get(unfinished_slot.slot)
                .unwrap()
                .num_batches(),
            2
        );
        assert_eq!(
            standard_broadcast_run
                .insert_shreds_stats
                .lock()
                .unwrap()
                .get(unfinished_slot.slot)
                .unwrap()
                .num_batches(),
            2
        );
        // Try to fetch ticks from blockstore, nothing should break
        assert_eq!(blockstore.get_slot_entries(0, 0).unwrap(), ticks0);
        assert_eq!(
            blockstore.get_slot_entries(0, num_shreds_per_slot).unwrap(),
            vec![],
        );

        // Step 2: Make a transmission for another bank that interrupts the transmission for
        // slot 0
        let bank2 = Arc::new(Bank::new_from_parent(&bank0, &leader_keypair.pubkey(), 2));
        let interrupted_slot = unfinished_slot.slot;
        // Interrupting the slot should cause the unfinished_slot and stats to reset
        let num_shreds = 1;
        assert!(num_shreds < num_shreds_per_slot);
        let ticks1 = create_ticks(max_ticks_per_n_shreds(num_shreds), 0, genesis_config.hash());
        let receive_results = ReceiveResults {
            entries: ticks1.clone(),
            time_elapsed: Duration::new(2, 0),
            bank: bank2.clone(),
            last_tick_height: (ticks1.len() - 1) as u64,
        };
        standard_broadcast_run
            .test_process_receive_results(&cluster_info, &socket, &blockstore, receive_results)
            .unwrap();
        let unfinished_slot = standard_broadcast_run.unfinished_slot.as_ref().unwrap();

        // The shred index should have reset to 0, which makes it possible for the
        // index < the previous shred index for slot 0
        assert_eq!(unfinished_slot.next_shred_index as u64, num_shreds);
        assert_eq!(unfinished_slot.slot, 2);
        assert_eq!(unfinished_slot.parent, 0);

        // Check that the stats were reset as well
        assert_eq!(
            standard_broadcast_run.process_shreds_stats.receive_elapsed,
            0
        );

        // Broadcast stats for interrupted slot should be cleared
        assert!(standard_broadcast_run
            .transmit_shreds_stats
            .lock()
            .unwrap()
            .get(interrupted_slot)
            .is_none());
        assert!(standard_broadcast_run
            .insert_shreds_stats
            .lock()
            .unwrap()
            .get(interrupted_slot)
            .is_none());

        // Try to fetch the incomplete ticks from blockstore, should succeed
        assert_eq!(blockstore.get_slot_entries(0, 0).unwrap(), ticks0);
        assert_eq!(
            blockstore.get_slot_entries(0, num_shreds_per_slot).unwrap(),
            vec![],
        );
    }

    #[test]
    fn test_slot_finish() {
        // Setup
        let num_shreds_per_slot = 2;
        let (blockstore, genesis_config, cluster_info, bank0, leader_keypair, socket) =
            setup(num_shreds_per_slot);

        // Insert complete slot of ticks needed to finish the slot
        let ticks = create_ticks(genesis_config.ticks_per_slot, 0, genesis_config.hash());
        let receive_results = ReceiveResults {
            entries: ticks.clone(),
            time_elapsed: Duration::new(3, 0),
            bank: bank0.clone(),
            last_tick_height: ticks.len() as u64,
        };

        let mut standard_broadcast_run = StandardBroadcastRun::new(leader_keypair, 0);
        standard_broadcast_run
            .test_process_receive_results(&cluster_info, &socket, &blockstore, receive_results)
            .unwrap();
        assert!(standard_broadcast_run.unfinished_slot.is_none())
    }
}
