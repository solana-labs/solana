//! The `streamer` module defines a set of services for efficiently pulling data from UDP sockets.
//!
use counter::Counter;
use crdt::{Crdt, CrdtError, ReplicatedData};
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

const LOG_RATE: usize = 10;
pub const WINDOW_SIZE: u64 = 2 * 1024;
pub type PacketReceiver = Receiver<SharedPackets>;
pub type PacketSender = Sender<SharedPackets>;
pub type BlobSender = Sender<SharedBlobs>;
pub type BlobReceiver = Receiver<SharedBlobs>;
pub type Window = Arc<RwLock<Vec<Option<SharedBlob>>>>;

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
                    _ => error!("{} responder error: {:?}", name, e),
                }
            }
        })
        .unwrap()
}

//TODO, we would need to stick block authentication before we create the
//window.
fn recv_blobs(recycler: &BlobRecycler, sock: &UdpSocket, s: &BlobSender) -> Result<()> {
    trace!("receiving on {}", sock.local_addr().unwrap());
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
    locked_window: &Window,
    crdt: &Arc<RwLock<Crdt>>,
    consumed: &mut u64,
    received: &mut u64,
) -> Result<Vec<(SocketAddr, Vec<u8>)>> {
    if *received <= *consumed {
        Err(WindowError::GenericError)?;
    }
    let window = locked_window.read().unwrap();
    let reqs: Vec<_> = (*consumed..*received)
        .filter_map(|pix| {
            let i = (pix % WINDOW_SIZE) as usize;
            if let &None = &window[i] {
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
    locked_window: &Window,
    crdt: &Arc<RwLock<Crdt>>,
    _recycler: &BlobRecycler,
    last: &mut u64,
    times: &mut usize,
    consumed: &mut u64,
    received: &mut u64,
) -> Result<()> {
    #[cfg(feature = "erasure")]
    {
        if erasure::recover(
            _recycler,
            &mut locked_window.write().unwrap(),
            *consumed as usize,
            *received as usize,
        ).is_err()
        {
            trace!("erasure::recover failed");
        }
    }
    //exponential backoff
    if *last != *consumed {
        *times = 0;
    }
    *last = *consumed;
    *times += 1;
    //if times flips from all 1s 7 -> 8, 15 -> 16, we retry otherwise return Ok
    if *times & (*times - 1) != 0 {
        trace!(
            "repair_window counter {} {} {}",
            *times,
            *consumed,
            *received
        );
        return Ok(());
    }

    let reqs = find_next_missing(locked_window, crdt, consumed, received)?;
    trace!("{:x}: repair_window missing: {}", debug_id, reqs.len());
    if reqs.len() > 0 {
        static mut COUNTER_REPAIR: Counter =
            create_counter!("streamer-repair_window-repair", LOG_RATE);
        inc_counter!(COUNTER_REPAIR, reqs.len());
        debug!(
            "{:x}: repair_window counter times: {} consumed: {} received: {} missing: {}",
            debug_id,
            *times,
            *consumed,
            *received,
            reqs.len()
        );
    }
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    for (to, req) in reqs {
        //todo cache socket
        debug!(
            "{:x} repair_window request {} {} {}",
            debug_id, *consumed, *received, to
        );
        assert!(req.len() < BLOB_SIZE);
        sock.send_to(&req, to)?;
    }
    Ok(())
}

fn recv_window(
    debug_id: u64,
    locked_window: &Window,
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
    let maybe_leader: Option<ReplicatedData> = crdt.read()
        .expect("'crdt' read lock in fn recv_window")
        .leader_data()
        .cloned();
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    static mut COUNTER_RECV: Counter = create_counter!("streamer-recv_window-recv", LOG_RATE);
    inc_counter!(COUNTER_RECV, dq.len());
    debug!(
        "{:x}: RECV_WINDOW {} {}: got packets {}",
        debug_id,
        *consumed,
        *received,
        dq.len(),
    );
    {
        //retransmit all leader blocks
        let mut retransmit_queue = VecDeque::new();
        if let Some(leader) = maybe_leader {
            for b in &dq {
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
                *consumed,
                *received,
                retransmit_queue.len(),
            );
            static mut COUNTER_RETRANSMIT: Counter =
                create_counter!("streamer-recv_window-retransmit", LOG_RATE);
            inc_counter!(COUNTER_RETRANSMIT, retransmit_queue.len());
            retransmit.send(retransmit_queue)?;
        }
    }
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
        let w = (pix % WINDOW_SIZE) as usize;
        //TODO, after the block are authenticated
        //if we get different blocks at the same index
        //that is a network failure/attack
        trace!("window w: {} size: {}", w, meta_size);
        {
            let mut window = locked_window.write().unwrap();

            // Search the window for old blobs in the window
            // of consumed to received and clear any old ones
            for ix in *consumed..(pix + 1) {
                let k = (ix % WINDOW_SIZE) as usize;
                if let Some(b) = &mut window[k] {
                    if b.read().unwrap().get_index().unwrap() >= *consumed as u64 {
                        continue;
                    }
                }
                if let Some(b) = mem::replace(&mut window[k], None) {
                    recycler.recycle(b);
                }
            }

            // Insert the new blob into the window
            // spot should be free because we cleared it above
            if window[w].is_none() {
                window[w] = Some(b);
            } else if let Some(cblob) = &window[w] {
                if cblob.read().unwrap().get_index().unwrap() != pix as u64 {
                    warn!("{:x}: overrun blob at index {:}", debug_id, w);
                } else {
                    debug!("{:x}: duplicate blob at index {:}", debug_id, w);
                }
            }
            loop {
                let k = (*consumed % WINDOW_SIZE) as usize;
                trace!("k: {} consumed: {}", k, *consumed);

                if window[k].is_none() {
                    break;
                }
                let mut is_coding = false;
                if let &Some(ref cblob) = &window[k] {
                    let cblob_r = cblob
                        .read()
                        .expect("blob read lock for flogs streamer::window");
                    if cblob_r.get_index().unwrap() < *consumed {
                        break;
                    }
                    if cblob_r.is_coding() {
                        is_coding = true;
                    }
                }
                if !is_coding {
                    consume_queue.push_back(window[k].clone().expect("clone in fn recv_window"));
                    *consumed += 1;
                } else {
                    #[cfg(feature = "erasure")]
                    {
                        let block_start = *consumed - (*consumed % erasure::NUM_CODED as u64);
                        let coding_end = block_start + erasure::NUM_CODED as u64;
                        // We've received all this block's data blobs, go and null out the window now
                        for j in block_start..*consumed {
                            if let Some(b) =
                                mem::replace(&mut window[(j % WINDOW_SIZE) as usize], None)
                            {
                                recycler.recycle(b);
                            }
                        }
                        for j in *consumed..coding_end {
                            window[(j % WINDOW_SIZE) as usize] = None;
                        }

                        *consumed += erasure::MAX_MISSING as u64;
                        debug!(
                            "skipping processing coding blob k: {} consumed: {}",
                            k, *consumed
                        );
                    }
                }
            }
        }
    }
    print_window(debug_id, locked_window, *consumed);
    trace!("sending consume_queue.len: {}", consume_queue.len());
    if !consume_queue.is_empty() {
        debug!(
            "{:x}: RECV_WINDOW {} {}: forwarding consume_queue {}",
            debug_id,
            *consumed,
            *received,
            consume_queue.len(),
        );
        trace!("sending consume_queue.len: {}", consume_queue.len());
        static mut COUNTER_CONSUME: Counter =
            create_counter!("streamer-recv_window-consume", LOG_RATE);
        inc_counter!(COUNTER_CONSUME, consume_queue.len());
        s.send(consume_queue)?;
    }
    Ok(())
}

fn print_window(debug_id: u64, locked_window: &Window, consumed: u64) {
    {
        let buf: Vec<_> = locked_window
            .read()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, v)| {
                if i == (consumed % WINDOW_SIZE) as usize {
                    "_"
                } else if v.is_none() {
                    "0"
                } else {
                    if let &Some(ref cblob) = &v {
                        if cblob.read().unwrap().is_coding() {
                            "C"
                        } else {
                            "1"
                        }
                    } else {
                        "0"
                    }
                }
            })
            .collect();
        trace!("{:x}:WINDOW ({}): {}", debug_id, consumed, buf.join(""));
    }
}

