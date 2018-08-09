//! The `window` module defines data structure for storing the tail of the ledger.
//!
use counter::Counter;
use crdt::{Crdt, NodeInfo};
use entry::Entry;
#[cfg(feature = "erasure")]
use erasure;
use ledger::Block;
use log::Level;
use packet::{BlobRecycler, SharedBlob, SharedBlobs, BLOB_SIZE};
use result::{Error, Result};
use std::cmp;
use std::collections::VecDeque;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::AtomicUsize;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, RwLock};
use std::thread::{Builder, JoinHandle};
use std::time::{Duration, Instant};
use streamer::{BlobReceiver, BlobSender};
use timing::duration_as_ms;

pub const WINDOW_SIZE: u64 = 2 * 1024;

#[derive(Clone, Default)]
pub struct WindowSlot {
    pub data: Option<SharedBlob>,
    pub coding: Option<SharedBlob>,
}

pub type SharedWindow = Arc<RwLock<Vec<WindowSlot>>>;

#[derive(Debug, PartialEq, Eq)]
pub enum WindowError {
    GenericError,
}

#[derive(Debug)]
pub struct WindowIndex {
    pub data: u64,
    pub coding: u64,
}

fn find_next_missing(
    window: &SharedWindow,
    crdt: &Arc<RwLock<Crdt>>,
    recycler: &BlobRecycler,
    consumed: u64,
    received: u64,
) -> Result<Vec<(SocketAddr, Vec<u8>)>> {
    if received <= consumed {
        Err(WindowError::GenericError)?;
    }
    let mut window = window.write().unwrap();
    let reqs: Vec<_> = (consumed..received)
        .filter_map(|pix| {
            let i = (pix % WINDOW_SIZE) as usize;

            if let Some(blob) = mem::replace(&mut window[i].data, None) {
                let blob_idx = blob.read().unwrap().get_index().unwrap();
                if blob_idx == pix {
                    mem::replace(&mut window[i].data, Some(blob));
                } else {
                    recycler.recycle(blob);
                }
            }
            if window[i].data.is_none() {
                let val = crdt.read().unwrap().window_index_request(pix as u64);
                if let Ok((to, req)) = val {
                    return Some((to, req));
                }
            }
            None
        })
        .collect();
    Ok(reqs)
}

fn calculate_highest_lost_blob_index(num_peers: u64, consumed: u64, received: u64) -> u64 {
    // Calculate the highest blob index that this node should have already received
    // via avalanche. The avalanche splits data stream into nodes and each node retransmits
    // the data to their peer nodes. So there's a possibility that a blob (with index lower
    // than current received index) is being retransmitted by a peer node.
    let highest_lost = cmp::max(consumed, received.saturating_sub(num_peers));

    // This check prevents repairing a blob that will cause window to roll over. Even if
    // the highes_lost blob is actually missing, asking to repair it might cause our
    // current window to move past other missing blobs
    cmp::min(consumed + WINDOW_SIZE - 1, highest_lost)
}

fn repair_window(
    debug_id: u64,
    window: &SharedWindow,
    crdt: &Arc<RwLock<Crdt>>,
    recycler: &BlobRecycler,
    last: &mut u64,
    times: &mut usize,
    consumed: u64,
    received: u64,
) -> Result<()> {
    //exponential backoff
    if *last != consumed {
        *times = 0;
    }
    *last = consumed;
    *times += 1;
    //if times flips from all 1s 7 -> 8, 15 -> 16, we retry otherwise return Ok
    if *times & (*times - 1) != 0 {
        trace!("repair_window counter {} {} {}", *times, consumed, received);
        return Ok(());
    }

    let highest_lost = calculate_highest_lost_blob_index(
        crdt.read().unwrap().table.len() as u64,
        consumed,
        received,
    );
    let reqs = find_next_missing(window, crdt, recycler, consumed, highest_lost)?;
    trace!("{:x}: repair_window missing: {}", debug_id, reqs.len());
    if !reqs.is_empty() {
        inc_new_counter_info!("streamer-repair_window-repair", reqs.len());
        info!(
            "{:x}: repair_window counter times: {} consumed: {} highest_lost: {} missing: {}",
            debug_id,
            *times,
            consumed,
            highest_lost,
            reqs.len()
        );
    }
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    for (to, req) in reqs {
        //todo cache socket
        debug!(
            "{:x}: repair_window request {} {} {}",
            debug_id, consumed, highest_lost, to
        );
        assert!(req.len() <= BLOB_SIZE);
        sock.send_to(&req, to)?;
    }
    Ok(())
}

