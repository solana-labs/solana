//! The `streamer` module defines a set of services for effecently pulling data from udp sockets.
use crdt::Crdt;
#[cfg(feature = "erasure")]
use erasure;
use packet::{Blob, BlobRecycler, PacketRecycler, SharedBlob, SharedPackets, NUM_BLOBS};
use result::Result;
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;

pub type PacketReceiver = mpsc::Receiver<SharedPackets>;
pub type PacketSender = mpsc::Sender<SharedPackets>;
pub type BlobSender = mpsc::Sender<VecDeque<SharedBlob>>;
pub type BlobReceiver = mpsc::Receiver<VecDeque<SharedBlob>>;

fn recv_loop(
    sock: &UdpSocket,
    exit: &Arc<AtomicBool>,
    re: &PacketRecycler,
    channel: &PacketSender,
) -> Result<()> {
    loop {
        let msgs = re.allocate();
        let msgs_ = msgs.clone();
        loop {
            match msgs.write().unwrap().recv_from(sock) {
                Ok(()) => {
                    channel.send(msgs_)?;
                    break;
                }
                Err(_) => {
                    if exit.load(Ordering::Relaxed) {
                        re.recycle(msgs_);
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
    channel: PacketSender,
) -> Result<JoinHandle<()>> {
    let timer = Duration::new(1, 0);
    sock.set_read_timeout(Some(timer))?;
    Ok(spawn(move || {
        let _ = recv_loop(&sock, &exit, &recycler, &channel);
        ()
    }))
}

fn recv_send(sock: &UdpSocket, recycler: &BlobRecycler, r: &BlobReceiver) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut msgs = r.recv_timeout(timer)?;
    Blob::send_to(recycler, sock, &mut msgs)?;
    Ok(())
}

pub fn responder(
    sock: UdpSocket,
    exit: Arc<AtomicBool>,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    spawn(move || loop {
        if recv_send(&sock, &recycler, &r).is_err() && exit.load(Ordering::Relaxed) {
            break;
        }
    })
}

//TODO, we would need to stick block authentication before we create the
//window.
fn recv_blobs(recycler: &BlobRecycler, sock: &UdpSocket, s: &BlobSender) -> Result<()> {
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
    let t = spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            break;
        }
        let _ = recv_blobs(&recycler, &sock, &s);
    });
    Ok(t)
}

fn recv_window(
    window: &mut Vec<Option<SharedBlob>>,
    crdt: &Arc<RwLock<Crdt>>,
    recycler: &BlobRecycler,
    consumed: &mut usize,
    r: &BlobReceiver,
    s: &BlobSender,
    retransmit: &BlobSender,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    let leader_id = crdt.read().unwrap().leader_data().id;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    {
        //retransmit all leader blocks
        let mut retransmitq = VecDeque::new();
        for b in &dq {
            let p = b.read().unwrap();
            //TODO this check isn't safe against adverserial packets
            //we need to maintain a sequence window
            trace!(
                "idx: {} addr: {:?} id: {:?} leader: {:?}",
                p.get_index().unwrap(),
                p.get_id().unwrap(),
                p.meta.addr(),
                leader_id
            );
            if p.get_id().unwrap() == leader_id {
                //TODO
                //need to copy the retransmited blob
                //otherwise we get into races with which thread
                //should do the recycling
                //
                //a better absraction would be to recycle when the blob
                //is dropped via a weakref to the recycler
                let nv = recycler.allocate();
                {
                    let mut mnv = nv.write().unwrap();
                    let sz = p.meta.size;
                    mnv.meta.size = sz;
                    mnv.data[..sz].copy_from_slice(&p.data[..sz]);
                }
                retransmitq.push_back(nv);
            }
        }
        if !retransmitq.is_empty() {
            retransmit.send(retransmitq)?;
        }
    }
    //send a contiguous set of blocks
    let mut contq = VecDeque::new();
    while let Some(b) = dq.pop_front() {
        let b_ = b.clone();
        let p = b.write().unwrap();
        let pix = p.get_index()? as usize;
        let w = pix % NUM_BLOBS;
        //TODO, after the block are authenticated
        //if we get different blocks at the same index
        //that is a network failure/attack
        trace!("window w: {} size: {}", w, p.meta.size);
        {
            if window[w].is_none() {
                window[w] = Some(b_);
            } else {
                debug!("duplicate blob at index {:}", w);
            }
            loop {
                let k = *consumed % NUM_BLOBS;
                trace!("k: {} consumed: {}", k, *consumed);
                if window[k].is_none() {
                    break;
                }
                contq.push_back(window[k].clone().unwrap());
                window[k] = None;
                *consumed += 1;
            }
        }
    }
    trace!("sending contq.len: {}", contq.len());
    if !contq.is_empty() {
        s.send(contq)?;
    }
    Ok(())
}

pub fn window(
    exit: Arc<AtomicBool>,
    crdt: Arc<RwLock<Crdt>>,
    recycler: BlobRecycler,
    r: BlobReceiver,
    s: BlobSender,
    retransmit: BlobSender,
) -> JoinHandle<()> {
    spawn(move || {
        let mut window = vec![None; NUM_BLOBS];
        let mut consumed = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = recv_window(
                &mut window,
                &crdt,
                &recycler,
                &mut consumed,
                &r,
                &s,
                &retransmit,
            );
        }
    })
}