pub fn default_window() -> Window {
    Arc::new(RwLock::new(vec![None; WINDOW_SIZE as usize]))
}

/// Initialize a rebroadcast window with most recent Entry blobs
/// * `crdt` - gossip instance, used to set blob ids
/// * `blobs` - up to WINDOW_SIZE most recent blobs
/// * `entry_height` - current entry height
pub fn initialized_window(
    crdt: &Arc<RwLock<Crdt>>,
    blobs: Vec<SharedBlob>,
    entry_height: u64,
) -> Window {
    let window = default_window();

    {
        let mut win = window.write().unwrap();
        let me = crdt.read().unwrap().my_data().clone();

        debug!(
            "initialized window entry_height:{} blobs_len:{}",
            entry_height,
            blobs.len()
        );

        // Index the blobs
        let mut received = entry_height - blobs.len() as u64;
        Crdt::index_blobs(&me, &blobs, &mut received).expect("index blobs for initial window");

        // populate the window, offset by implied index
        let diff = cmp::max(blobs.len() as isize - win.len() as isize, 0) as usize;
        for b in blobs.into_iter().skip(diff) {
            let ix = b.read().unwrap().get_index().expect("blob index");
            let pos = (ix % WINDOW_SIZE) as usize;
            trace!("caching {} at {}", ix, pos);
            assert!(win[pos].is_none());
            win[pos] = Some(b);
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
                        _ => error!("window error: {:?}", e),
                    }
                }
                let _ = repair_window(
                    debug_id,
                    &window,
                    &crdt,
                    &recycler,
                    &mut last,
                    &mut times,
                    &mut consumed,
                    &mut received,
                );
                assert!(consumed <= (received + 1));
            }
        })
        .unwrap()
}

