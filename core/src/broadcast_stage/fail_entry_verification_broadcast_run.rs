use super::*;
use solana_ledger::shred::Shredder;
use solana_sdk::hash::Hash;
use solana_sdk::signature::Keypair;

#[derive(Clone)]
pub(super) struct FailEntryVerificationBroadcastRun {
    shred_version: u16,
    keypair: Arc<Keypair>,
}

impl FailEntryVerificationBroadcastRun {
    pub(super) fn new(keypair: Arc<Keypair>, shred_version: u16) -> Self {
        Self {
            shred_version,
            keypair,
        }
    }
}

impl BroadcastRun for FailEntryVerificationBroadcastRun {
    fn run(
        &mut self,
        blockstore: &Arc<Blockstore>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(TransmitShreds, Option<BroadcastShredBatchInfo>)>,
        blockstore_sender: &Sender<(Arc<Vec<Shred>>, Option<BroadcastShredBatchInfo>)>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let mut receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick_height = receive_results.last_tick_height;

        // 2) Convert entries to shreds + generate coding shreds. Set a garbage PoH on the last entry
        // in the slot to make verification fail on validators
        let last_entries = {
            if last_tick_height == bank.max_tick_height() {
                let good_last_entry = receive_results.entries.pop().unwrap();
                let mut bad_last_entry = good_last_entry.clone();
                bad_last_entry.hash = Hash::default();
                Some((good_last_entry, bad_last_entry))
            } else {
                None
            }
        };

        let next_shred_index = blockstore
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0) as u32;

        let shredder = Shredder::new(
            bank.slot(),
            bank.parent().unwrap().slot(),
            0.0,
            self.keypair.clone(),
            (bank.tick_height() % bank.ticks_per_slot()) as u8,
            self.shred_version,
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, _, _) =
            shredder.entries_to_shreds(&receive_results.entries, false, next_shred_index);

        let last_shreds = last_entries.map(|(good_last_entry, bad_last_entry)| {
            let (good_last_data_shred, _, _) = shredder.entries_to_shreds(
                &vec![good_last_entry],
                true,
                next_shred_index + data_shreds.len() as u32,
            );

            let (bad_last_data_shred, _, _) = shredder.entries_to_shreds(
                &vec![bad_last_entry],
                true,
                next_shred_index + data_shreds.len() as u32,
            );

            (good_last_data_shred, bad_last_data_shred)
        });

        let data_shreds = Arc::new(data_shreds);
        blockstore_sender.send((data_shreds.clone(), None))?;
        // 3) Start broadcast step
        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);
        let stakes = stakes.map(Arc::new);
        socket_sender.send(((stakes.clone(), data_shreds), None))?;
        if let Some((good_last_data_shred, bad_last_data_shred)) = last_shreds {
            let bad_last_data_shred = Arc::new(bad_last_data_shred);
            // Store the bad shred so we serve bad repairs to validators catching up
            blockstore_sender.send((bad_last_data_shred.clone(), None))?;
            // Send good shreds to rest of network
            socket_sender.send(((stakes.clone(), Arc::new(good_last_data_shred)), None))?;
        }
        Ok(())
    }
    fn transmit(
        &mut self,
        receiver: &Arc<Mutex<TransmitReceiver>>,
        cluster_info: &ClusterInfo,
        sock: &UdpSocket,
    ) -> Result<()> {
        let ((stakes, shreds), _) = receiver.lock().unwrap().recv()?;
        // Broadcast data
        let (peers, peers_and_stakes) = get_broadcast_peers(cluster_info, stakes);

        let mut send_mmsg_total = 0;
        broadcast_shreds(
            sock,
            &shreds,
            &peers_and_stakes,
            &peers,
            &Arc::new(AtomicU64::new(0)),
            &mut send_mmsg_total,
        )?;

        Ok(())
    }
    fn record(
        &mut self,
        receiver: &Arc<Mutex<RecordReceiver>>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<()> {
        let (all_shreds, _) = receiver.lock().unwrap().recv()?;
        blockstore
            .insert_shreds(all_shreds.to_vec(), None, true)
            .expect("Failed to insert shreds in blockstore");
        Ok(())
    }
}
