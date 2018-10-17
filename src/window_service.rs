//! The `window_service` provides a thread for maintaining a window (tail of the ledger).
//!
use cluster_info::{ClusterInfo, NodeInfo};
use counter::Counter;
use entry::EntrySender;
use influx_db_client as influxdb;
use leader_scheduler::LeaderScheduler;
use log::Level;
use metrics;
use packet::SharedBlob;
use rand::{thread_rng, Rng};
use result::{Error, Result};
use solana_program_interface::pubkey::Pubkey;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::{BlobReceiver, BlobSender};
use sys_info::hostname;
use timing::duration_as_ms;
use window::{blob_idx_in_window, SharedWindow, WindowUtil};

pub const MAX_REPAIR_BACKOFF: usize = 128;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WindowServiceReturnType {
    LeaderRotation(u64),
}

fn repair_backoff(last: &mut u64, times: &mut usize, consumed: u64) -> bool {
    //exponential backoff
    if *last != consumed {
        //start with a 50% chance of asking for repairs
        *times = 1;
    }
    *last = consumed;
    *times += 1;

    // Experiment with capping repair request duration.
    // Once nodes are too far behind they can spend many
    // seconds without asking for repair
    if *times > MAX_REPAIR_BACKOFF {
        // 50% chance that a request will fire between 64 - 128 tries
        *times = MAX_REPAIR_BACKOFF / 2;
    }

    //if we get lucky, make the request, which should exponentially get less likely
    thread_rng().gen_range(0, *times as u64) == 0
}

fn add_block_to_retransmit_queue(
    b: &SharedBlob,
    leader_id: Pubkey,
    retransmit_queue: &mut Vec<SharedBlob>,
) {
    let p = b.read().unwrap();
    //TODO this check isn't safe against adverserial packets
    //we need to maintain a sequence window
    trace!(
        "idx: {} addr: {:?} id: {:?} leader: {:?}",
        p.get_index()
            .expect("get_index in fn add_block_to_retransmit_queue"),
        p.get_id()
            .expect("get_id in trace! fn add_block_to_retransmit_queue"),
        p.meta.addr(),
        leader_id
    );
    if p.get_id()
        .expect("get_id in fn add_block_to_retransmit_queue")
        == leader_id
    {
        //TODO
        //need to copy the retransmitted blob
        //otherwise we get into races with which thread
        //should do the recycling
        //
        let nv = SharedBlob::default();
        {
            let mut mnv = nv.write().unwrap();
            let sz = p.meta.size;
            mnv.meta.size = sz;
            mnv.data[..sz].copy_from_slice(&p.data[..sz]);
        }
        retransmit_queue.push(nv);
    }
}

