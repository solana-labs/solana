use super::*;
use solana_ledger::shred::{Shredder, RECOMMENDED_FEC_RATE};
use solana_sdk::hash::Hash;
use solana_sdk::signature::Keypair;

pub(super) struct FailEntryVerificationBroadcastRun {
    shred_version: u16,
    keypair: Arc<Keypair>,
}

impl FailEntryVerificationBroadcastRun {
    //let keypair = cluster_info.read().unwrap().keypair.clone();
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
        blocktree: &Arc<Blocktree>,
        receiver: &Receiver<WorkingBankEntry>,
        socket_sender: &Sender<(Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>)>,
        blocktree_sender: &Sender<Arc<Vec<Shred>>>,
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

        let next_shred_index = blocktree
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

        let all_shreds = data_shreds
            .iter()
            .cloned()
            .chain(coding_shreds.iter().cloned())
            .collect::<Vec<_>>();
        blocktree_sender.send(all_shreds)?;
        // 3) Start broadcast step
        let bank_epoch = bank.get_leader_schedule_epoch(bank.slot());
        let stakes = staking_utils::staked_nodes_at_epoch(&bank, bank_epoch);

        let all_shred_bufs: Vec<Vec<u8>> = data_shreds
            .into_iter()
            .chain(coding_shreds.into_iter())
            .map(|s| s.payload)
            .collect();

        socket_sender.send((stakes, all_shred_bufs))?;
        Ok(())
    }
    fn transmit(
        &self,
        receiver: &Arc<Mutex<Receiver<(Option<Arc<HashMap<Pubkey, u64>>>, Arc<Vec<Shred>>)>>>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sock: &UdpSocket,
    ) -> Result<()> {
        let (stakes, shreds) = receiver.recv()?;
        let all_seeds: Vec<[u8; 32]> = shreds.iter().map(|s| s.seed()).collect();
        // Broadcast data
        cluster_info
            .read()
            .unwrap()
            .broadcast_shreds(sock, shreds, &all_seeds, stakes.as_ref())?;
        Ok(())
    }
    fn record(
        &self,
        receiver: &Arc<Mutex<Receiver<Arc<Vec<Shred>>>>>,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        let all_shreds = receiver.recv()?;
        blocktree
            .insert_shreds(all_shreds, None, true)
            .expect("Failed to insert shreds in blocktree");
    }
}
