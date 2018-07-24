//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!
use counter::Counter;
use crdt::{Crdt, CrdtError, NodeInfo};
#[cfg(feature = "erasure")]
use erasure;
use packet::{
    Blob, BlobRecycler, PacketRecycler, SharedBlob, SharedBlobs, SharedPackets, BLOB_SIZE,
};
use result::{Error, Result};
use std::cmp;
use std::collections::VecDeque;
use std::mem;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, RwLock};
use std::thread::{Builder, JoinHandle};
use std::time::Duration;

pub const WINDOW_SIZE: u64 = 2 * 1024;
pub type PacketReceiver = Receiver<SharedPackets>;
pub type PacketSender = Sender<SharedPackets>;
pub type BlobSender = Sender<SharedBlobs>;
pub type BlobReceiver = Receiver<SharedBlobs>;

#[derive(Clone, Default)]
pub struct WindowSlot {
    pub data: Option<SharedBlob>,
    pub coding: Option<SharedBlob>,
}

pub type Window = Arc<RwLock<Vec<WindowSlot>>>;

#[derive(Debug, PartialEq, Eq)]
pub enum WindowError {
    GenericError,
}

fn recv_loop(
    sock: &UdpSocket,
    exit: &Arc<AtomicBool>,
    re: &PacketRecycler,
    channel: &PacketSender,
) -> Result<()> {
    loop {
        let msgs = re.allocate();
        loop {
            let result = msgs.write()
                .expect("write lock in fn recv_loop")
                .recv_from(sock);
            match result {
                Ok(()) => {
                    channel.send(msgs)?;
                    break;
                }
                Err(_) => {
                    if exit.load(Ordering::Relaxed) {
                        re.recycle(msgs);
                        return Ok(());
                    }
                }
            }
        }
    }
}

pub fn receiver(
    sock: UdpSocket,
    exit: Arc<AtomicBool>,
    recycler: PacketRecycler,
    packet_sender: PacketSender,
) -> JoinHandle<()> {
    let res = sock.set_read_timeout(Some(Duration::new(1, 0)));
    if res.is_err() {
        panic!("streamer::receiver set_read_timeout error");
    }
    Builder::new()
        .name("solana-receiver".to_string())
        .spawn(move || {
            let _ = recv_loop(&sock, &exit, &recycler, &packet_sender);
            ()
        })
        .unwrap()
}

fn recv_send(sock: &UdpSocket, recycler: &BlobRecycler, r: &BlobReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut msgs = r.recv_timeout(timer)?;
    Blob::send_to(recycler, sock, &mut msgs)?;
    Ok(())
}

pub fn recv_batch(recvr: &PacketReceiver) -> Result<(Vec<SharedPackets>, usize)> {
    let timer = Duration::new(1, 0);
    let msgs = recvr.recv_timeout(timer)?;
    trace!("got msgs");
    let mut len = msgs.read().unwrap().packets.len();
    let mut batch = vec![msgs];
    while let Ok(more) = recvr.try_recv() {
        trace!("got more msgs");
        len += more.read().unwrap().packets.len();
        batch.push(more);

        if len > 100_000 {
            break;
        }
    }
    trace!("batch len {}", batch.len());
    Ok((batch, len))
}

pub fn responder(
    name: &'static str,
    sock: UdpSocket,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    Builder::new()
        .name(format!("solana-responder-{}", name))
        .spawn(move || loop {
            if let Err(e) = recv_send(&sock, &recycler, &r) {
                match e {
                    Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                    Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                    _ => warn!("{} responder error: {:?}", name, e),
                }
            }
        })
        .unwrap()
}

//TODO, we would need to stick block authentication before we create the
//window.
fn recv_blobs(recycler: &BlobRecycler, sock: &UdpSocket, s: &BlobSender) -> Result<()> {
    trace!("recv_blobs: receiving on {}", sock.local_addr().unwrap());
    let dq = Blob::recv_from(recycler, sock)?;
    if !dq.is_empty() {
        s.send(dq)?;
    }
    Ok(())
}

pub fn blob_receiver(
    exit: Arc<AtomicBool>,
    recycler: BlobRecycler,
    sock: UdpSocket,
    s: BlobSender,
) -> Result<JoinHandle<()>> {
    //DOCUMENTED SIDE-EFFECT
    //1 second timeout on socket read
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))?;
    let t = Builder::new()
        .name("solana-blob_receiver".to_string())
        .spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = recv_blobs(&recycler, &sock, &s);
        })
        .unwrap();
    Ok(t)
}