fn retransmit_all_leader_blocks(
    maybe_leader: Option<NodeInfo>,
    dq: &mut SharedBlobs,
    debug_id: u64,
    recycler: &BlobRecycler,
    consumed: u64,
    received: u64,
    retransmit: &BlobSender,
) -> Result<()> {
    let mut retransmit_queue = VecDeque::new();
    if let Some(leader) = maybe_leader {
        for b in dq {
            let p = b.read().expect("'b' read lock in fn recv_window");
            //TODO this check isn't safe against adverserial packets
            //we need to maintain a sequence window
            let leader_id = leader.id;
            trace!(
                "idx: {} addr: {:?} id: {:?} leader: {:?}",
                p.get_index().expect("get_index in fn recv_window"),
                p.get_id().expect("get_id in trace! fn recv_window"),
                p.meta.addr(),
                leader_id
            );
            if p.get_id().expect("get_id in fn recv_window") == leader_id {
                //TODO
                //need to copy the retransmitted blob
                //otherwise we get into races with which thread
                //should do the recycling
                //
                //a better abstraction would be to recycle when the blob
                //is dropped via a weakref to the recycler
                let nv = recycler.allocate();
                {
                    let mut mnv = nv.write().expect("recycler write lock in fn recv_window");
                    let sz = p.meta.size;
                    mnv.meta.size = sz;
                    mnv.data[..sz].copy_from_slice(&p.data[..sz]);
                }
                retransmit_queue.push_back(nv);
            }
        }
    } else {
        warn!("{:x}: no leader to retransmit from", debug_id);
    }
    if !retransmit_queue.is_empty() {
        debug!(
            "{:x}: RECV_WINDOW {} {}: retransmit {}",
            debug_id,
            consumed,
            received,
            retransmit_queue.len(),
        );
        inc_new_counter_info!("streamer-recv_window-retransmit", retransmit_queue.len());
        retransmit.send(retransmit_queue)?;
    }
    Ok(())
}

