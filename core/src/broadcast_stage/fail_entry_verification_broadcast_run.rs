use super::*;
use solana_ledger::shred::{Shredder, RECOMMENDED_FEC_RATE};
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
        socket_sender: &Sender<TransmitShreds>,
        blockstore_sender: &Sender<Arc<Vec<Shred>>>,
    ) -> Result<()> {
        // 1) Pull entries from banking stage
        let mut receive_results = broadcast_utils::recv_slot_entries(receiver)?;
        let bank = receive_results.bank.clone();
        let last_tick_height = receive_results.last_tick_height;

        // 2) Convert entries to shreds + generate coding shreds. Set a garbage PoH on the last entry
        // in the slot to make verification fail on validators
        if last_tick_height == bank.max_tick_height() {
            let mut last_entry = receive_results.entries.last_mut().unwrap();
            last_entry.hash = Hash::default();
        }

        let next_shred_index = blockstore
            .meta(bank.slot())
            .expect("Database error")
            .map(|meta| meta.consumed)
            .unwrap_or(0) as u32;

        let shredder = Shredder::new(
            bank.slot(),
            bank.parent().unwrap().slot(),
            RECOMMENDED_FEC_RATE,
            self.keypair.clone(),
            (bank.tick_height() % bank.ticks_per_slot()) as u8,
            self.shred_version,
        )
        .expect("Expected to create a new shredder");

        let (data_shreds, coding_shreds, _) = shredder.entries_to_shreds(
            &receive_results.entries,
            last_tick_height == bank.max_tick_height(),
            next_shred_index,
        );

        let data_shreds = Arc::new(data_shreds);
        blockstore_sender.send(data_shreds.clone())?;
        // 3) Start broadcast step
        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let stakes = stakes.map(Arc::new);
        socket_sender.send((stakes.clone(), data_shreds))?;
        socket_sender.send((stakes, Arc::new(coding_shreds)))?;
        Ok(())
    }
    fn transmit(
        &self,
        receiver: &Arc<Mutex<Receiver<TransmitShreds>>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
    ) -> Result<()> {
        let (stakes, shreds) = receiver.lock().unwrap().recv()?;
        let all_seeds: Vec<[u8; 32]> = shreds.iter().map(|s| s.seed()).collect();
        // Broadcast data
        let all_shred_bufs: Vec<Vec<u8>> = shreds.to_vec().into_iter().map(|s| s.payload).collect();
        cluster_info
            .read()
            .unwrap()
            .broadcast_shreds(sock, all_shred_bufs, &all_seeds, stakes)?;
        Ok(())
    }
    fn record(
        &self,
        receiver: &Arc<Mutex<Receiver<Arc<Vec<Shred>>>>>,
        blockstore: &Arc<Blockstore>,
    ) -> Result<()> {
        let all_shreds = receiver.lock().unwrap().recv()?;
        blockstore
            .insert_shreds(all_shreds.to_vec(), None, true)
            .expect("Failed to insert shreds in blockstore");
        Ok(())
    }
}
