use super::broadcast_utils::{self, ReceiveResults};
use super::*;
use crate::broadcast_stage::broadcast_utils::UnfinishedSlotInfo;
use crate::entry::Entry;
use crate::shred::{Shred, Shredder, RECOMMENDED_FEC_RATE};
use solana_sdk::signature::Keypair;
use solana_sdk::timing::duration_as_us;
use std::sync::mpsc::Sender;
use std::time::Duration;

#[derive(Default)]
struct BroadcastStats {
    // Per-slot elapsed time
    shredding_elapsed: u64,
    broadcast_elapsed: u64,
    receive_elapsed: u64,
    clone_and_seed_elapsed: u64,
}

impl BroadcastStats {
    fn reset(&mut self) {
        self.shredding_elapsed = 0;
        self.broadcast_elapsed = 0;
        self.receive_elapsed = 0;
        self.clone_and_seed_elapsed = 0;
    }
}

pub(super) struct StandardBroadcastRun {
    stats: BroadcastStats,
    unfinished_slot: Option<UnfinishedSlotInfo>,
    current_slot_and_parent: Option<(u64, u64)>,
    slot_broadcast_start: Option<Instant>,
    keypair: Arc<Keypair>,
}

impl StandardBroadcastRun {
    pub(super) fn new(keypair: Arc<Keypair>) -> Self {
        Self {
            stats: BroadcastStats::default(),
            unfinished_slot: None,
            current_slot_and_parent: None,
            slot_broadcast_start: None,
            keypair,
        }
    }