/// process a blob: Add blob to the window. If a continuous set of blobs
///      starting from consumed is thereby formed, add that continuous
///      range of blobs to a queue to be sent on to the next stage.
///
/// * `debug_id` - this node's id in a useful-for-debug format
/// * `blob` -  the blob to be processed into the window and rebroadcast
/// * `pix` -  the index of the blob, corresponds to
///            the entry height of this blob
/// * `consume_queue` - output, blobs to be rebroadcast are placed here
/// * `window` - the window we're operating on
/// * `recycler` - where to return the blob once processed, also where
///                  to return old blobs from the window
/// * `consumed` - input/output, the entry-height to which this
///                 node has populated and rebroadcast entries
fn process_blob(
    debug_id: u64,
    blob: SharedBlob,
    pix: u64,
    consume_queue: &mut SharedBlobs,
    window: &SharedWindow,
    recycler: &BlobRecycler,
    consumed: &mut u64,
) {
    let mut window = window.write().unwrap();
    let w = (pix % WINDOW_SIZE) as usize;

    let is_coding = {
        let blob_r = blob
            .read()
            .expect("blob read lock for flogs streamer::window");
        blob_r.is_coding()
    };

    // insert a newly received blob into a window slot, clearing out and recycling any previous
    //  blob unless the incoming blob is a duplicate (based on idx)
    // returns whether the incoming is a duplicate blob
    fn insert_blob_is_dup(
        debug_id: u64,
        blob: SharedBlob,
        pix: u64,
        window_slot: &mut Option<SharedBlob>,
        recycler: &BlobRecycler,
        c_or_d: &str,
    ) -> bool {
        if let Some(old) = mem::replace(window_slot, Some(blob)) {
            let is_dup = old.read().unwrap().get_index().unwrap() == pix;
            recycler.recycle(old);
            trace!(
                "{:x}: occupied {} window slot {:}, is_dup: {}",
                debug_id,
                c_or_d,
                pix,
                is_dup
            );
            is_dup
        } else {
            trace!("{:x}: empty {} window slot {:}", debug_id, c_or_d, pix);
            false
        }
    }

    // insert the new blob into the window, overwrite and recycle old (or duplicate) entry
    let is_duplicate = if is_coding {
        insert_blob_is_dup(
            debug_id,
            blob,
            pix,
            &mut window[w].coding,
            recycler,
            "coding",
        )
    } else {
        insert_blob_is_dup(debug_id, blob, pix, &mut window[w].data, recycler, "data")
    };

    if is_duplicate {
        return;
    }

    #[cfg(feature = "erasure")]
    {
        if erasure::recover(
            debug_id,
            recycler,
            &mut window,
            *consumed,
            (*consumed % WINDOW_SIZE) as usize,
        ).is_err()
        {
            trace!("{:x}: erasure::recover failed", debug_id);
        }
    }

    // push all contiguous blobs into consumed queue, increment consumed
    loop {
        let k = (*consumed % WINDOW_SIZE) as usize;
        trace!("{:x}: k: {} consumed: {}", debug_id, k, *consumed,);

        if let Some(blob) = &window[k].data {
            if blob.read().unwrap().get_index().unwrap() < *consumed {
                // window wrap-around, end of received
                break;
            }
        } else {
            // window[k].data is None, end of received
            break;
        }
        consume_queue.push_back(window[k].data.clone().expect("clone in fn recv_window"));
        *consumed += 1;
    }
}

fn blob_idx_in_window(debug_id: u64, pix: u64, consumed: u64, received: &mut u64) -> bool {
    // Prevent receive window from running over
    // Got a blob which has already been consumed, skip it
    // probably from a repair window request
    if pix < consumed {
        trace!(
            "{:x}: received: {} but older than consumed: {} skipping..",
            debug_id,
            pix,
            consumed
        );
        false
    } else {
        // received always has to be updated even if we don't accept the packet into
        //  the window.  The worst case here is the server *starts* outside
        //  the window, none of the packets it receives fits in the window
        //  and repair requests (which are based on received) are never generated
        *received = cmp::max(pix, *received);

        if pix >= consumed + WINDOW_SIZE {
            trace!(
                "{:x}: received: {} will overrun window: {} skipping..",
                debug_id,
                pix,
                consumed + WINDOW_SIZE
            );
            false
        } else {
            true
        }
    }
}