fn broadcast(
    me: &ReplicatedData,
    broadcast_table: &Vec<ReplicatedData>,
    window: &Window,
    recycler: &BlobRecycler,
    r: &BlobReceiver,
    sock: &UdpSocket,
    transmit_index: &mut u64,
    receive_index: &mut u64,
) -> Result<()> {
    let debug_id = me.debug_id();
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

    print_window(me.debug_id(), window, *receive_index);

    for mut blobs in blobs_chunked {
        // Insert the coding blobs into the blob stream
        #[cfg(feature = "erasure")]
        erasure::add_coding_blobs(recycler, &mut blobs, *receive_index);

        let blobs_len = blobs.len();
        debug!("{:x} broadcast blobs.len: {}", debug_id, blobs_len);

        // Index the blobs
        Crdt::index_blobs(&me, &blobs, receive_index)?;
        // keep the cache of blobs that are broadcast
        static mut COUNTER_BROADCAST: Counter =
            create_counter!("streamer-broadcast-sent", LOG_RATE);
        inc_counter!(COUNTER_BROADCAST, blobs.len());
        {
            let mut win = window.write().unwrap();
            assert!(blobs.len() <= win.len());
            for b in &blobs {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                if let Some(x) = mem::replace(&mut win[pos], None) {
                    trace!(
                        "popped {} at {}",
                        x.read().unwrap().get_index().unwrap(),
                        pos
                    );
                    recycler.recycle(x);
                }
                trace!("null {}", pos);
            }
            while let Some(b) = blobs.pop() {
                let ix = b.read().unwrap().get_index().expect("blob index");
                let pos = (ix % WINDOW_SIZE) as usize;
                trace!("caching {} at {}", ix, pos);
                assert!(win[pos].is_none());
                win[pos] = Some(b);
            }
        }

        // Fill in the coding blob data from the window data blobs
        #[cfg(feature = "erasure")]
        {
            erasure::generate_coding(
                &mut window.write().unwrap(),
                *receive_index as usize,
                blobs_len,
            )?;
        }

        *receive_index += blobs_len as u64;

        // Send blobs out from the window
        Crdt::broadcast(
            &me,
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
                        Error::CrdtError(CrdtError::TooSmall) => (), // TODO: Why are the unit-tests throwing hundreds of these?
                        _ => error!("broadcaster error: {:?}", e),
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
                        _ => error!("retransmitter error: {:?}", e),
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
        let tn = TestNode::new();
        let exit = Arc::new(AtomicBool::new(false));
        let mut crdt_me = Crdt::new(tn.data.clone());
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