fn broadcast(
    crdt: &Arc<RwLock<Crdt>>,
    recycler: &BlobRecycler,
    r: &BlobReceiver,
    sock: &UdpSocket,
    transmit_index: &mut u64,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq);
    }
    let mut blobs = dq.into_iter().collect();
    /// appends codes to the list of blobs allowing us to reconstruct the stream
    #[cfg(feature = "erasure")]
    erasure::generate_codes(blobs);
    Crdt::broadcast(crdt, &blobs, &sock, transmit_index)?;
    while let Some(b) = blobs.pop() {
        recycler.recycle(b);
    }
    Ok(())
}

/// Service to broadcast messages from the leader to layer 1 nodes.
/// See `crdt` for network layer definitions.
/// # Arguments
/// * `sock` - Socket to send from.
/// * `exit` - Boolean to signal system exit.
/// * `crdt` - CRDT structure
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
pub fn broadcaster(
    sock: UdpSocket,
    exit: Arc<AtomicBool>,
    crdt: Arc<RwLock<Crdt>>,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    spawn(move || {
        let mut transmit_index = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = broadcast(&crdt, &recycler, &r, &sock, &mut transmit_index);
        }
    })
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
/// * `crdt` - This structure needs to be updated and populated by the accountant and via gossip.
/// * `recycler` - Blob recycler.
/// * `r` - Receive channel for blobs to be retransmitted to all the layer 1 nodes.
pub fn retransmitter(
    sock: UdpSocket,
    exit: Arc<AtomicBool>,
    crdt: Arc<RwLock<Crdt>>,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    spawn(move || {
        trace!("retransmitter started");
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            // TODO: handle this error
            let _ = retransmit(&crdt, &recycler, &r, &sock);
        }
        trace!("exiting retransmitter");
    })
}

#[cfg(all(feature = "unstable", test))]
mod bench {
    extern crate test;
    use self::test::Bencher;
    use packet::{Packet, PacketRecycler, PACKET_DATA_SIZE};
    use result::Result;
    use std::net::{SocketAddr, UdpSocket};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    use std::thread::sleep;
    use std::thread::{spawn, JoinHandle};
    use std::time::Duration;
    use std::time::SystemTime;
    use streamer::{receiver, PacketReceiver};