fn retransmit_all_leader_blocks(
    window: &SharedWindow,
    maybe_leader: Option<NodeInfo>,
    dq: &[SharedBlob],
    id: &Pubkey,
    consumed: u64,
    received: u64,
    retransmit: &BlobSender,
    pending_retransmits: &mut bool,
) -> Result<()> {
    let mut retransmit_queue: Vec<SharedBlob> = Vec::new();
    if let Some(leader) = maybe_leader {
        let leader_id = leader.id;
        for b in dq {
            add_block_to_retransmit_queue(b, leader_id, &mut retransmit_queue);
        }

        if *pending_retransmits {
            for w in window
                .write()
                .expect("Window write failed in retransmit_all_leader_blocks")
                .iter_mut()
            {
                *pending_retransmits = false;
                if w.leader_unknown {
                    if let Some(ref b) = w.data {
                        add_block_to_retransmit_queue(b, leader_id, &mut retransmit_queue);
                        w.leader_unknown = false;
                    }
                }
            }
        }
        metrics::submit(
            influxdb::Point::new("retransmit-queue")
                .add_field(
                    "host",
                    influxdb::Value::String(hostname().unwrap_or_else(|_| "?".to_string())),
                ).add_field(
                    "count",
                    influxdb::Value::Integer(retransmit_queue.len() as i64),
                ).to_owned(),
        );
    } else {
        warn!("{}: no leader to retransmit from", id);
    }
    if !retransmit_queue.is_empty() {
        trace!(
            "{}: RECV_WINDOW {} {}: retransmit {}",
            id,
            consumed,
            received,
            retransmit_queue.len(),
        );
        inc_new_counter_info!("streamer-recv_window-retransmit", retransmit_queue.len());
        retransmit.send(retransmit_queue)?;
    }
    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
fn recv_window(
    window: &SharedWindow,
    id: &Pubkey,
    cluster_info: &Arc<RwLock<ClusterInfo>>,
    consumed: &mut u64,
    received: &mut u64,
    max_ix: u64,
    r: &BlobReceiver,
    s: &EntrySender,
    retransmit: &BlobSender,
    pending_retransmits: &mut bool,
    done: &Arc<AtomicBool>,
) -> Result<()> {
    let timer = Duration::from_millis(200);
    let mut dq = r.recv_timeout(timer)?;
    let maybe_leader: Option<NodeInfo> = cluster_info
        .read()
        .expect("'cluster_info' read lock in fn recv_window")
        .leader_data()
        .cloned();
    let leader_unknown = maybe_leader.is_none();
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    let now = Instant::now();
    inc_new_counter_info!("streamer-recv_window-recv", dq.len(), 100);

    metrics::submit(
        influxdb::Point::new("recv-window")
            .add_field(
                "host",
                influxdb::Value::String(hostname().unwrap_or_else(|_| "?".to_string())),
            ).add_field("count", influxdb::Value::Integer(dq.len() as i64))
            .to_owned(),
    );

    trace!(
        "{}: RECV_WINDOW {} {}: got packets {}",
        id,
        *consumed,
        *received,
        dq.len(),
    );

    retransmit_all_leader_blocks(
        window,
        maybe_leader,
        &dq,
        id,
        *consumed,
        *received,
        retransmit,
        pending_retransmits,
    )?;

    let mut pixs = Vec::new();
    //send a contiguous set of blocks
    let mut consume_queue = Vec::new();
    for b in dq {
        let (pix, meta_size) = {
            let p = b.read().unwrap();
            (p.get_index()?, p.meta.size)
        };
        pixs.push(pix);

        if !blob_idx_in_window(&id, pix, *consumed, received) {
            continue;
        }

        // For downloading storage blobs,
        // we only want up to a certain index
        // then stop
        if max_ix != 0 && pix > max_ix {
            continue;
        }

        trace!("{} window pix: {} size: {}", id, pix, meta_size);

        window.write().unwrap().process_blob(
            id,
            b,
            pix,
            &mut consume_queue,
            consumed,
            leader_unknown,
            pending_retransmits,
        );

        // Send a signal when we hit the max entry_height
        if max_ix != 0 && *consumed == (max_ix + 1) {
            done.store(true, Ordering::Relaxed);
        }
    }
    if log_enabled!(Level::Trace) {
        trace!("{}", window.read().unwrap().print(id, *consumed));
        trace!(
            "{}: consumed: {} received: {} sending consume.len: {} pixs: {:?} took {} ms",
            id,
            *consumed,
            *received,
            consume_queue.len(),
            pixs,
            duration_as_ms(&now.elapsed())
        );
    }
    if !consume_queue.is_empty() {
        inc_new_counter_info!("streamer-recv_window-consume", consume_queue.len());
        s.send(consume_queue)?;
    }
    Ok(())
}

#[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
pub fn window_service(
    cluster_info: Arc<RwLock<ClusterInfo>>,
    window: SharedWindow,
    entry_height: u64,
    max_entry_height: u64,
    r: BlobReceiver,
    s: EntrySender,
    retransmit: BlobSender,
    repair_socket: Arc<UdpSocket>,
    leader_scheduler: Arc<RwLock<LeaderScheduler>>,
    done: Arc<AtomicBool>,
) -> JoinHandle<()> {
    Builder::new()
        .name("solana-window".to_string())
        .spawn(move || {
            let mut consumed = entry_height;
            let mut received = entry_height;
            let mut last = entry_height;
            let mut times = 0;
            let id = cluster_info.read().unwrap().id;
            let mut pending_retransmits = false;
            trace!("{}: RECV_WINDOW started", id);
            loop {
                // Check if leader rotation was configured
                if let Err(e) = recv_window(
                    &window,
                    &id,
                    &cluster_info,
                    &mut consumed,
                    &mut received,
                    max_entry_height,
                    &r,
                    &s,
                    &retransmit,
                    &mut pending_retransmits,
                    &done,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter_info!("streamer-window-error", 1, 1);
                            error!("window error: {:?}", e);
                        }
                    }
                }

                metrics::submit(
                    influxdb::Point::new("window-stage")
                        .add_field(
                            "host",
                            influxdb::Value::String(hostname().unwrap_or_else(|_| "?".to_string())),
                        ).add_field("consumed", influxdb::Value::Integer(consumed as i64))
                        .to_owned(),
                );

                if received <= consumed {
                    trace!(
                        "{} we have everything received:{} consumed:{}",
                        id,
                        received,
                        consumed
                    );
                    continue;
                }

                //exponential backoff
                if !repair_backoff(&mut last, &mut times, consumed) {
                    trace!("{} !repair_backoff() times = {}", id, times);
                    continue;
                }
                trace!("{} let's repair! times = {}", id, times);

                let mut window = window.write().unwrap();
                let reqs = window.repair(
                    &cluster_info,
                    &id,
                    times,
                    consumed,
                    received,
                    max_entry_height,
                    &leader_scheduler,
                );
                for (to, req) in reqs {
                    repair_socket.send_to(&req, to).unwrap_or_else(|e| {
                        info!("{} repair req send_to({}) error {:?}", id, to, e);
                        0
                    });
                }
            }
        }).unwrap()
}

