use crate::cluster_info::{ClusterInfo, GOSSIP_SLEEP_MILLIS};
use crate::packet;
use crate::result::Result;
use crate::service::Service;
use crate::streamer::PacketSender;
use solana_metrics::counter::Counter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::thread::{self, sleep, Builder, JoinHandle};
use std::time::Duration;

pub struct ClusterInfoVoteListener {
    exit: Arc<AtomicBool>,
    thread_hdls: Vec<JoinHandle<()>>,
}

impl ClusterInfoVoteListener {
    pub fn new(
        exit: Arc<AtomicBool>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        sender: PacketSender,
    ) -> Self {
        let exit1 = exit.clone();
        let thread = Builder::new()
            .name("solana-cluster_info_vote_listener".to_string())
            .spawn(move || {
                let _ = Self::recv_loop(&exit1, &cluster_info, &sender);
            })
            .unwrap();
        Self {
            exit,
            thread_hdls: vec![thread],
        }
    }
    fn recv_loop(
        exit: &Arc<AtomicBool>,
        cluster_info: &Arc<RwLock<ClusterInfo>>,
        sender: &PacketSender,
    ) -> Result<()> {
        let mut last_ts = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                return Ok(());
            }
            let (votes, new_ts) = cluster_info.read().unwrap().get_votes(last_ts);
            last_ts = new_ts;
            inc_new_counter_info!("cluster_info_vote_listener-recv_count", votes.len());
            let msgs = packet::to_packets(&votes);
            for m in msgs {
                sender.send(m)?;
            }
            sleep(Duration::from_millis(GOSSIP_SLEEP_MILLIS));
        }
    }
    pub fn close(&self) {
        self.exit.store(true, Ordering::Relaxed);
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