    fn check_for_interrupted_slot(&mut self) -> Option<Shred> {
        let (slot, _) = self.current_slot_and_parent.unwrap();
        let last_unfinished_slot_shred = self
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
                    ))
                } else {
                    None
                }
            })
            .unwrap_or(None);

        // This shred should only be Some if the previous slot was interrupted
        if last_unfinished_slot_shred.is_some() {
            self.unfinished_slot = None;
        }

        last_unfinished_slot_shred
    }

    fn coalesce_shreds(
        data_shreds: Vec<Shred>,
        coding_shreds: Vec<Shred>,
        last_unfinished_slot_shred: Option<Shred>,
    ) -> Vec<Shred> {
        if let Some(shred) = last_unfinished_slot_shred {
            data_shreds
                .iter()
                .chain(coding_shreds.iter())
                .cloned()
                .chain(std::iter::once(shred))
                .collect::<Vec<_>>()
        } else {
            data_shreds
                .iter()
                .chain(coding_shreds.iter())
                .cloned()
                .collect::<Vec<_>>()
        }
    }

    fn entries_to_shreds(
        &mut self,
        blocktree: &Blocktree,
        entries: &[Entry],
        is_slot_end: bool,
    ) -> (Vec<Shred>, Vec<Shred>) {
        let (slot, parent_slot) = self.current_slot_and_parent.unwrap();
        let shredder = Shredder::new(
            slot,
            parent_slot,
            RECOMMENDED_FEC_RATE,
            self.keypair.clone(),
        )
        .expect("Expected to create a new shredder");

        let next_shred_index = self
            .unfinished_slot
            .map(|s| s.next_shred_index)
            .unwrap_or_else(|| {
                blocktree
                    .meta(slot)
                    .expect("Database error")
                    .map(|meta| meta.consumed)
                    .unwrap_or(0) as u32
            });

        let (data_shreds, coding_shreds, new_next_shred_index) =
            shredder.entries_to_shreds(entries, is_slot_end, next_shred_index);

        self.unfinished_slot = Some(UnfinishedSlotInfo {
            next_shred_index: new_next_shred_index,
            slot,
            parent: parent_slot,
            is_marked_in_blocktree: self
                .unfinished_slot
                .map(|u| u.is_marked_in_blocktree)
                .unwrap_or(false),
        });

        (data_shreds, coding_shreds)
    }

    fn process_receive_results(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
        blocktree: &Arc<Blocktree>,
        insert_blocktree_sender: &Sender<Vec<Shred>>,
        receive_results: ReceiveResults,
    ) -> Result<()> {
        let mut receive_elapsed = receive_results.time_elapsed;
        let num_entries = receive_results.entries.len();
        let bank = receive_results.bank.clone();
        let last_tick = receive_results.last_tick;
        inc_new_counter_info!("broadcast_service-entries_received", num_entries);

        if self.current_slot_and_parent.is_none()
            || bank.slot() != self.current_slot_and_parent.unwrap().0
        {
            self.slot_broadcast_start = Some(Instant::now());
            let slot = bank.slot();
            let parent_slot = {
                if let Some(parent_bank) = bank.parent() {
                    parent_bank.slot()
                } else {
                    0
                }
            };

            self.current_slot_and_parent = Some((slot, parent_slot));
            receive_elapsed = Duration::new(0, 0);
        }

        let to_shreds_start = Instant::now();

        // 1) Check if slot was interrupted
        let last_unfinished_slot_shred = self.check_for_interrupted_slot();

        // 2) Convert entries to shreds and coding shreds
        let (data_shreds, coding_shreds) = self.entries_to_shreds(
            blocktree,
            &receive_results.entries,
            last_tick == bank.max_tick_height(),
        );
        let to_shreds_elapsed = to_shreds_start.elapsed();

        let clone_and_seed_start = Instant::now();
        let all_shreds =
            Self::coalesce_shreds(data_shreds, coding_shreds, last_unfinished_slot_shred);
        let all_shreds_ = all_shreds.clone();
        let all_seeds: Vec<[u8; 32]> = all_shreds.iter().map(|s| s.seed()).collect();
        let clone_and_seed_elapsed = clone_and_seed_start.elapsed();

        // 3) Insert first shred into blocktree, send the remaining shreds to separate thread to
        // insert into blocktree
        let unfinished_slot = self
            .unfinished_slot
            .as_mut()
            .expect("Must exist an unfinished slot at this point");
        if !unfinished_slot.is_marked_in_blocktree {
            unfinished_slot.is_marked_in_blocktree = true;
            blocktree
                .insert_shreds(vec![all_shreds[0].clone()], None)
                .expect("Failed to insert shreds in blocktree");
        }
        insert_blocktree_sender
            .send(all_shreds_)
            .expect("Sender should not be dropped");

        // 4) Broadcast the shreds
        let broadcast_start = Instant::now();
        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let all_shred_bufs: Vec<Vec<u8>> = all_shreds.into_iter().map(|s| s.payload).collect();
        trace!("Broadcasting {:?} shreds", all_shred_bufs.len());

        cluster_info.read().unwrap().broadcast_shreds(
            sock,
            all_shred_bufs,
            &all_seeds,
            stakes.as_ref(),
        )?;

        let broadcast_elapsed = broadcast_start.elapsed();

        self.update_broadcast_stats(
            duration_as_us(&receive_elapsed),
            duration_as_us(&to_shreds_elapsed),
            duration_as_us(&broadcast_elapsed),
            duration_as_us(&clone_and_seed_elapsed),
            last_tick == bank.max_tick_height(),
        );

        if last_tick == bank.max_tick_height() {
            self.unfinished_slot = None;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn update_broadcast_stats(
        &mut self,
        receive_entries_elapsed: u64,
        shredding_elapsed: u64,
        broadcast_elapsed: u64,
        clone_and_seed_elapsed: u64,
        slot_ended: bool,
    ) {
        self.stats.receive_elapsed += receive_entries_elapsed;
        self.stats.shredding_elapsed += shredding_elapsed;
        self.stats.broadcast_elapsed += broadcast_elapsed;
        self.stats.clone_and_seed_elapsed += clone_and_seed_elapsed;

        if slot_ended {
            self.report_and_reset_stats()
        }
    }

    fn report_and_reset_stats(&mut self) {
        assert!(self.unfinished_slot.is_some());
        datapoint_info!(
            "broadcast-bank-stats",
            ("slot", self.unfinished_slot.unwrap().slot as i64, i64),
            ("shredding_time", self.stats.shredding_elapsed as i64, i64),
            ("broadcast_time", self.stats.broadcast_elapsed as i64, i64),
            ("receive_time", self.stats.receive_elapsed as i64, i64),
            (
                "clone_and_seed",
                self.stats.clone_and_seed_elapsed as i64,
                i64
            ),
            (
                "num_shreds",
                i64::from(self.unfinished_slot.unwrap().next_shred_index),
                i64
            ),
            (
                "slot_broadcast_time",
                self.slot_broadcast_start.unwrap().elapsed().as_millis() as i64,
                i64
            ),
        );
        self.stats.reset();
    }
}

impl BroadcastRun for StandardBroadcastRun {
    fn run(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntry>,
        sock: &UdpSocket,
        insert_blocktree_sender: &Sender<Vec<Shred>>,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        let receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        self.process_receive_results(
            cluster_info,
            sock,
            blocktree,
            insert_blocktree_sender,
            receive_results,
        )
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::blocktree::{get_tmp_ledger_path, Blocktree};
    use crate::broadcast_stage::BroadcastStage;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::create_ticks;
    use crate::genesis_utils::create_genesis_block;
    use crate::shred::max_ticks_per_n_shreds;
    use solana_runtime::bank::Bank;
    use solana_sdk::genesis_block::GenesisBlock;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::{Arc, RwLock};
    use std::thread::JoinHandle;
    use std::time::Duration;

    fn setup(
        num_shreds_per_slot: u64,
    ) -> (
        Arc<Blocktree>,
        GenesisBlock,
        Arc<RwLock<ClusterInfo>>,
        Arc<Bank>,
        Arc<Keypair>,
        UdpSocket,
        Sender<Vec<Shred>>,
        JoinHandle<()>,
    ) {
        // Setup
        let ledger_path = get_tmp_ledger_path!();
        let blocktree = Arc::new(
            Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
        );
        let leader_keypair = Arc::new(Keypair::new());
        let leader_pubkey = leader_keypair.pubkey();
        let leader_info = Node::new_localhost_with_pubkey(&leader_pubkey);
        let cluster_info = Arc::new(RwLock::new(ClusterInfo::new_with_invalid_keypair(
            leader_info.info.clone(),
        )));
        let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
        let mut genesis_block = create_genesis_block(10_000).genesis_block;
        genesis_block.ticks_per_slot = max_ticks_per_n_shreds(num_shreds_per_slot) + 1;
        let bank0 = Arc::new(Bank::new(&genesis_block));
        let (sender, receiver) = channel();
        let blocktree_writer = BroadcastStage::run_blocktree_writer(receiver, blocktree.clone());
        (
            blocktree,
            genesis_block,
            cluster_info,
            bank0,
            leader_keypair,
            socket,
            sender,
            blocktree_writer,
        )
    }

    #[test]
    fn test_slot_interrupt() {
        // Setup
        let num_shreds_per_slot = 2;
        let (
            blocktree,
            genesis_block,
            cluster_info,
            bank0,
            leader_keypair,
            socket,
            shred_sender,
            blocktree_writer,
        ) = setup(num_shreds_per_slot);

        // Insert 1 less than the number of ticks needed to finish the slot
        let slot0_ticks = create_ticks(genesis_block.ticks_per_slot - 1, genesis_block.hash());
        let receive_results = ReceiveResults {
            entries: slot0_ticks.clone(),
            time_elapsed: Duration::new(3, 0),
            bank: bank0.clone(),
            last_tick: (slot0_ticks.len() - 1) as u64,
        };

        // Step 1: Make an incomplete transmission for slot 0
        let mut standard_broadcast_run = StandardBroadcastRun::new(leader_keypair.clone());
        standard_broadcast_run
            .process_receive_results(
                &cluster_info,
                &socket,
                &blocktree,
                &shred_sender,
                receive_results,
            )
            .unwrap();
        let unfinished_slot = standard_broadcast_run.unfinished_slot.as_ref().unwrap();
        assert_eq!(unfinished_slot.next_shred_index as u64, num_shreds_per_slot);
        assert_eq!(unfinished_slot.slot, 0);
        assert_eq!(unfinished_slot.parent, 0);
        // Make sure the slot is not complete
        assert!(!blocktree.is_full(0));
        // Modify the stats, should reset later
        standard_broadcast_run.stats.receive_elapsed = 10;

        // Step 2: Make a transmission for another bank that interrupts the transmission for
        // slot 0
        let bank2 = Arc::new(Bank::new_from_parent(&bank0, &leader_keypair.pubkey(), 2));

        // Interrupting the slot should cause the unfinished_slot and stats to reset
        let num_shreds = 1;
        assert!(num_shreds < num_shreds_per_slot);
        let slot1_ticks = create_ticks(max_ticks_per_n_shreds(num_shreds), genesis_block.hash());
        let receive_results = ReceiveResults {
            entries: slot1_ticks.clone(),
            time_elapsed: Duration::new(2, 0),
            bank: bank2.clone(),
            last_tick: (slot1_ticks.len() - 1) as u64,
        };
        standard_broadcast_run
            .process_receive_results(
                &cluster_info,
                &socket,
                &blocktree,
                &shred_sender,
                receive_results,
            )
            .unwrap();
        let unfinished_slot = standard_broadcast_run.unfinished_slot.as_ref().unwrap();

        // The shred index should have reset to 0, which makes it possible for the
        // index < the previous shred index for slot 0
        assert_eq!(unfinished_slot.next_shred_index as u64, num_shreds);
        assert_eq!(unfinished_slot.slot, 2);
        assert_eq!(unfinished_slot.parent, 0);
        // Check that the stats were reset as well
        assert_eq!(standard_broadcast_run.stats.receive_elapsed, 0);

        drop(shred_sender);
        blocktree_writer.join().unwrap();

        // Try to fetch ticks from blocktree for the interrupted slot, nothing should break
        assert_eq!(blocktree.get_slot_entries(0, 0, None).unwrap(), slot0_ticks);
        assert_eq!(
            blocktree
                .get_slot_entries(0, num_shreds_per_slot, None)
                .unwrap(),
            vec![],
        );
    }

    #[test]
    fn test_slot_finish() {
        // Setup
        let num_shreds_per_slot = 2;
        let (
            blocktree,
            genesis_block,
            cluster_info,
            bank0,
            leader_keypair,
            socket,
            shred_sender,
            blocktree_writer,
        ) = setup(num_shreds_per_slot);

        // Insert complete slot of ticks needed to finish the slot
        let ticks = create_ticks(genesis_block.ticks_per_slot, genesis_block.hash());
        let receive_results = ReceiveResults {
            entries: ticks.clone(),
            time_elapsed: Duration::new(3, 0),
            bank: bank0.clone(),
            last_tick: (ticks.len() - 1) as u64,
        };

        let mut standard_broadcast_run = StandardBroadcastRun::new(leader_keypair);
        standard_broadcast_run
            .process_receive_results(
                &cluster_info,
                &socket,
                &blocktree,
                &shred_sender,
                receive_results,
            )
            .unwrap();
        assert!(standard_broadcast_run.unfinished_slot.is_none());
        drop(shred_sender);
        blocktree_writer.join().unwrap();
    }
}
