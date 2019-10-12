use crate::blocktree::Blocktree;
use crate::broadcast_stage::standard_broadcast_run::StandardBroadcastRun;
use crate::broadcast_stage::BroadcastRun;
use crate::cluster_info::ClusterInfo;
use crate::poh_recorder::WorkingBankEntry;
use crate::result::Result;
use crate::shred::Shred;
use solana_sdk::signature::Keypair;
use std::collections::HashSet;
use std::net::UdpSocket;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, RwLock};

pub struct DropSlotsFromBlocktreeRun {
    slots_to_ignore: HashSet<u64>,
    standard_broadcast_run: StandardBroadcastRun,
}
impl DropSlotsFromBlocktreeRun {
    pub fn new(slots_to_ignore: HashSet<u64>, keypair: Arc<Keypair>) -> Self {
        let standard_broadcast_run = StandardBroadcastRun::new(keypair);
        Self {
            slots_to_ignore,
            standard_broadcast_run,
        }
    }
}
impl BroadcastRun for DropSlotsFromBlocktreeRun {
    fn run(
        &mut self,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        receiver: &Receiver<WorkingBankEntry>,
        sock: &UdpSocket,
        insert_blocktree_sender: &Sender<Vec<Shred>>,
        blocktree: &Arc<Blocktree>,
    ) -> Result<()> {
        let (send_intercept, receive_intercept) = channel();
        let result = self.standard_broadcast_run.run(
            cluster_info,
            receiver,
            sock,
            &send_intercept,
            blocktree,
        );
        if let Ok(mut shreds) = receive_intercept.try_recv() {
            shreds.retain(|s| !self.slots_to_ignore.contains(&s.slot()));
            insert_blocktree_sender
                .send(shreds)
                .expect("Sender should not have been dropped");
        }
        result
    }
}
