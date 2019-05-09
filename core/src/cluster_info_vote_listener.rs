use crate::cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS};
use crate::poh_recorder::PohRecorder;
use crate::result::Result;
use crate::service::Service;
use crate::sigverify_stage::VerifiedPackets;
use crate::{packet, sigverify};
use solana_metrics::inc_new_counter_info;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
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
        sender: Sender<VerifiedPackets>,
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
        sender: &Sender<VerifiedPackets>,
        poh_recorder: Arc<Mutex<PohRecorder>>,
    ) -> Result<()> {
        let mut last_ts = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            let (votes, new_ts) = cluster_info.read().unwrap().get_votes(last_ts);
            if poh_recorder.lock().unwrap().bank().is_some() {
                last_ts = new_ts;
                inc_new_counter_info!("cluster_info_vote_listener-recv_count", votes.len());
                let msgs = packet::to_packets(&votes);
                if !msgs.is_empty() {
                    let r = if sigverify_disabled {
                        sigverify::ed25519_verify_disabled(&msgs)
                    } else {
                        sigverify::ed25519_verify_cpu(&msgs)
                    };
                    sender.send(msgs.into_iter().zip(r).collect())?;
                }
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }
}

impl Service for ClusterInfoVoteListener {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        for thread_hdl in self.thread_hdls {
            thread_hdl.join()?;
        }
        Ok(())
    }
}
