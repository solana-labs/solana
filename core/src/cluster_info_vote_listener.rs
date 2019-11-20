use crate::cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS};
use crate::packet::Packets;
use crate::poh_recorder::PohRecorder;
use crate::result::Result;
use crate::{packet, sigverify};
use crossbeam_channel::Sender as CrossbeamSender;
use solana_metrics::inc_new_counter_debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct ClusterInfoVoteListener {
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ClusterInfoVoteListener {
    pub fn new(
        exit: &Arc<AtomicBool>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        sigverify_disabled: bool,
        sender: CrossbeamSender<Vec<Packets>>,
        poh_recorder: &Arc<Mutex<PohRecorder>>,
    ) -> Self {
        let exit = exit.clone();
        let poh_recorder = poh_recorder.clone();
        let thread = Builder::new()
            .name("solana-cluster_info_vote_listener".to_string())
            .spawn(move || {
                let _ = Self::recv_loop(
                    exit,
                    &cluster_info,
                    sigverify_disabled,
                    &sender,
                    poh_recorder,
                );
            })
            .unwrap();
        Self {
            thread_hdls: vec![thread],
        }
    }
    fn recv_loop(
        exit: Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sigverify_disabled: bool,
        sender: &CrossbeamSender<Vec<Packets>>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
    ) -> Result<()> {
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            if let Some(bank) = poh_recorder.lock().unwrap().bank() {
                let last_ts = bank.last_vote_sync.load(Ordering::Relaxed);
                let (votes, new_ts) = cluster_info.read().unwrap().get_votes(last_ts);
                bank.last_vote_sync
                    .compare_and_swap(last_ts, new_ts, Ordering::Relaxed);
                inc_new_counter_debug!("cluster_info_vote_listener-recv_count", votes.len());
                let mut msgs = packet::to_packets(&votes);
                if !msgs.is_empty() {
                    let r = if sigverify_disabled {
                        sigverify::ed25519_verify_disabled(&msgs)
                    } else {
                        sigverify::ed25519_verify_cpu(&msgs)
                    };
                    sigverify::mark_disabled(&mut msgs, &r);
                    sender.send(msgs)?;
                }
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }

    pub fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::packet;
    use solana_sdk::hash::Hash;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::transaction::Transaction;
    use solana_vote_program::vote_instruction;
    use solana_vote_program::vote_state::Vote;

    #[test]
    fn test_max_vote_tx_fits() {
        solana_logger::setup();
        let node_keypair = Keypair::new();
        let vote_keypair = Keypair::new();
        let slots: Vec<_> = (0..31).into_iter().collect();
        let votes = Vote::new(slots, Hash::default());
        let vote_ix = vote_instruction::vote(&vote_keypair.pubkey(), &vote_keypair.pubkey(), votes);

        let mut vote_tx = Transaction::new_with_payer(vec![vote_ix], Some(&node_keypair.pubkey()));

        vote_tx.partial_sign(&[&node_keypair], Hash::default());
        vote_tx.partial_sign(&[&vote_keypair], Hash::default());

        use bincode::serialized_size;
        info!("max vote size {}", serialized_size(&vote_tx).unwrap());

        let msgs = packet::to_packets(&[vote_tx]); // panics if won't fit

        assert_eq!(msgs.len(), 1);
    }
}
