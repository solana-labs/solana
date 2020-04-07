use super::broadcast_utils::{self, ReceiveResults};
use super::*;
use crate::broadcast_stage::broadcast_utils::UnfinishedSlotInfo;
use solana_ledger::{
    entry::Entry,
    shred::{Shred, Shredder, RECOMMENDED_FEC_RATE, SHRED_TICK_REFERENCE_MASK},
};
use solana_sdk::{pubkey::Pubkey, signature::Keypair, timing::duration_as_us};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Default)]
struct BroadcastStats {
    // Per-slot elapsed time
    shredding_elapsed: u64,
    insert_shreds_elapsed: u64,
    broadcast_elapsed: u64,
    receive_elapsed: u64,
    seed_elapsed: u64,
    send_mmsg_elapsed: u64,
}

impl BroadcastStats {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone)]
pub struct StandardBroadcastRun {
    stats: Arc<RwLock<BroadcastStats>>,
    unfinished_slot: Option<UnfinishedSlotInfo>,
    current_slot_and_parent: Option<(u64, u64)>,
    slot_broadcast_start: Option<Instant>,
    keypair: Arc<Keypair>,
    shred_version: u16,
    last_datapoint_submit: Instant,
}

impl StandardBroadcastRun {
    pub(super) fn new(keypair: Arc<Keypair>, shred_version: u16) -> Self {
        Self {
            stats: Arc::new(RwLock::new(BroadcastStats::default())),
            unfinished_slot: None,
            current_slot_and_parent: None,
            slot_broadcast_start: None,
            keypair,
            shred_version,
            last_datapoint_submit: Instant::now(),
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
        //coding
        let _ = self.transmit(&srecv, cluster_info, sock);
        let _ = self.record(&brecv, blockstore);
        Ok(())
    }

    fn process_receive_results(
        &mut self,
        blockstore: &Arc<Blockstore>,
        socket_sender: &Sender<TransmitShreds>,
        blockstore_sender: &Sender<Arc<Vec<Shred>>>,
        receive_results: ReceiveResults,
    ) -> Result<()> {
        let mut receive_elapsed = receive_results.time_elapsed;
        let num_entries = receive_results.entries.len();
        let bank = receive_results.bank.clone();
        let last_tick_height = receive_results.last_tick_height;
        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        if self.current_slot_and_parent.is_none()
            || bank.slot() != self.current_slot_and_parent.unwrap().0
        {
            self.slot_broadcast_start = Some(Instant::now());
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
        let mut data_shreds = self.entries_to_data_shreds(
            &shredder,
            next_shred_index,
            &receive_results.entries,
            last_tick_height == bank.max_tick_height(),
        );
        //Insert the first shred so blockstore stores that the leader started this block
        //This must be done before the blocks are sent out over the wire.
        if !data_shreds.is_empty() && data_shreds[0].index() == 0 {
            let first = vec![data_shreds[0].clone()];
            blockstore
                .insert_shreds(first, None, true)
                .expect("Failed to insert shreds in blockstore");
        }
        let last_data_shred = data_shreds.len();
        if let Some(last_shred) = last_unfinished_slot_shred {
            data_shreds.push(last_shred);
        }
        let to_shreds_elapsed = to_shreds_start.elapsed();

        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
        let stakes = stakes.map(Arc::new);
        let data_shreds = Arc::new(data_shreds);
        socket_sender.send((stakes.clone(), data_shreds.clone()))?;
        blockstore_sender.send(data_shreds.clone())?;
        let coding_shreds = shredder.data_shreds_to_coding_shreds(&data_shreds[0..last_data_shred]);
        let coding_shreds = Arc::new(coding_shreds);
        socket_sender.send((stakes, coding_shreds.clone()))?;
        blockstore_sender.send(coding_shreds)?;
        self.update_broadcast_stats(BroadcastStats {
            shredding_elapsed: duration_as_us(&to_shreds_elapsed),
            receive_elapsed: duration_as_us(&receive_elapsed),
            ..BroadcastStats::default()
        });

        if last_tick_height == bank.max_tick_height() {
            self.report_and_reset_stats();
            self.unfinished_slot = None;
        }

        Ok(())
    }

    fn insert(&self, blockstore: &Arc<Blockstore>, shreds: Arc<Vec<Shred>>) -> Result<()> {
        // Insert shreds into blockstore
        let insert_shreds_start = Instant::now();
        //The first shred is inserted synchronously
        let data_shreds = if !shreds.is_empty() && shreds[0].index() == 0 {
            shreds[1..].to_vec()
        } else {
            shreds.to_vec()
        };
        blockstore
            .insert_shreds(data_shreds, None, true)
            .expect("Failed to insert shreds in blockstore");
        let insert_shreds_elapsed = insert_shreds_start.elapsed();
        self.update_broadcast_stats(BroadcastStats {
            insert_shreds_elapsed: duration_as_us(&insert_shreds_elapsed),
            ..BroadcastStats::default()
        });
        Ok(())
    }

    fn broadcast(
        &mut self,
        sock: &UdpSocket,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        stakes: Option<Arc<HashMap<Pubkey, u64>>>,
        shreds: Arc<Vec<Shred>>,
    ) -> Result<()> {
        let seed_start = Instant::now();
        let seed_elapsed = seed_start.elapsed();

        // Broadcast the shreds
        let broadcast_start = Instant::now();
        trace!("Broadcasting {:?} shreds", shreds.len());

        let (peers, peers_and_stakes) = get_broadcast_peers(cluster_info, stakes);

        let mut send_mmsg_total = 0;
        broadcast_shreds(
            sock,
            &shreds,
            &peers_and_stakes,
            &peers,
            &mut self.last_datapoint_submit,
            &mut send_mmsg_total,
        )?;

        let broadcast_elapsed = broadcast_start.elapsed();

        self.update_broadcast_stats(BroadcastStats {
            broadcast_elapsed: duration_as_us(&broadcast_elapsed),
            seed_elapsed: duration_as_us(&seed_elapsed),
            send_mmsg_elapsed: send_mmsg_total,
            ..BroadcastStats::default()
        });
        Ok(())
    }

    fn update_broadcast_stats(&self, stats: BroadcastStats) {
        let mut wstats = self.stats.write().unwrap();
        wstats.receive_elapsed += stats.receive_elapsed;
        wstats.shredding_elapsed += stats.shredding_elapsed;
        wstats.insert_shreds_elapsed += stats.insert_shreds_elapsed;
        wstats.broadcast_elapsed += stats.broadcast_elapsed;
        wstats.seed_elapsed += stats.seed_elapsed;
        wstats.send_mmsg_elapsed += stats.send_mmsg_elapsed;
    }

    fn report_and_reset_stats(&mut self) {
        let stats = self.stats.read().unwrap();
        assert!(self.unfinished_slot.is_some());
        datapoint_info!(
            "broadcast-bank-stats",
            ("slot", self.unfinished_slot.unwrap().slot as i64, i64),
            ("shredding_time", stats.shredding_elapsed as i64, i64),
            ("insertion_time", stats.insert_shreds_elapsed as i64, i64),
            ("broadcast_time", stats.broadcast_elapsed as i64, i64),
            ("receive_time", stats.receive_elapsed as i64, i64),
            ("send_mmsg", stats.send_mmsg_elapsed as i64, i64),
            ("seed", stats.seed_elapsed as i64, i64),
            (
                "num_shreds",
                i64::from(self.unfinished_slot.unwrap().next_shred_index),
                i64
            ),
            (
                "slot_broadcast_time",
                self.slot_broadcast_start.unwrap().elapsed().as_micros() as i64,
                i64
            ),
        );
        drop(stats);
        self.stats.write().unwrap().reset();
    }
}

impl BroadcastRun for StandardBroadcastRun {
    fn run(
        &mut self,
        blockstore: &Arc<Blockstore>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<TransmitShreds>,
        blockstore_sender: &Sender<Arc<Vec<Shred>>>,
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
        receiver: &Arc<Mutex<Receiver<TransmitShreds>>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
    ) -> Result<()> {
        let (stakes, shreds) = receiver.lock().unwrap().recv()?;
        self.broadcast(sock, cluster_info, stakes, shreds)
    }
    fn record(
        &self,
        receiver: &Arc<Mutex<Receiver<Arc<Vec<Shred>>>>>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<()> {
        let shreds = receiver.lock().unwrap().recv()?;
        self.insert(blockstore, shreds)
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
        standard_broadcast_run
            .stats
            .write()
            .unwrap()
            .receive_elapsed = 10;

        // Try to fetch ticks from blockstore, nothing should break
        assert_eq!(blockstore.get_slot_entries(0, 0, None).unwrap(), ticks0);
        assert_eq!(
            blockstore
                .get_slot_entries(0, num_shreds_per_slot, None)
                .unwrap(),
            vec![],
        );

        // Step 2: Make a transmission for another bank that interrupts the transmission for
        // slot 0
        let bank2 = Arc::new(Bank::new_from_parent(&bank0, &leader_keypair.pubkey(), 2));

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
            standard_broadcast_run.stats.read().unwrap().receive_elapsed,
            0
        );

        // Try to fetch the incomplete ticks from blockstore, should succeed
        assert_eq!(blockstore.get_slot_entries(0, 0, None).unwrap(), ticks0);
        assert_eq!(
            blockstore
                .get_slot_entries(0, num_shreds_per_slot, None)
                .unwrap(),
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