fn recv_window(
    debug_id: u64,
    window: &SharedWindow,
    crdt: &Arc<RwLock<Crdt>>,
    recycler: &BlobRecycler,
    consumed: &mut u64,
    received: &mut u64,
    r: &BlobReceiver,
    s: &BlobSender,
    retransmit: &BlobSender,
) -> Result<()> {
    let timer = Duration::from_millis(200);
    let mut dq = r.recv_timeout(timer)?;
    let maybe_leader: Option<NodeInfo> = crdt
        .read()
        .expect("'crdt' read lock in fn recv_window")
        .leader_data()
        .cloned();
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    let now = Instant::now();
    inc_new_counter_info!("streamer-recv_window-recv", dq.len(), 100);
    debug!(
        "{:x}: RECV_WINDOW {} {}: got packets {}",
        debug_id,
        *consumed,
        *received,
        dq.len(),
    );

    retransmit_all_leader_blocks(
        maybe_leader,
        &mut dq,
        debug_id,
        recycler,
        *consumed,
        *received,
        retransmit,
    )?;

    let mut pixs = Vec::new();
    //send a contiguous set of blocks
    let mut consume_queue = VecDeque::new();
    while let Some(b) = dq.pop_front() {
        let (pix, meta_size) = {
            let p = b.write().expect("'b' write lock in fn recv_window");
            (p.get_index()?, p.meta.size)
        };
        pixs.push(pix);

        if !blob_idx_in_window(debug_id, pix, *consumed, received) {
            recycler.recycle(b);
            continue;
        }

        trace!("{:x} window pix: {} size: {}", debug_id, pix, meta_size);

        process_blob(
            debug_id,
            b,
            pix,
            &mut consume_queue,
            window,
            recycler,
            consumed,
        );
    }
    if log_enabled!(Level::Trace) {
        trace!("{}", print_window(debug_id, window, *consumed));
    }
    info!(
        "{:x}: consumed: {} received: {} sending consume.len: {} pixs: {:?} took {} ms",
        debug_id,
        *consumed,
        *received,
        consume_queue.len(),
        pixs,
        duration_as_ms(&now.elapsed())
    );
    if !consume_queue.is_empty() {
        debug!(
            "{:x}: RECV_WINDOW {} {}: forwarding consume_queue {}",
            debug_id,
            *consumed,
            *received,
            consume_queue.len(),
        );
        trace!(
            "{:x}: sending consume_queue.len: {}",
            debug_id,
            consume_queue.len()
        );
        inc_new_counter_info!("streamer-recv_window-consume", consume_queue.len());
        s.send(consume_queue)?;
    }
    Ok(())
}

pub fn print_window(debug_id: u64, window: &SharedWindow, consumed: u64) -> String {
    let pointer: Vec<_> = window
        .read()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, _v)| {
            if i == (consumed % WINDOW_SIZE) as usize {
                "V"
            } else {
                " "
            }
        })
        .collect();

    let buf: Vec<_> = window
        .read()
        .unwrap()
        .iter()
        .map(|v| {
            if v.data.is_none() && v.coding.is_none() {
                "O"
            } else if v.data.is_some() && v.coding.is_some() {
                "D"
            } else if v.data.is_some() {
                // coding.is_none()
                "d"
            } else {
                // data.is_none()
                "c"
            }
        })
        .collect();
    format!(
        "\n{:x}: WINDOW ({}): {}\n{:x}: WINDOW ({}): {}",
        debug_id,
        consumed,
        pointer.join(""),
        debug_id,
        consumed,
        buf.join("")
    )
}

pub fn default_window() -> SharedWindow {
    Arc::new(RwLock::new(vec![
        WindowSlot::default();
        WINDOW_SIZE as usize
    ]))
}

pub fn index_blobs(
    node_info: &NodeInfo,
    blobs: &[SharedBlob],
    receive_index: &mut u64,
) -> Result<()> {
    // enumerate all the blobs, those are the indices
    trace!("{:x}: INDEX_BLOBS {}", node_info.debug_id(), blobs.len());
    for (i, b) in blobs.iter().enumerate() {
        // only leader should be broadcasting
        let mut blob = b.write().expect("'blob' write lock in crdt::index_blobs");
        blob.set_id(node_info.id)
            .expect("set_id in pub fn broadcast");
        blob.set_index(*receive_index + i as u64)
            .expect("set_index in pub fn broadcast");
        blob.set_flags(0).unwrap();
    }

    Ok(())
}

/// Initialize a rebroadcast window with most recent Entry blobs
/// * `crdt` - gossip instance, used to set blob ids
/// * `blobs` - up to WINDOW_SIZE most recent blobs
/// * `entry_height` - current entry height
pub fn initialized_window(
    node_info: &NodeInfo,
    blobs: Vec<SharedBlob>,
    entry_height: u64,
) -> SharedWindow {
    let window = default_window();
    let debug_id = node_info.debug_id();

    {
        let mut win = window.write().unwrap();

        trace!(
            "{:x} initialized window entry_height:{} blobs_len:{}",
            debug_id,
            entry_height,
            blobs.len()
        );

        // Index the blobs
        let mut received = entry_height - blobs.len() as u64;
        index_blobs(&node_info, &blobs, &mut received).expect("index blobs for initial window");

        // populate the window, offset by implied index
        let diff = cmp::max(blobs.len() as isize - win.len() as isize, 0) as usize;
        for b in blobs.into_iter().skip(diff) {
            let ix = b.read().unwrap().get_index().expect("blob index");
            let pos = (ix % WINDOW_SIZE) as usize;
            trace!("{:x} caching {} at {}", debug_id, ix, pos);
            assert!(win[pos].data.is_none());
            win[pos].data = Some(b);
        }
    }

    window
}

