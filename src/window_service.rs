//! The `window_service` provides a thread for maintaining a window (tail of the ledger).
//!
use crate::blocktree::Blocktree;
use crate::cluster_info::ClusterInfo;
use crate::counter::Counter;
use crate::db_window::*;
use crate::leader_scheduler::LeaderScheduler;
use crate::repair_service::RepairService;
use crate::result::{Error, Result};
use crate::service::Service;
use crate::streamer::{BlobReceiver, BlobSender};
use log::Level;
use solana_metrics::{influxdb, submit};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::duration_as_ms;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{self, Builder, JoinHandle};
use std::time::{Duration, Instant};

pub const MAX_REPAIR_BACKOFF: usize = 128;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WindowServiceReturnType {
    LeaderRotation(u64),
}

#[allow(clippy::too_many_arguments)]
fn recv_window(
    blocktree: &Arc<Blocktree>,
    id: &Pubkey,
    leader_scheduler: &Arc<RwLock<LeaderScheduler>>,
    r: &BlobReceiver,
    retransmit: &BlobSender,
) -> Result<()> {
    let timer = Duration::from_millis(200);
    let mut dq = r.recv_timeout(timer)?;

    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    let now = Instant::now();
    inc_new_counter_info!("streamer-recv_window-recv", dq.len(), 100);

    submit(
        influxdb::Point::new("recv-window")
            .add_field("count", influxdb::Value::Integer(dq.len() as i64))
            .to_owned(),
    );

    retransmit_blobs(&dq, leader_scheduler, retransmit, id)?;

    //send a contiguous set of blocks
    trace!("{} num blobs received: {}", id, dq.len());

    for b in dq {
        let (pix, meta_size) = {
            let p = b.read().unwrap();
            (p.index(), p.meta.size)
        };

        trace!("{} window pix: {} size: {}", id, pix, meta_size);

        let _ = process_blob(leader_scheduler, blocktree, &b);
    }

    trace!(
        "Elapsed processing time in recv_window(): {}",
        duration_as_ms(&now.elapsed())
    );

    Ok(())
}

// Implement a destructor for the window_service thread to signal it exited
// even on panics
struct Finalizer {
    exit_sender: Arc<AtomicBool>,
}

impl Finalizer {
    fn new(exit_sender: Arc<AtomicBool>) -> Self {
        Finalizer { exit_sender }
    }
}
// Implement a destructor for Finalizer.
impl Drop for Finalizer {
    fn drop(&mut self) {
        self.exit_sender.clone().store(true, Ordering::Relaxed);
    }
}

pub struct WindowService {
    t_window: JoinHandle<()>,
    repair_service: RepairService,
}

impl WindowService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        blocktree: Arc<Blocktree>,
        cluster_info: Arc<RwLock<ClusterInfo>>,
        r: BlobReceiver,
        retransmit: BlobSender,
        repair_socket: Arc<UdpSocket>,
        leader_scheduler: Arc<RwLock<LeaderScheduler>>,
        exit: Arc<AtomicBool>,
    ) -> WindowService {
        let exit_ = exit.clone();
        let repair_service = RepairService::new(
            blocktree.clone(),
            exit.clone(),
            repair_socket,
            cluster_info.clone(),
        );
        let t_window = Builder::new()
            .name("solana-window".to_string())
            .spawn(move || {
                let _exit = Finalizer::new(exit_);
                let id = cluster_info.read().unwrap().id();
                trace!("{}: RECV_WINDOW started", id);
                loop {
                    if exit.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = recv_window(&blocktree, &id, &leader_scheduler, &r, &retransmit)
                    {
                        match e {
                            Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                            Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                            _ => {
                                inc_new_counter_info!("streamer-window-error", 1, 1);
                                error!("window error: {:?}", e);
                            }
                        }
                    }
                }
            })
            .unwrap();

        WindowService {
            t_window,
            repair_service,
        }
    }
}

impl Service for WindowService {
    type JoinReturnType = ();

    fn join(self) -> thread::Result<()> {
        self.t_window.join()?;
        self.repair_service.join()
    }
}