#[cfg(test)]
mod test {
    use cluster_info::{ClusterInfo, Node};
    use entry::Entry;
    use hash::Hash;
    use leader_scheduler::LeaderScheduler;
    use logger;
    use packet::{make_consecutive_blobs, SharedBlob, PACKET_DATA_SIZE};
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{channel, Receiver};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer::{blob_receiver, responder};
    use window::default_window;
    use window_service::{repair_backoff, window_service};

    fn get_entries(r: Receiver<Vec<Entry>>, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => {
                    *num += m.len();
                }
                e => info!("error {:?}", e),
            }
            if *num == 10 {
                break;
            }
        }
    }

    #[test]
    pub fn window_send_test() {
        logger::setup();
        let tn = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let mut cluster_info_me = ClusterInfo::new(tn.info.clone()).expect("ClusterInfo::new");
        let me_id = cluster_info_me.my_data().id;
        cluster_info_me.set_leader(me_id);
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver = blob_receiver(Arc::new(tn.sockets.gossip), exit.clone(), s_reader);
        let (s_window, r_window) = channel();
        let (s_retransmit, r_retransmit) = channel();
        let win = Arc::new(RwLock::new(default_window()));
        let done = Arc::new(AtomicBool::new(false));
        let t_window = window_service(
            subs,
            win,
            0,
            0,
            r_reader,
            s_window,
            s_retransmit,
            Arc::new(tn.sockets.repair),
            Arc::new(RwLock::new(LeaderScheduler::from_bootstrap_leader(me_id))),
            done,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                tn.sockets.replicate.into_iter().map(Arc::new).collect();

            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let mut num_blobs_to_make = 10;
            let gossip_address = &tn.info.contact_info.ncp;
            let msgs = make_consecutive_blobs(
                me_id,
                num_blobs_to_make,
                0,
                Hash::default(),
                &gossip_address,
            ).into_iter()
            .rev()
            .collect();;
            s_responder.send(msgs).expect("send");
            t_responder
        };

        let mut num = 0;
        get_entries(r_window, &mut num);
        assert_eq!(num, 10);
        let mut q = r_retransmit.recv().unwrap();
        while let Ok(mut nq) = r_retransmit.try_recv() {
            q.append(&mut nq);
        }
        assert_eq!(q.len(), 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
    }

    #[test]
    pub fn window_send_no_leader_test() {
        logger::setup();
        let tn = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let cluster_info_me = ClusterInfo::new(tn.info.clone()).expect("ClusterInfo::new");
        let me_id = cluster_info_me.my_data().id;
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver = blob_receiver(Arc::new(tn.sockets.gossip), exit.clone(), s_reader);
        let (s_window, _r_window) = channel();
        let (s_retransmit, r_retransmit) = channel();
        let win = Arc::new(RwLock::new(default_window()));
        let done = Arc::new(AtomicBool::new(false));
        let t_window = window_service(
            subs.clone(),
            win,
            0,
            0,
            r_reader,
            s_window,
            s_retransmit,
            Arc::new(tn.sockets.repair),
            // TODO: For now, the window still checks the ClusterInfo for the current leader
            // to determine whether to retransmit a block. In the future when we rely on
            // the LeaderScheduler for retransmits, this test will need to be rewritten
            // because a leader should only be unknown in the window when the write stage
            // hasn't yet calculated the leaders for slots in the next epoch (on entries
            // at heights that are multiples of seed_rotation_interval in LeaderScheduler)
            Arc::new(RwLock::new(LeaderScheduler::default())),
            done,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                tn.sockets.replicate.into_iter().map(Arc::new).collect();
            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let mut msgs = Vec::new();
            for v in 0..10 {
                let i = 9 - v;
                let b = SharedBlob::default();
                {
                    let mut w = b.write().unwrap();
                    w.set_index(i).unwrap();
                    w.set_id(me_id).unwrap();
                    assert_eq!(i, w.get_index().unwrap());
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&tn.info.contact_info.ncp);
                }
                msgs.push(b);
            }
            s_responder.send(msgs).expect("send");
            t_responder
        };

        assert!(r_retransmit.recv_timeout(Duration::new(3, 0)).is_err());
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
    }

    #[test]
    pub fn window_send_late_leader_test() {
        logger::setup();
        let tn = Node::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let cluster_info_me = ClusterInfo::new(tn.info.clone()).expect("ClusterInfo::new");
        let me_id = cluster_info_me.my_data().id;
        let subs = Arc::new(RwLock::new(cluster_info_me));

        let (s_reader, r_reader) = channel();
        let t_receiver = blob_receiver(Arc::new(tn.sockets.gossip), exit.clone(), s_reader);
        let (s_window, _r_window) = channel();
        let (s_retransmit, r_retransmit) = channel();
        let win = Arc::new(RwLock::new(default_window()));
        let done = Arc::new(AtomicBool::new(false));
        let t_window = window_service(
            subs.clone(),
            win,
            0,
            0,
            r_reader,
            s_window,
            s_retransmit,
            Arc::new(tn.sockets.repair),
            // TODO: For now, the window still checks the ClusterInfo for the current leader
            // to determine whether to retransmit a block. In the future when we rely on
            // the LeaderScheduler for retransmits, this test will need to be rewritten
            // becasue a leader should only be unknown in the window when the write stage
            // hasn't yet calculated the leaders for slots in the next epoch (on entries
            // at heights that are multiples of seed_rotation_interval in LeaderScheduler)
            Arc::new(RwLock::new(LeaderScheduler::default())),
            done,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let blob_sockets: Vec<Arc<UdpSocket>> =
                tn.sockets.replicate.into_iter().map(Arc::new).collect();
            let t_responder = responder("window_send_test", blob_sockets[0].clone(), r_responder);
            let mut msgs = Vec::new();
            for v in 0..10 {
                let i = 9 - v;
                let b = SharedBlob::default();
                {
                    let mut w = b.write().unwrap();
                    w.set_index(i).unwrap();
                    w.set_id(me_id).unwrap();
                    assert_eq!(i, w.get_index().unwrap());
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&tn.info.contact_info.ncp);
                }
                msgs.push(b);
            }
            s_responder.send(msgs).expect("send");

            assert!(r_retransmit.recv_timeout(Duration::new(3, 0)).is_err());

            subs.write().unwrap().set_leader(me_id);

            let mut msgs1 = Vec::new();
            for v in 1..5 {
                let i = 9 + v;
                let b = SharedBlob::default();
                {
                    let mut w = b.write().unwrap();
                    w.set_index(i).unwrap();
                    w.set_id(me_id).unwrap();
                    assert_eq!(i, w.get_index().unwrap());
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&tn.info.contact_info.ncp);
                }
                msgs1.push(b);
            }
            s_responder.send(msgs1).expect("send");
            t_responder
        };
        let mut q = r_retransmit.recv().unwrap();
        while let Ok(mut nq) = r_retransmit.recv_timeout(Duration::from_millis(100)) {
            q.append(&mut nq);
        }
        assert!(q.len() > 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
    }

    #[test]
    pub fn test_repair_backoff() {
        let num_tests = 100;
        let res: usize = (0..num_tests)
            .map(|_| {
                let mut last = 0;
                let mut times = 0;
                let total: usize = (0..127)
                    .map(|x| {
                        let rv = repair_backoff(&mut last, &mut times, 1) as usize;
                        assert_eq!(times, x + 2);
                        rv
                    }).sum();
                assert_eq!(times, 128);
                assert_eq!(last, 1);
                repair_backoff(&mut last, &mut times, 1);
                assert_eq!(times, 64);
                repair_backoff(&mut last, &mut times, 2);
                assert_eq!(times, 2);
                assert_eq!(last, 2);
                total
            }).sum();
        let avg = res / num_tests;
        assert!(avg >= 3);
        assert!(avg <= 5);
    }
}