pub fn new_window_from_entries(
    ledger_tail: &[Entry],
    entry_height: u64,
    node_info: &NodeInfo,
    blob_recycler: &BlobRecycler,
) -> SharedWindow {
    // convert to blobs
    let mut blobs = VecDeque::new();
    ledger_tail.to_blobs(&blob_recycler, &mut blobs);

    // flatten deque to vec
    let blobs: Vec<_> = blobs.into_iter().collect();
    initialized_window(&node_info, blobs, entry_height)
}

pub fn window(
    crdt: Arc<RwLock<Crdt>>,
    window: SharedWindow,
    entry_height: u64,
    recycler: BlobRecycler,
    r: BlobReceiver,
    s: BlobSender,
    retransmit: BlobSender,
) -> JoinHandle<()> {
    Builder::new()
        .name("solana-window".to_string())
        .spawn(move || {
            let mut consumed = entry_height;
            let mut received = entry_height;
            let mut last = entry_height;
            let mut times = 0;
            let debug_id = crdt.read().unwrap().debug_id();
            trace!("{:x}: RECV_WINDOW started", debug_id);
            loop {
                if let Err(e) = recv_window(
                    debug_id,
                    &window,
                    &crdt,
                    &recycler,
                    &mut consumed,
                    &mut received,
                    &r,
                    &s,
                    &retransmit,
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
                let _ = repair_window(
                    debug_id, &window, &crdt, &recycler, &mut last, &mut times, consumed, received,
                );
            }
        })
        .unwrap()
}

#[cfg(test)]
mod test {
    use crdt::{Crdt, TestNode};
    use logger;
    use packet::{Blob, BlobRecycler, Packet, PacketRecycler, Packets, PACKET_DATA_SIZE};
    use std::collections::VecDeque;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer::{blob_receiver, receiver, responder, BlobReceiver, PacketReceiver};
    use window::{
        blob_idx_in_window, calculate_highest_lost_blob_index, default_window, window, WINDOW_SIZE,
    };

    fn get_msgs(r: PacketReceiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => *num += m.read().unwrap().packets.len(),
                e => info!("error {:?}", e),
            }
            if *num == 10 {
                break;
            }
        }
    }
    #[test]
    pub fn streamer_debug() {
        write!(io::sink(), "{:?}", Packet::default()).unwrap();
        write!(io::sink(), "{:?}", Packets::default()).unwrap();
        write!(io::sink(), "{:?}", Blob::default()).unwrap();
    }
    #[test]
    pub fn streamer_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        read.set_read_timeout(Some(Duration::new(1, 0))).unwrap();

        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let pack_recycler = PacketRecycler::default();
        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(read, exit.clone(), pack_recycler.clone(), s_reader);
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let t_responder = responder(
                "streamer_send_test",
                send,
                resp_recycler.clone(),
                r_responder,
            );
            let mut msgs = VecDeque::new();
            for i in 0..10 {
                let b = resp_recycler.allocate();
                {
                    let mut w = b.write().unwrap();
                    w.data[0] = i as u8;
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&addr);
                }
                msgs.push_back(b);
            }
            s_responder.send(msgs).expect("send");
            t_responder
        };

        let mut num = 0;
        get_msgs(r_reader, &mut num);
        assert_eq!(num, 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
    }

    fn get_blobs(r: BlobReceiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => {
                    for (i, v) in m.iter().enumerate() {
                        assert_eq!(v.read().unwrap().get_index().unwrap() as usize, *num + i);
                    }
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
        let tn = TestNode::new_localhost();
        let exit = Arc::new(AtomicBool::new(false));
        let mut crdt_me = Crdt::new(tn.data.clone()).expect("Crdt::new");
        let me_id = crdt_me.my_data().id;
        crdt_me.set_leader(me_id);
        let subs = Arc::new(RwLock::new(crdt_me));

        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver = blob_receiver(
            exit.clone(),
            resp_recycler.clone(),
            tn.sockets.gossip,
            s_reader,
        ).unwrap();
        let (s_window, r_window) = channel();
        let (s_retransmit, r_retransmit) = channel();
        let win = default_window();
        let t_window = window(
            subs,
            win,
            0,
            resp_recycler.clone(),
            r_reader,
            s_window,
            s_retransmit,
        );
        let t_responder = {
            let (s_responder, r_responder) = channel();
            let t_responder = responder(
                "window_send_test",
                tn.sockets.replicate,
                resp_recycler.clone(),
                r_responder,
            );
            let mut msgs = VecDeque::new();
            for v in 0..10 {
                let i = 9 - v;
                let b = resp_recycler.allocate();
                {
                    let mut w = b.write().unwrap();
                    w.set_index(i).unwrap();
                    w.set_id(me_id).unwrap();
                    assert_eq!(i, w.get_index().unwrap());
                    w.meta.size = PACKET_DATA_SIZE;
                    w.meta.set_addr(&tn.data.contact_info.ncp);
                }
                msgs.push_back(b);
            }
            s_responder.send(msgs).expect("send");
            t_responder
        };

        let mut num = 0;
        get_blobs(r_window, &mut num);
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
    pub fn calculate_highest_lost_blob_index_test() {
        assert_eq!(calculate_highest_lost_blob_index(0, 10, 90), 90);
        assert_eq!(calculate_highest_lost_blob_index(15, 10, 90), 75);
        assert_eq!(calculate_highest_lost_blob_index(90, 10, 90), 10);
        assert_eq!(calculate_highest_lost_blob_index(90, 10, 50), 10);
        assert_eq!(calculate_highest_lost_blob_index(90, 10, 99), 10);
        assert_eq!(calculate_highest_lost_blob_index(90, 10, 101), 11);
        assert_eq!(
            calculate_highest_lost_blob_index(90, 10, 95 + WINDOW_SIZE),
            WINDOW_SIZE + 5
        );
        assert_eq!(
            calculate_highest_lost_blob_index(90, 10, 99 + WINDOW_SIZE),
            WINDOW_SIZE + 9
        );
        assert_eq!(
            calculate_highest_lost_blob_index(90, 10, 100 + WINDOW_SIZE),
            WINDOW_SIZE + 9
        );
        assert_eq!(
            calculate_highest_lost_blob_index(90, 10, 120 + WINDOW_SIZE),
            WINDOW_SIZE + 9
        );
    }

    fn wrap_blob_idx_in_window(
        debug_id: u64,
        pix: u64,
        consumed: u64,
        received: u64,
    ) -> (bool, u64) {
        let mut received = received;
        let is_in_window = blob_idx_in_window(debug_id, pix, consumed, &mut received);
        (is_in_window, received)
    }
    #[test]
    pub fn blob_idx_in_window_test() {
        assert_eq!(
            wrap_blob_idx_in_window(0, 90 + WINDOW_SIZE, 90, 100),
            (false, 90 + WINDOW_SIZE)
        );
        assert_eq!(
            wrap_blob_idx_in_window(0, 91 + WINDOW_SIZE, 90, 100),
            (false, 91 + WINDOW_SIZE)
        );
        assert_eq!(wrap_blob_idx_in_window(0, 89, 90, 100), (false, 100));

        assert_eq!(wrap_blob_idx_in_window(0, 91, 90, 100), (true, 100));
        assert_eq!(wrap_blob_idx_in_window(0, 101, 90, 100), (true, 101));
    }
}