#[cfg(test)]
mod test {
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::Blocktree;
    use crate::cluster_info::{ClusterInfo, Node};
    use crate::entry::make_consecutive_blobs;
    use crate::leader_scheduler::LeaderScheduler;
    use crate::service::Service;
    use crate::streamer::{blob_receiver, responder};
    use crate::window_service::WindowService;
    use solana_sdk::hash::Hash;
    use std::fs::remove_dir_all;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    #[test]
    pub fn window_send_test() {
        solana_logger::setup();
        // setup a leader whose id is used to generates blobs and a validator
        // node whose window service will retransmit leader blobs.
        let leader_node = Node::new_localhost();
        let validator_node = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let mut cluster_info_me = ClusterInfo::new(validator_node.info.clone());
        let me_id = leader_node.info.id;
        cluster_info_me.set_leader(me_id);
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver =
            blob_receiver(Arc::new(leader_node.sockets.gossip), exit.clone(), s_reader);
        let (s_retransmit, r_retransmit) = channel();
        let blocktree_path = get_tmp_ledger_path("window_send_test");
        let blocktree = Arc::new(
            Blocktree::open(&blocktree_path).expect("Expected to be able to open database ledger"),
        );
        let mut leader_schedule = LeaderScheduler::default();
        leader_schedule.set_leader_schedule(vec![me_id]);
        let t_window = WindowService::new(
            blocktree,
            subs,
            r_reader,
            s_retransmit,
            Arc::new(leader_node.sockets.repair),
            Arc::new(RwLock::new(leader_schedule)),
            exit.clone(),
        );
        let num_blobs = 7;
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                leader_node.sockets.tvu.into_iter().map(Arc::new).collect();

            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let gossip_address = &leader_node.info.gossip;
            let msgs = make_consecutive_blobs(num_blobs, 0, Hash::default(), &gossip_address)
                .into_iter()
                .rev()
                .collect();;
            s_responder.send(msgs).expect("send");
            t_responder
        };
        let max_attempts = 10;
        let mut num_attempts = 0;
        let mut q = Vec::new();
        loop {
            assert!(num_attempts != max_attempts);
            while let Ok(mut nq) = r_retransmit.recv_timeout(Duration::from_millis(500)) {
                q.append(&mut nq);
            }
            if q.len() == num_blobs as usize {
                break;
            }
            num_attempts += 1;
        }

        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&blocktree_path);
    }

    #[test]
    pub fn window_send_leader_test2() {
        solana_logger::setup();
        // setup a leader whose id is used to generates blobs and a validator
        // node whose window service will retransmit leader blobs.
        let leader_node = Node::new_localhost();
        let validator_node = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let cluster_info_me = ClusterInfo::new(validator_node.info.clone());
        let me_id = leader_node.info.id;
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver =
            blob_receiver(Arc::new(leader_node.sockets.gossip), exit.clone(), s_reader);
        let (s_retransmit, r_retransmit) = channel();
        let blocktree_path = get_tmp_ledger_path("window_send_late_leader_test");
        let blocktree = Arc::new(
            Blocktree::open(&blocktree_path).expect("Expected to be able to open database ledger"),
        );
        let mut leader_schedule = LeaderScheduler::default();
        leader_schedule.set_leader_schedule(vec![me_id]);
        let t_window = WindowService::new(
            blocktree,
            subs.clone(),
            r_reader,
            s_retransmit,
            Arc::new(leader_node.sockets.repair),
            Arc::new(RwLock::new(leader_schedule)),
            exit.clone(),
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                leader_node.sockets.tvu.into_iter().map(Arc::new).collect();
            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let mut msgs = Vec::new();
            let blobs = make_consecutive_blobs(14u64, 0, Hash::default(), &leader_node.info.gossip);

            for v in 0..10 {
                let i = 9 - v;
                msgs.push(blobs[i].clone());
            }
            s_responder.send(msgs).expect("send");

            subs.write().unwrap().set_leader(me_id);
            let mut msgs1 = Vec::new();
            for v in 1..5 {
                let i = 9 + v;
                msgs1.push(blobs[i].clone());
            }
            s_responder.send(msgs1).expect("send");
            t_responder
        };
        let mut q = Vec::new();
        while let Ok(mut nq) = r_retransmit.recv_timeout(Duration::from_millis(500)) {
            q.append(&mut nq);
        }
        assert!(q.len() > 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
        Blocktree::destroy(&blocktree_path).expect("Expected successful database destruction");
        let _ignored = remove_dir_all(&blocktree_path);
    }
}
