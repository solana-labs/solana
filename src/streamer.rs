use packet::{Blob, BlobRecycler, PacketRecycler, SharedBlob, SharedPackets, NUM_BLOBS};
use result::Result;
use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;
use subscribers;

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
    subs: &Arc<RwLock<subscribers::Subscribers>>,
    recycler: &BlobRecycler,
    consumed: &mut usize,
    r: &BlobReceiver,
    s: &BlobSender,
    cast: &BlobSender,
) -> Result<()> {
    let timer = Duration::new(1, 0);
    let mut dq = r.recv_timeout(timer)?;
    while let Ok(mut nq) = r.try_recv() {
        dq.append(&mut nq)
    }
    {
        //retransmit all leader blocks
        let mut castq = VecDeque::new();
        let rsubs = subs.read().unwrap();
        for b in &dq {
            let p = b.read().unwrap();
            //TODO this check isn't safe against adverserial packets
            //we need to maintain a sequence window
            if p.meta.addr() == rsubs.leader.addr {
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
                castq.push_back(nv);
            }
        }
        if !castq.is_empty() {
            cast.send(castq)?;
        }
    }
    //send a contiguous set of blocks
    let mut contq = VecDeque::new();
    while let Some(b) = dq.pop_front() {
        let b_ = b.clone();
        let mut p = b.write().unwrap();
        let pix = p.get_index()? as usize;
        let w = pix % NUM_BLOBS;
        //TODO, after the block are authenticated
        //if we get different blocks at the same index
        //that is a network failure/attack
        {
            if window[w].is_none() {
                window[w] = Some(b_);
            } else {
                debug!("duplicate blob at index {:}", w);
            }
            loop {
                let k = *consumed % NUM_BLOBS;
                if window[k].is_none() {
                    break;
                }
                contq.push_back(window[k].clone().unwrap());
                window[k] = None;
                *consumed += 1;
            }
        }
    }
    if !contq.is_empty() {
        s.send(contq)?;
    }
    Ok(())
}

pub fn window(
    exit: Arc<AtomicBool>,
    subs: Arc<RwLock<subscribers::Subscribers>>,
    recycler: BlobRecycler,
    r: BlobReceiver,
    s: BlobSender,
    cast: BlobSender,
) -> JoinHandle<()> {
    spawn(move || {
        let mut window = vec![None; NUM_BLOBS];
        let mut consumed = 0;
        loop {
            if exit.load(Ordering::Relaxed) {
                break;
            }
            let _ = recv_window(&mut window, &subs, &recycler, &mut consumed, &r, &s, &cast);
        }
    })
}

fn retransmit(
    subs: &Arc<RwLock<subscribers::Subscribers>>,
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
        let wsubs = subs.read().unwrap();
        for b in &dq {
            let mut mb = b.write().unwrap();
            wsubs.retransmit(&mut mb, sock)?;
        }
    }
    while let Some(b) = dq.pop_front() {
        recycler.recycle(b);
    }
    Ok(())
}

//service to retransmit messages from the leader to layer 1 nodes
//see subscriber.rs for network layer definitions
//window receives blobs from the network
//for any blobs that originated from the leader, we broadcast
//to the rest of the network
pub fn retransmitter(
    sock: UdpSocket,
    exit: Arc<AtomicBool>,
    subs: Arc<RwLock<subscribers::Subscribers>>,
    recycler: BlobRecycler,
    r: BlobReceiver,
) -> JoinHandle<()> {
    spawn(move || loop {
        if exit.load(Ordering::Relaxed) {
            break;
        }
        let _ = retransmit(&subs, &recycler, &r, &sock);
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
        println!("performance: {:?}", fcount / ftime);
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
    use packet::{Blob, BlobRecycler, Packet, PacketRecycler, Packets, PACKET_DATA_SIZE};
    use std::collections::VecDeque;
    use std::io;
    use std::io::Write;
    use std::net::UdpSocket;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, RwLock};
    use std::time::Duration;
    use streamer::{blob_receiver, receiver, responder, retransmitter, window, BlobReceiver,
                   PacketReceiver};
    use subscribers::{Node, Subscribers};

    fn get_msgs(r: PacketReceiver, num: &mut usize) {
        for _t in 0..5 {
            let timer = Duration::new(1, 0);
            match r.recv_timeout(timer) {
                Ok(m) => *num += m.read().unwrap().packets.len(),
                e => println!("error {:?}", e),
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
                e => println!("error {:?}", e),
            }
            if *num == 10 {
                break;
            }
        }
    }

    #[test]
    pub fn window_send_test() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let addr = read.local_addr().unwrap();
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let subs = Arc::new(RwLock::new(Subscribers::new(
            Node::default(),
            Node::new([0; 8], 0, send.local_addr().unwrap()),
        )));
        let resp_recycler = BlobRecycler::default();
        let (s_reader, r_reader) = channel();
        let t_receiver =
            blob_receiver(exit.clone(), resp_recycler.clone(), read, s_reader).unwrap();
        let (s_window, r_window) = channel();
        let (s_cast, r_cast) = channel();
        let t_window = window(
            exit.clone(),
            subs,
            resp_recycler.clone(),
            r_reader,
            s_window,
            s_cast,
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
            assert_eq!(i, w.get_index().unwrap());
            w.meta.size = PACKET_DATA_SIZE;
            w.meta.set_addr(&addr);
            msgs.push_back(b_);
        }
        s_responder.send(msgs).expect("send");
        let mut num = 0;
        get_blobs(r_window, &mut num);
        assert_eq!(num, 10);
        let mut q = r_cast.recv().unwrap();
        while let Ok(mut nq) = r_cast.try_recv() {
            q.append(&mut nq);
        }
        assert_eq!(q.len(), 10);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_responder.join().expect("join");
        t_window.join().expect("join");
    }

    #[test]
    pub fn retransmit() {
        let read = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let send = UdpSocket::bind("127.0.0.1:0").expect("bind");
        let exit = Arc::new(AtomicBool::new(false));
        let subs = Arc::new(RwLock::new(Subscribers::new(
            Node::default(),
            Node::default(),
        )));
        let n3 = Node::new([0; 8], 1, read.local_addr().unwrap());
        subs.write().unwrap().insert(&[n3]);
        let (s_retransmit, r_retransmit) = channel();
        let blob_recycler = BlobRecycler::default();
        let saddr = send.local_addr().unwrap();
        let t_retransmit = retransmitter(
            send,
            exit.clone(),
            subs,
            blob_recycler.clone(),
            r_retransmit,
        );
        let mut bq = VecDeque::new();
        let b = blob_recycler.allocate();
        b.write().unwrap().meta.size = 10;
        bq.push_back(b);
        s_retransmit.send(bq).unwrap();
        let (s_blob_receiver, r_blob_receiver) = channel();
        let t_receiver =
            blob_receiver(exit.clone(), blob_recycler.clone(), read, s_blob_receiver).unwrap();
        let mut oq = r_blob_receiver.recv().unwrap();
        assert_eq!(oq.len(), 1);
        let o = oq.pop_front().unwrap();
        let ro = o.read().unwrap();
        assert_eq!(ro.meta.size, 10);
        assert_eq!(ro.meta.addr(), saddr);
        exit.store(true, Ordering::Relaxed);
        t_receiver.join().expect("join");
        t_retransmit.join().expect("join");
    }

}