fn find_next_missing(
    window: &Window,
    crdt: &Arc<RwLock<Crdt>>,
    consumed: u64,
    received: u64,
) -> Result<Vec<(SocketAddr, Vec<u8>)>> {
    if received <= consumed {
        Err(WindowError::GenericError)?;
    }
    let window = window.read().unwrap();
    let reqs: Vec<_> = (consumed..received)
        .filter_map(|pix| {
            let i = (pix % WINDOW_SIZE) as usize;
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

fn repair_window(
    debug_id: u64,
    window: &Window,
    crdt: &Arc<RwLock<Crdt>>,
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

    let reqs = find_next_missing(window, crdt, consumed, received)?;
    trace!("{:x}: repair_window missing: {}", debug_id, reqs.len());
    if !reqs.is_empty() {
        inc_new_counter!("streamer-repair_window-repair", reqs.len());
        debug!(
            "{:x}: repair_window counter times: {} consumed: {} received: {} missing: {}",
            debug_id,
            *times,
            consumed,
            received,
            reqs.len()
        );
    }
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    for (to, req) in reqs {
        //todo cache socket
        debug!(
            "{:x}: repair_window request {} {} {}",
            debug_id, consumed, received, to
        );
        assert!(req.len() < BLOB_SIZE);
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
        inc_new_counter!("streamer-recv_window-retransmit", retransmit_queue.len());
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
    window: &Window,
    recycler: &BlobRecycler,
    consumed: &mut u64,
) {
    let mut window = window.write().unwrap();
    let w = (pix % WINDOW_SIZE) as usize;

    let is_coding = {
        let blob_r = blob.read()
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
            if old.read().unwrap().get_index().unwrap() == pix {
                trace!(
                    "{:x}: duplicate {} blob at index {:}",
                    debug_id,
                    c_or_d,
                    pix
                );
            }
            trace!(
                "{:x}: recycling {} blob at index {:}",
                debug_id,
                c_or_d,
                pix
            );
            recycler.recycle(old);
            true
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

fn recv_window(
    debug_id: u64,
    window: &Window,
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
    let maybe_leader: Option<NodeInfo> = crdt.read()
        .expect("'crdt' read lock in fn recv_window")
        .leader_data()
        .cloned();
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    inc_new_counter!("streamer-recv_window-recv", dq.len());
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

    //send a contiguous set of blocks
    let mut consume_queue = VecDeque::new();
    while let Some(b) = dq.pop_front() {
        let (pix, meta_size) = {
            let p = b.write().expect("'b' write lock in fn recv_window");
            (p.get_index()?, p.meta.size)
        };
        if pix > *received {
            *received = pix;
        }
        // Got a blob which has already been consumed, skip it
        // probably from a repair window request
        if pix < *consumed {
            debug!(
                "{:x}: received: {} but older than consumed: {} skipping..",
                debug_id, pix, *consumed
            );
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
    print_window(debug_id, window, *consumed);
    trace!(
        "{:x}: sending consume_queue.len: {}",
        debug_id,
        consume_queue.len()
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
        inc_new_counter!("streamer-recv_window-consume", consume_queue.len());
        s.send(consume_queue)?;
    }
    Ok(())
}

fn print_window(debug_id: u64, window: &Window, consumed: u64) {
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
    trace!(
        "{:x}: WINDOW ({}): {}",
        debug_id,
        consumed,
        pointer.join("")
    );
    trace!("{:x}: WINDOW ({}): {}", debug_id, consumed, buf.join(""));
}

pub fn default_window() -> Window {
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
) -> Window {
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

pub fn window(
    crdt: Arc<RwLock<Crdt>>,
    window: Window,
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
                            inc_new_counter!("streamer-window-error", 1, 1);
                            error!("window error: {:?}", e);
                        }
                    }
                }
                let _ = repair_window(
                    debug_id, &window, &crdt, &mut last, &mut times, consumed, received,
                );
            }
        })
        .unwrap()
}

fn broadcast(
    node_info: &NodeInfo,
    broadcast_table: &[NodeInfo],
    window: &Window,
    recycler: &BlobRecycler,
    r: &BlobReceiver,
    sock: &UdpSocket,
    transmit_index: &mut u64,
    receive_index: &mut u64,
) -> Result<()> {
    let debug_id = node_info.debug_id();
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq);
    }

    // flatten deque to vec
    let blobs_vec: Vec<_> = dq.into_iter().collect();

    // We could receive more blobs than window slots so
    // break them up into window-sized chunks to process
    let blobs_chunked = blobs_vec.chunks(WINDOW_SIZE as usize).map(|x| x.to_vec());

    print_window(debug_id, window, *receive_index);

    for mut blobs in blobs_chunked {
        let blobs_len = blobs.len();
        trace!("{:x}: broadcast blobs.len: {}", debug_id, blobs_len);

        // Index the blobs
        index_blobs(node_info, &blobs, receive_index).expect("index blobs for initial window");

        // keep the cache of blobs that are broadcast
        inc_new_counter!("streamer-broadcast-sent", blobs.len());
        {
            let mut win = window.write().unwrap();
            assert!(blobs.len() <= win.len());
            for b in &blobs {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                if let Some(x) = mem::replace(&mut win[pos].data, None) {
                    trace!(
                        "{:x} popped {} at {}",
                        debug_id,
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                    recycler.recycle(x);
                }
                if let Some(x) = mem::replace(&mut win[pos].coding, None) {
                    trace!(
                        "{:x} popped {} at {}",
                        debug_id,
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                    recycler.recycle(x);
                }

                trace!("{:x} null {}", debug_id, pos);
            }
            while let Some(b) = blobs.pop() {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                trace!("{:x} caching {} at {}", debug_id, ix, pos);
                assert!(win[pos].data.is_none());
                win[pos].data = Some(b);
            }
        }

        // Fill in the coding blob data from the window data blobs
        #[cfg(feature = "erasure")]
        {
            erasure::generate_coding(
                debug_id,
                &mut window.write().unwrap(),
                recycler,
                (*receive_index % WINDOW_SIZE) as usize,
                blobs_len,
            )?;
        }

        *receive_index += blobs_len as u64;

        // Send blobs out from the window
        Crdt::broadcast(
            &node_info,
            &broadcast_table,
            &window,
            &sock,
            transmit_index,
            *receive_index,
        )?;
    }
    Ok(())
}

/// Service to broadcast messages from the leader to layer 1 nodes.
/// See `crdt` for network layer definitions.
/// # Arguments
/// * `sock` - Socket to send from.
/// * `exit` - Boolean to signal system exit.
/// * `crdt` - CRDT structure
/// * `window` - Cache of blobs that we have broadcast
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
pub fn broadcaster(
    sock: UdpSocket,
    crdt: Arc<RwLock<Crdt>>,
    window: Window,
    entry_height: u64,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    Builder::new()
        .name("solana-broadcaster".to_string())
        .spawn(move || {
            let mut transmit_index = entry_height;
            let mut receive_index = entry_height;
            let me = crdt.read().unwrap().my_data().clone();
            loop {
                let broadcast_table = crdt.read().unwrap().compute_broadcast_table();
                if let Err(e) = broadcast(
                    &me,
                    &broadcast_table,
                    &window,
                    &recycler,
                    &r,
                    &sock,
                    &mut transmit_index,
                    &mut receive_index,
                ) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        Error::CrdtError(CrdtError::NoPeers) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                        _ => {
                            inc_new_counter!("streamer-broadcaster-error", 1, 1);
                            error!("broadcaster error: {:?}", e);
                        }
                    }
                }
            }
        })
        .unwrap()
}

fn retransmit(
    crdt: &Arc<RwLock<Crdt>>,
    recycler: &BlobRecycler,
    r: &BlobReceiver,
    sock: &UdpSocket,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq);
    }
    {
        for b in &dq {
            Crdt::retransmit(&crdt, b, sock)?;
        }
    }
    while let Some(b) = dq.pop_front() {
        recycler.recycle(b);
    }
    Ok(())
}

/// Service to retransmit messages from the leader to layer 1 nodes.
/// See `crdt` for network layer definitions.
/// # Arguments
/// * `sock` - Socket to read from.  Read timeout is set to 1.
/// * `exit` - Boolean to signal system exit.
/// * `crdt` - This structure needs to be updated and populated by the bank and via gossip.
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
pub fn retransmitter(
    sock: UdpSocket,
    crdt: Arc<RwLock<Crdt>>,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    Builder::new()
        .name("solana-retransmitter".to_string())
        .spawn(move || {
            trace!("retransmitter started");
            loop {
                if let Err(e) = retransmit(&crdt, &recycler, &r, &sock) {
                    match e {
                        Error::RecvTimeoutError(RecvTimeoutError::Disconnected) => break,
                        Error::RecvTimeoutError(RecvTimeoutError::Timeout) => (),
                        _ => {
                            inc_new_counter!("streamer-retransmit-error", 1, 1);
                            error!("retransmitter error: {:?}", e);
                        }
                    }
                }
            }
            trace!("exiting retransmitter");
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
    use streamer::{blob_receiver, receiver, responder, window};
    use streamer::{default_window, BlobReceiver, PacketReceiver};

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
}