    fn producer(
        addr: &SocketAddr,
        recycler: PacketRecycler,
        exit: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let send = UdpSocket::bind("0.0.0.0:0").unwrap();
        let msgs = recycler.allocate();
        let msgs_ = msgs.clone();
        msgs.write().unwrap().packets.resize(10, Packet::default());
        for w in msgs.write().unwrap().packets.iter_mut() {
            w.meta.size = PACKET_DATA_SIZE;
            w.meta.set_addr(&addr);
        }
        spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                return;
            }
            let mut num = 0;
            for p in msgs_.read().unwrap().packets.iter() {
                let a = p.meta.addr();
                send.send_to(&p.data[..p.meta.size], &a).unwrap();
                num += 1;
            }
            assert_eq!(num, 10);
        })
    }

    fn sink(
        recycler: PacketRecycler,
        exit: Arc<AtomicBool>,
        rvs: Arc<Mutex<usize>>,
        r: PacketReceiver,
    ) -> JoinHandle<()> {
        spawn(move || loop {
            if exit.load(Ordering::Relaxed) {
                return;
            }
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(msgs) => {
                    let msgs_ = msgs.clone();
                    *rvs.lock().unwrap() += msgs.read().unwrap().packets.len();
                    recycler.recycle(msgs_);
                }
                _ => (),
            }
        })
    }
    fn run_streamer_bench() -> Result<()> {
        let read = UdpSocket::bind("127.0.0.1:0")?;
        let addr = read.local_addr()?;
        let exit = Arc::new(AtomicBool::new(false));
        let pack_recycler = PacketRecycler::default();

        let (s_reader, r_reader) = channel();
        let t_reader = receiver(read, exit.clone(), pack_recycler.clone(), s_reader)?;
        let t_producer1 = producer(&addr, pack_recycler.clone(), exit.clone());
        let t_producer2 = producer(&addr, pack_recycler.clone(), exit.clone());
        let t_producer3 = producer(&addr, pack_recycler.clone(), exit.clone());

        let rvs = Arc::new(Mutex::new(0));
        let t_sink = sink(pack_recycler.clone(), exit.clone(), rvs.clone(), r_reader);

        let start = SystemTime::now();
        let start_val = *rvs.lock().unwrap();
        sleep(Duration::new(5, 0));
        let elapsed = start.elapsed().unwrap();
        let end_val = *rvs.lock().unwrap();
        let time = elapsed.as_secs() * 10000000000 + elapsed.subsec_nanos() as u64;
        let ftime = (time as f64) / 10000000000f64;
        let fcount = (end_val - start_val) as f64;
        trace!("performance: {:?}", fcount / ftime);
        exit.store(true, Ordering::Relaxed);
        t_reader.join()?;
        t_producer1.join()?;
        t_producer2.join()?;
        t_producer3.join()?;
        t_sink.join()?;
        Ok(())
    }
    #[bench]
    pub fn streamer_bench(_bench: &mut Bencher) {
        run_streamer_bench().unwrap();
    }
}

#[cfg(test)]
mod test {
    use crdt::{Crdt, ReplicatedData};
    use logger;
    use packet::{Blob, BlobRecycler, Packet, PacketRecycler, Packets, PACKET_DATA_SIZE};
    use signature::KeyPair;
    use signature::KeyPairUtil;
    use std::collections::VecDeque;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::thread::sleep;
    use std::time::Duration;
    use streamer::{blob_receiver, receiver, responder, retransmitter, window};
    use streamer::{BlobReceiver, PacketReceiver};

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
        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let pack_recycler = PacketRecycler::default();
        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver = receiver(read, exit.clone(), pack_recycler.clone(), s_reader).unwrap();
        let (s_responder, r_responder) = channel();
        let t_responder = responder(send, exit.clone(), resp_recycler.clone(), r_responder);
        let mut msgs = VecDeque::new();
        for i in 0..10 {
            let b = resp_recycler.allocate();
            let b_ = b.clone();
            let mut w = b.write().unwrap();
            w.data[0] = i as u8;
            w.meta.size = PACKET_DATA_SIZE;
            w.meta.set_addr(&addr);
            msgs.push_back(b_);
        }
        s_responder.send(msgs).expect("send");
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
        let pubkey_me = KeyPair::new().pubkey();
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let serve = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let rep_data = ReplicatedData::new(
            pubkey_me,
            read.local_addr().unwrap(),
            send.local_addr().unwrap(),
            serve.local_addr().unwrap(),
        );
        let mut crdt_me = Crdt::new(rep_data);
        let me_id = crdt_me.my_data().id;
        crdt_me.set_leader(me_id);
        let subs = Arc::new(RwLock::new(crdt_me));

        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver =
            blob_receiver(exit.clone(), resp_recycler.clone(), read, s_reader).unwrap();
        let (s_window, r_window) = channel();
        let (s_retransmit, r_retransmit) = channel();
        let t_window = window(
            exit.clone(),
            subs,
            resp_recycler.clone(),
            r_reader,
            s_window,
            s_retransmit,
        );
        let (s_responder, r_responder) = channel();
        let t_responder = responder(send, exit.clone(), resp_recycler.clone(), r_responder);
        let mut msgs = VecDeque::new();
        for v in 0..10 {
            let i = 9 - v;
            let b = resp_recycler.allocate();
            let b_ = b.clone();
            let mut w = b.write().unwrap();
            w.set_index(i).unwrap();
            w.set_id(me_id).unwrap();
            assert_eq!(i, w.get_index().unwrap());
            w.meta.size = PACKET_DATA_SIZE;
            w.meta.set_addr(&addr);
            msgs.push_back(b_);
        }
        s_responder.send(msgs).expect("send");
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

    fn test_node() -> (Arc<RwLock<Crdt>>, UdpSocket, UdpSocket, UdpSocket) {
        let gossip = UdpSocket::bind("127.0.0.1:0").unwrap();
        let replicate = UdpSocket::bind("127.0.0.1:0").unwrap();
        let serve = UdpSocket::bind("127.0.0.1:0").unwrap();
        let pubkey = KeyPair::new().pubkey();
        let d = ReplicatedData::new(
            pubkey,
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            serve.local_addr().unwrap(),
        );
        let crdt = Crdt::new(d);
        trace!(
            "id: {} gossip: {} replicate: {} serve: {}",
            crdt.my_data().id[0],
            gossip.local_addr().unwrap(),
            replicate.local_addr().unwrap(),
            serve.local_addr().unwrap(),
        );
        (Arc::new(RwLock::new(crdt)), gossip, replicate, serve)
    }

    #[test]
    //retransmit from leader to replicate target
    pub fn retransmit() {
        logger::setup();
        trace!("retransmit test start");
        let exit = Arc::new(AtomicBool::new(false));
        let (crdt_leader, sock_gossip_leader, _, sock_leader) = test_node();
        let (crdt_target, sock_gossip_target, sock_replicate_target, _) = test_node();
        let leader_data = crdt_leader.read().unwrap().my_data().clone();
        crdt_leader.write().unwrap().insert(leader_data.clone());
        crdt_leader.write().unwrap().set_leader(leader_data.id);
        let t_crdt_leader_g = Crdt::gossip(crdt_leader.clone(), exit.clone());
        let t_crdt_leader_l = Crdt::listen(crdt_leader.clone(), sock_gossip_leader, exit.clone());

        crdt_target.write().unwrap().insert(leader_data.clone());
        crdt_target.write().unwrap().set_leader(leader_data.id);
        let t_crdt_target_g = Crdt::gossip(crdt_target.clone(), exit.clone());
        let t_crdt_target_l = Crdt::listen(crdt_target.clone(), sock_gossip_target, exit.clone());
        //leader retransmitter
        let (s_retransmit, r_retransmit) = channel();
        let blob_recycler = BlobRecycler::default();
        let saddr = sock_leader.local_addr().unwrap();
        let t_retransmit = retransmitter(
            sock_leader,
            exit.clone(),
            crdt_leader.clone(),
            blob_recycler.clone(),
            r_retransmit,
        );

        //target receiver
        let (s_blob_receiver, r_blob_receiver) = channel();
        let t_receiver = blob_receiver(
            exit.clone(),
            blob_recycler.clone(),
            sock_replicate_target,
            s_blob_receiver,
        ).unwrap();
        for _ in 0..10 {
            let done = crdt_target.read().unwrap().update_index == 2
                && crdt_leader.read().unwrap().update_index == 2;
            if done {
                break;
            }
            let timer = Duration::new(1, 0);
            sleep(timer);
        }

        //send the data through
        let mut bq = VecDeque::new();
        let b = blob_recycler.allocate();
        b.write().unwrap().meta.size = 10;
        bq.push_back(b);
        s_retransmit.send(bq).unwrap();
        let timer = Duration::new(5, 0);
        trace!("Waiting for timeout");
        let mut oq = r_blob_receiver.recv_timeout(timer).unwrap();
        assert_eq!(oq.len(), 1);
        let o = oq.pop_front().unwrap();
        let ro = o.read().unwrap();
        assert_eq!(ro.meta.size, 10);
        assert_eq!(ro.meta.addr(), saddr);
        exit.store(true, Ordering::Relaxed);
        let threads = vec![
            t_receiver,
            t_retransmit,
            t_crdt_target_g,
            t_crdt_target_l,
            t_crdt_leader_g,
            t_crdt_leader_l,
        ];
        for t in threads {
            t.join().unwrap();
        }
    }

}
